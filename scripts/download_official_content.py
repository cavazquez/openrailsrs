#!/usr/bin/env python3
"""Install curated Open Rails packages alongside existing content, never over it.

Only entries in the bundled official catalogue are accepted. Preserve authors'
licences. Downloads are not executed; audit/import is a separate player choice.
"""
import argparse
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
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
CATALOG = Path(__file__).with_name('official-catalog.json')
if not CATALOG.is_file():
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


def player_data_dir(env=None, platform=None):
    env = os.environ if env is None else env
    platform = os.name if platform is None else platform
    if env.get('OPENRAILSRS_PLAYER_DIR'):
        return Path(env['OPENRAILSRS_PLAYER_DIR']).absolute()
    if env.get('SNAP_USER_COMMON') and Path(env['SNAP_USER_COMMON']).is_absolute():
        return Path(env['SNAP_USER_COMMON']) / 'openrailsrs'
    if platform == 'nt' and env.get('LOCALAPPDATA'):
        return Path(env['LOCALAPPDATA']) / 'openrailsrs'
    if env.get('XDG_DATA_HOME') and Path(env['XDG_DATA_HOME']).is_absolute():
        return Path(env['XDG_DATA_HOME']) / 'openrailsrs'
    if env.get('HOME'):
        import sys
        suffix = 'Library/Application Support/openrailsrs' if sys.platform == 'darwin' else '.local/share/openrailsrs'
        return Path(env['HOME']) / suffix
    return Path(tempfile.gettempdir()) / ('openrailsrs-' + str(os.getpid()))


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
        commit_data = json.loads(response.read(1024 * 1024))
        commit = commit_data['sha']
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('Commit inválido')
    # Resolve the author's current branch on each update. Record the identity
    # of this download so old/new editions coexist; do not pin future updates.
    return f'https://codeload.github.com/{repo}/zip/{commit}', dict(
        repository=repo, commit=commit, published_at=metadata.get('pushed_at'),
        commit_date=commit_data.get('commit', {}).get('committer', {}).get('date'))


def installed_match(entry, destination, revision=None, headers=None):
    for path in sorted(destination.glob(entry['id'] + '-*')):
        try:
            if path.is_symlink() or not path.is_dir():
                continue
            metadata = path / 'openrailsrs-content.json'
            if metadata.is_symlink() or metadata.stat().st_size > 8 * 1024**2:
                continue
            manifest = json.loads(metadata.read_text())
            if not isinstance(manifest, dict) or manifest.get('advertised_url') != entry['url'] or manifest.get('package') != entry['name']:
                continue
            same_revision = (revision and isinstance(manifest.get('revision'), dict)
                and all(revision.get(k) == manifest['revision'].get(k) for k in ('repository', 'commit')))
            same_entity = (headers and headers.get('ETag') and headers.get('ETag') == manifest.get('source_entity_tag'))
            if same_revision or same_entity:
                emit('complete', path=str(path), manifest=manifest, reused=True)
                return path
        except (OSError, ValueError, TypeError):
            continue
    return None


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
        existing = installed_match(entry, destination, revision)
        if existing:
            return existing
        with tempfile.TemporaryDirectory(prefix='.' + entry['id'] + '-', dir=destination) as temp:
            stage = Path(temp)
            archive = stage / 'download.zip'
            digest, done, last = hashlib.sha256(), 0, 0.
            with opener(url) as response, archive.open('xb') as output:
                existing = installed_match(entry, destination, revision, response.headers)
                if existing:
                    return existing
                length = response.headers.get('Content-Length')
                last_modified = response.headers.get('Last-Modified')
                entity_tag = response.headers.get('ETag')
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
                metadata = final / 'openrailsrs-content.json'
                if final.is_symlink() or not final.is_dir() or metadata.is_symlink() or metadata.stat().st_size > 8 * 1024**2:
                    raise ValueError('La instalación existente tiene metadatos inseguros')
                manifest = json.loads(metadata.read_text())
                if not isinstance(manifest, dict):
                    raise ValueError('Manifiesto de instalación inválido')
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
            source_date = (revision.get('published_at') or revision.get('commit_date')) if revision else None
            if not source_date and last_modified:
                try:
                    source_date = parsedate_to_datetime(last_modified).astimezone(timezone.utc).isoformat()
                except (ValueError, TypeError, OverflowError):
                    pass
            manifest = dict(version=2, package=entry['name'], catalog_source=catalog()['source_url'],
                author=entry['author'], advertised_url=entry['url'], download_url=url,
                revision=revision, download_sha256=checksum, download_bytes=done,
                source_date=source_date, source_date_kind='repository' if revision else 'last-modified',
                source_last_modified=last_modified, source_entity_tag=entity_tag,
                downloaded_at=datetime.now(timezone.utc).isoformat(),
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
                   default=player_data_dir() / 'official-content')
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
