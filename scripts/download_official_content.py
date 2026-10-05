#!/usr/bin/env python3
"""Install curated Open Rails packages alongside existing content, never over it.

Only entries in the bundled official catalogue are accepted. Preserve authors'
licences. Downloads are not executed; audit/import is a separate player choice.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import tempfile
import time
from urllib.parse import urlparse
from urllib.request import HTTPRedirectHandler, Request, build_opener
import zipfile

ROOT = Path(__file__).resolve().parents[1]
CATALOG = ROOT / 'docs/fixtures/content/official-catalog.json'
CATALOG_PAGE = 'https://www.openrails.org/download/content/'
MAX_DOWNLOAD = 8 * 1024**3
MAX_INSTALL = 32 * 1024**3
DOWNLOAD_HOSTS = frozenset(('static.openrails.org', 'ts-files.com',
                           'github.com', 'api.github.com', 'codeload.github.com'))
MANAGED_NAMES = frozenset(('openrailsrs-content.json', 'openrailsrs-prepared.json',
                          'openrailsrs-audit.json', 'openrailsrs-import'))


def catalog():
    data = json.loads(CATALOG.read_text())
    for entry in data['routes']:
        entry['id'] = re.sub(r'[^a-z0-9]+', '-', entry['name'].lower()).strip('-')
        entry['automatic'] = provider(entry) is not None
    return data


def provider(entry):
    if entry['compensation'] != 'free':
        return None
    u = urlparse(entry['url'])
    if u.scheme != 'https' or u.username or u.password or u.port not in (None, 443):
        return None
    if u.hostname == 'github.com' and re.fullmatch(r'/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\.git', u.path):
        return 'github'
    if u.hostname in ('static.openrails.org', 'ts-files.com') and u.path.lower().endswith('.zip'):
        return 'zip'
    return None


def emit(phase, **fields):
    print(json.dumps(dict(phase=phase, **fields), ensure_ascii=False), flush=True)


def cancelled(path):
    if path and path.exists():
        raise InterruptedError('Descarga cancelada; el contenido anterior se conserva')


def validate_download_url(url):
    u = urlparse(url)
    if (u.scheme != 'https' or u.hostname not in DOWNLOAD_HOSTS
            or u.username or u.password or u.port not in (None, 443)):
        raise ValueError('Descarga o redirección fuera de los proveedores HTTPS del catálogo')


class CuratedRedirectHandler(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # urllib otherwise sends the redirected GET before validating response.url.
        try:
            validate_download_url(newurl)
        except ValueError:
            fp.close()
            raise
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def https_open(url):
    validate_download_url(url)
    response = build_opener(CuratedRedirectHandler()).open(
        Request(url, headers={'User-Agent': 'openrailsrs-content/1'}), timeout=20)
    return response


def resolve_download(entry):
    if provider(entry) == 'zip':
        return entry['url'], None
    if provider(entry) != 'github':
        raise ValueError('Este paquete se obtiene en la página del autor; no tiene instalación automática')
    repo = urlparse(entry['url']).path.removeprefix('/').removesuffix('.git')
    with https_open('https://api.github.com/repos/' + repo) as response:
        metadata = json.loads(response.read(1024 * 1024))
    repo, branch = metadata['full_name'], metadata['default_branch']
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repo):
        raise ValueError('Identidad del repositorio inválida')
    from urllib.parse import quote
    with https_open('https://api.github.com/repos/' + repo + '/commits/' + quote(branch, safe='')) as response:
        commit = json.loads(response.read(1024 * 1024))['sha']
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('Commit inválido')
    # Resolve redirects in author repository identity (Chiltern v2 -> v4),
    # then freeze this specific download at a commit, not a moving branch.
    return f'https://codeload.github.com/{repo}/zip/{commit}', dict(repository=repo, commit=commit)


def safe_members(archive, max_install):
    members, paths, total = [], set(), 0
    for info in archive.infolist():
        name = info.filename.replace('\\', '/')
        path = PurePosixPath(name)
        mode = info.external_attr >> 16
        if (not name or '\x00' in name or path.is_absolute() or '..' in path.parts
                or any(':' in part for part in path.parts) or stat.S_ISLNK(mode)
                or (stat.S_IFMT(mode) and not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)))):
            raise ValueError(f'Ruta insegura en ZIP: {name}')
        if any(part.casefold() in MANAGED_NAMES for part in path.parts):
            raise ValueError(f'El ZIP incluye metadatos reservados al importador: {name}')
        key = str(path).casefold()
        if key in paths:
            raise ValueError(f'Recurso duplicado o ambiguo por mayúsculas: {name}')
        paths.add(key)
        if info.flag_bits & 1:
            raise ValueError('El ZIP requiere contraseña')
        total += info.file_size
        if total > max_install or len(paths) > 500000:
            raise ValueError('El ZIP excede el tamaño de instalación autorizado')
        members.append((info, path))
    return members, total


def extract(archive_path, destination, max_install=MAX_INSTALL, cancel_file=None):
    with zipfile.ZipFile(archive_path) as archive:
        members, total = safe_members(archive, max_install)
        if shutil.disk_usage(destination.parent).free < total + 512 * 1024**2:
            raise ValueError('No hay espacio suficiente para descomprimir')
        done, last = 0, 0.
        for info, relative in members:
            cancelled(cancel_file)
            target = destination.joinpath(*relative.parts)
            if info.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(info) as source, target.open('xb') as output:
                while block := source.read(256 * 1024):
                    cancelled(cancel_file)
                    output.write(block)
                    done += len(block)
                    if time.monotonic() - last > 0.2:
                        emit('extract', done_bytes=done, total_bytes=total)
                        last = time.monotonic()
        return total


def discover_content(directory):
    routes = []
    for path in directory.rglob('*'):
        if path.is_file() and path.suffix.casefold() == '.trk':
            parent = path.parent
            names = {p.name.casefold() for p in parent.iterdir()}
            if 'world' in names:
                routes.append(str(parent.relative_to(directory)))
    if not routes:
        raise ValueError('El paquete no contiene una ruta nativa con WORLD; puede requerir un instalador del autor')
    return sorted(set(routes))


def install(entry, destination, cancel_file=None, opener=https_open):
    destination = destination.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    lock = destination / ('.' + entry['id'] + '.lock')
    # Exclusive installation of this package. A stale lock is never deleted
    # implicitly; another process could still be writing this directory.
    lock.mkdir()
    try:
        advertised = entry.get('installSize', 0) + entry.get('downloadSize', 0)
        if shutil.disk_usage(destination).free < advertised * 1.1 + 512 * 1024**2:
            raise ValueError('No hay espacio suficiente para descargar y descomprimir este paquete')
        cancelled(cancel_file)
        emit('resolve', name=entry['name'])
        url, revision = resolve_download(entry)
        with tempfile.TemporaryDirectory(prefix='.' + entry['id'] + '-', dir=destination) as temp:
            stage = Path(temp)
            archive = stage / 'download.zip'
            digest, done, last = hashlib.sha256(), 0, 0.
            with opener(url) as response, archive.open('xb') as output:
                length = response.headers.get('Content-Length')
                total = int(length) if length and length.isdigit() else None
                if total and total > MAX_DOWNLOAD:
                    raise ValueError('La descarga supera el máximo de 8 GiB')
                while block := response.read(256 * 1024):
                    cancelled(cancel_file)
                    done += len(block)
                    if done > MAX_DOWNLOAD:
                        raise ValueError('La descarga supera el máximo de 8 GiB')
                    if done % (8 * 1024**2) < len(block) and shutil.disk_usage(stage).free < 512 * 1024**2:
                        raise ValueError('El disco se quedó sin espacio para continuar')
                    digest.update(block)
                    output.write(block)
                    if time.monotonic() - last > 0.2:
                        emit('download', done_bytes=done, total_bytes=total)
                        last = time.monotonic()
                if total is not None and done != total:
                    raise ValueError('Descarga incompleta')
            checksum = digest.hexdigest()
            final = destination / (entry['id'] + '-' + checksum[:16])
            if final.exists():
                manifest = json.loads((final / 'openrailsrs-content.json').read_text())
                if manifest['download_sha256'] != checksum:
                    raise ValueError('La instalación existente tiene otra identidad')
                emit('complete', path=str(final), manifest=manifest, reused=True)
                return final
            unpacked = stage / 'content'
            unpacked.mkdir()
            installed = extract(archive, unpacked, cancel_file=cancel_file)
            cancelled(cancel_file)
            routes = discover_content(unpacked)
            # Preserve relative references and all author licence files.
            manifest = dict(version=1, package=entry['name'], catalog_source=catalog()['source_url'],
                author=entry['author'], advertised_url=entry['url'], download_url=url,
                revision=revision, download_sha256=checksum, download_bytes=done,
                install_bytes=installed, routes=routes,
                audit='pending; resource completeness does not guarantee systems parity')
            (unpacked / 'openrailsrs-content.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n')
            cancelled(cancel_file)
            os.rename(unpacked, final)
            emit('complete', path=str(final), manifest=manifest, reused=False)
            return final
    finally:
        lock.rmdir()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--list', action='store_true')
    p.add_argument('--package', help='Stable id from --list, e.g. demo-model-1 or chiltern')
    p.add_argument('--destination', type=Path,
                   default=Path(os.environ.get('OPENRAILSRS_PLAYER_DIR', ROOT / 'player-data')) / 'official-content')
    p.add_argument('--cancel-file', type=Path)
    a = p.parse_args()
    data = catalog()
    if a.list:
        print(json.dumps(data, ensure_ascii=False, indent=2))
        return
    entry = next((e for e in data['routes'] if e['id'] == a.package), None)
    if entry is None:
        p.error('Seleccioná un id del catálogo mediante --package; --list muestra las opciones')
    try:
        install(entry, a.destination, a.cancel_file)
    except Exception as error:
        emit('error', message=str(error))
        raise SystemExit(1)


if __name__ == '__main__':
    main()
