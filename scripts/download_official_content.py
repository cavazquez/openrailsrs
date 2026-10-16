#!/usr/bin/env python3
"""Install curated Open Rails packages alongside existing content, never over it.

Catalogued origins and player-confirmed GitHub repositories are supported.
Preserve authors' licences. Downloads are never executed; audit/import follows installation.
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


def source_from_url(url, confirmed=False):
    """Resolve only supported author origins. Never fetch an HTML landing page."""
    if not isinstance(url, str) or len(url) > 2048 or any(c.isspace() or ord(c) < 32 or ord(c) == 127 for c in url):
        raise ValueError('URL inválida: pegá un enlace HTTPS completo sin espacios')
    u = urlparse(url)
    validate_download_url(url)
    if u.query or u.fragment:
        raise ValueError('Pegá el enlace del repositorio o ZIP sin parámetros ni fragmentos')
    ref = None
    if u.hostname == 'github.com':
        match = re.fullmatch(r'/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+?)(?:\.git)?/?', u.path)
        archive = re.fullmatch(r'/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/archive/(?:refs/heads/|refs/tags/)?([A-Za-z0-9_.\-/]+)\.zip', u.path)
        if not match and not archive:
            raise ValueError('Pegá la raíz del repositorio del autor o su ZIP de GitHub')
        match = match or archive
        owner, name = match.group(1), match.group(2)
        if owner in ('.', '..') or name in ('.', '..'):
            raise ValueError('Identidad de repositorio inválida')
        if archive:
            ref = archive.group(3)
        normalized = f'https://github.com/{owner}/{name}.git'
    elif u.hostname in ('static.openrails.org', 'ts-files.com') and u.path.lower().endswith('.zip'):
        normalized = url
    else:
        raise ValueError('Este proveedor no admite instalación por URL; descargá desde la página original del autor')
    entries = catalog()['routes']
    for entry in entries:
        if any(normalized.lower() == source['url'].lower()
               for source in [entry, *entry.get('previousSources', [])]):
            result = entry.copy()
            result['requires_confirmation'] = False
            result['input_url'] = url
            if ref:
                result['requested_ref'] = ref
            return result
    if u.hostname != 'github.com':
        raise ValueError('El ZIP no está verificado en el catálogo. Usá su repositorio original de GitHub o la instalación manual')
    result = dict(name=f'{owner}/{name}', author={'name': owner}, compensation='free',
                  url=normalized, downloadSize=0, installSize=0,
                  source_url=normalized, input_url=url, user_source=True,
                  requires_confirmation=not confirmed,
                  original_confirmed=bool(confirmed))
    # User URLs have their own namespace. Slugging owner/repository would merge
    # distinct origins (a-b/c and a/b-c) and even collide with catalogue names.
    result['id'] = 'url-github-' + hashlib.sha256(normalized.lower().encode()).hexdigest()
    if ref:
        result['requested_ref'] = ref
    return result


def registered_sources():
    path = player_data_dir() / 'original-url-sources.json'
    if not path.is_file() or path.is_symlink() or path.stat().st_size > 128 * 1024:
        return []
    values = json.loads(path.read_text())
    if not isinstance(values, list) or len(values) > 32:
        raise ValueError('Registro de orígenes inválido')
    result = []
    for item in values:
        if not isinstance(item, dict) or item.get('original_confirmed') is not True:
            continue
        source = source_from_url(item.get('url'), confirmed=True)
        if source.get('user_source'):
            result.append(source)
    return result


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
    # The GUI consumes bounded JSON lines. Keep the full inventory on disk;
    # a large valid package must not lose its completion event to that limit.
    manifest = fields.get('manifest')
    if phase == 'complete' and isinstance(manifest, dict) and len(json.dumps(manifest).encode()) > 64 * 1024:
        summary = manifest.copy()
        for key in ('stock_files', 'routes'):
            if isinstance(summary.get(key), list):
                summary[key + '_count'] = len(summary[key])
                summary[key] = summary[key][:16]
        summary['inventory_truncated_in_event'] = True
        fields['manifest'] = summary
        fields['manifest_path'] = str(Path(fields['path']) / 'openrailsrs-content.json')
    print(json.dumps(dict(phase=phase, **fields), ensure_ascii=False), flush=True)


def cancelled(path):
    if path and path.exists():
        raise InterruptedError('Descarga cancelada; el contenido anterior se conserva')


def validate_download_url(url):
    if not isinstance(url, str) or len(url)>2048 or any(c.isspace() or ord(c)<32 or ord(c)==127 for c in url):
        raise ValueError('URL de descarga inválida')
    u = urlparse(url)
    if (u.scheme != 'https' or u.hostname not in DOWNLOAD_HOSTS
            or u.username or u.password or u.port not in (None, 443)):
        raise ValueError('Descarga o redirección fuera de los proveedores HTTPS del catálogo')


def repository_identity(url):
    u = urlparse(url)
    match = re.match(r'^/(?:repos/)?([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)(?:/|$)', u.path)
    if u.hostname in ('github.com', 'api.github.com', 'codeload.github.com') and match:
        return '/'.join(match.groups()).removesuffix('.git').casefold()
    return None


def validate_redirect(source, target):
    validate_download_url(target)
    origin = urlparse(source)
    redirected = urlparse(target)
    source_repo = repository_identity(source)
    target_repo = repository_identity(target)
    if source_repo:
        if target_repo == source_repo:
            return
        # Explicit author renames recorded by the bundled catalogue are allowed;
        # arbitrary transfers to another repository require reviewing a new URL.
        for entry in catalog()['routes']:
            known = {repository_identity(s['url']) for s in [entry, *entry.get('previousSources', [])]}
            if source_repo in known and target_repo in known:
                return
        raise ValueError('La redirección cambia el repositorio del autor; revisá su URL original')
    if origin.hostname != redirected.hostname:
        raise ValueError('La redirección cambia el servidor original del ZIP')


class CuratedRedirectHandler(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # urllib otherwise sends the redirected GET before validating response.url.
        try:
            validate_redirect(req.full_url, newurl)
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
    if not isinstance(metadata, dict) or not isinstance(metadata.get('full_name'), str) or not isinstance(metadata.get('default_branch'), str):
        raise ValueError('El enlace no devuelve un repositorio público válido; revisá el origen o si requiere iniciar sesión')
    if metadata['full_name'].casefold() != repo.casefold() and not any(
            repo.casefold() == repository_identity(old['url'])
            and metadata['full_name'].casefold() == repository_identity(current['url'])
            for current in catalog()['routes'] for old in current.get('previousSources', [])):
        raise ValueError('El repositorio cambió de origen. Revisá la URL actual del autor antes de descargar.')
    repo, branch = metadata['full_name'], entry.get('requested_ref', metadata['default_branch'])
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repo):
        raise ValueError('Identidad del repositorio inválida')
    from urllib.parse import quote
    with https_open('https://api.github.com/repos/' + repo + '/commits/' + quote(branch, safe='')) as response:
        commit_data = json.loads(response.read(1024 * 1024))
        commit = commit_data.get('sha') if isinstance(commit_data, dict) else None
    if not isinstance(commit, str) or not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('Commit inválido')
    # Resolve the author's current branch on each update. Record the identity
    # of this download so old/new editions coexist; do not pin future updates.
    return f'https://codeload.github.com/{repo}/zip/{commit}', dict(
        repository=repo, commit=commit, published_at=metadata.get('pushed_at'),
        commit_date=commit_data.get('commit', {}).get('committer', {}).get('date'))


def inspect_source(entry):
    """Validate an accessible repository without downloading its archive."""
    preview = entry.copy()
    if provider(entry) == 'github':
        _, revision = resolve_download(entry)
        preview['resolved_repository'] = revision['repository']
        preview['resolved_commit'] = revision['commit']
        preview['source_date'] = revision.get('published_at') or revision.get('commit_date')
    preview['package_type'] = 'repository' if provider(entry) == 'github' else 'zip'
    return preview


def installed_match(entry, destination, revision=None, headers=None):
    sources = [entry, *entry.get('previousSources', [])]
    # Author-renamed repositories retain their old package identity on disk.
    # Reuse an old copy only when its recorded canonical repository/commit
    # matches the author's current revision; leave other editions untouched.
    paths = {path for source in sources
             for path in destination.glob(source['id'] + '-*')}
    for path in sorted(paths):
        try:
            if path.is_symlink() or not path.is_dir():
                continue
            metadata = path / 'openrailsrs-content.json'
            if metadata.is_symlink() or metadata.stat().st_size > 8 * 1024**2:
                continue
            manifest = json.loads(metadata.read_text())
            if not isinstance(manifest, dict) or not any(
                    manifest.get('advertised_url') == source['url']
                    and manifest.get('package') == source['name'] for source in sources):
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
    members, paths, directories, total = [], set(), {}, 0
    for index, info in enumerate(archive.infolist()):
        if index >= 500000:
            raise ValueError('El ZIP excede el número de recursos autorizado')
        name = info.filename.replace('\\', '/')
        if '\x00' in info.orig_filename or info.file_size < 0 or info.compress_size < 0:
            raise ValueError('Nombre o tamaño inválido en ZIP')
        path = PurePosixPath(name)
        mode = info.external_attr >> 16
        if (not name or not path.parts or '\x00' in name or path.is_absolute() or '..' in path.parts
                or any(':' in part for part in path.parts) or stat.S_ISLNK(mode)
                or (stat.S_IFMT(mode) and not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)))):
            raise ValueError(f'Ruta insegura en ZIP: {name}')
        if any(part.casefold() in MANAGED_NAMES for part in path.parts):
            raise ValueError(f'El ZIP incluye metadatos reservados al importador: {name}')
        # casefold expands ß to ss, conflating distinct authored filenames.
        # Chiltern v4 supplies both DFußballfeld.ace and DFussballfeld.ace.
        # Authors sometimes split CONSISTS/Consists or OpenRails/Openrails
        # into directory aliases. Merge directories only; reject any aliased
        # file, or a file used as a directory, before writing any content.
        normalized = PurePosixPath()
        for part in path.parts[:-1]:
            candidate = normalized / part
            key = str(candidate).lower()
            if key in paths:
                raise ValueError(f'Archivo usado como carpeta en ZIP: {name}')
            normalized = directories.setdefault(key, candidate)
        path = normalized / path.name
        key = str(path).lower()
        if key in paths or (not info.is_dir() and key in directories):
            raise ValueError(f'Recurso duplicado o ambiguo por mayúsculas: {name}')
        if info.flag_bits & 1:
            raise ValueError('El ZIP requiere contraseña')
        if info.is_dir():
            if key in directories:
                continue
            directories[key] = path
        else:
            paths.add(key)
        total += info.file_size
        if total > max_install:
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
            written = 0
            with archive.open(info) as source, target.open('xb') as output:
                while block := source.read(256 * 1024):
                    cancelled(cancel_file)
                    written += len(block)
                    if written > info.file_size or done + len(block) > max_install:
                        raise ValueError('Los datos extraídos exceden el tamaño autorizado')
                    output.write(block)
                    done += len(block)
                    if time.monotonic() - last > 0.2:
                        emit('extract', done_bytes=done, total_bytes=total)
                        last = time.monotonic()
            if written != info.file_size:
                raise ValueError('Recurso ZIP truncado respecto de su tamaño declarado')
        return total


def discover_content(directory):
    routes = []
    for path in directory.rglob('*'):
        if path.is_file() and path.suffix.casefold() == '.trk':
            parent = path.parent
            names = {p.name.casefold() for p in parent.iterdir()}
            if 'world' in names:
                routes.append(str(parent.relative_to(directory)))
    if not routes and not discover_stock(directory):
        raise ValueError('El paquete no contiene una ruta nativa con WORLD ni vehículos ENG/WAG; puede requerir un instalador del autor')
    return sorted(set(routes))


def discover_stock(directory):
    return sorted(str(p.relative_to(directory)) for p in directory.rglob('*')
                  if p.is_file() and p.suffix.casefold() in ('.eng', '.wag'))


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
                final_url = response.geturl() if hasattr(response, 'geturl') else url
                validate_redirect(url, final_url)
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
            stock_files = discover_stock(unpacked)
            if len(stock_files) > 20000:
                raise ValueError('El paquete contiene demasiados vehículos')
            # Preserve relative references and all author licence files.
            source_date = (revision.get('published_at') or revision.get('commit_date')) if revision else None
            if not source_date and last_modified:
                try:
                    source_date = parsedate_to_datetime(last_modified).astimezone(timezone.utc).isoformat()
                except (ValueError, TypeError, OverflowError):
                    pass
            manifest = dict(version=2, package=entry['name'],
                catalog_source=entry.get('source_url', catalog()['source_url']),
                author=entry['author'], advertised_url=entry['url'], original_url=entry.get('input_url', entry['url']), download_url=url,
                final_download_url=final_url,
                revision=revision, download_sha256=checksum, download_bytes=done,
                source_date=source_date, source_date_kind='repository' if revision else 'last-modified',
                source_last_modified=last_modified, source_entity_tag=entity_tag,
                downloaded_at=datetime.now(timezone.utc).isoformat(),
                install_bytes=installed, routes=routes, stock_files=stock_files,
                origin_verification='player_confirmed' if entry.get('user_source') else 'catalogue',
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
    p.add_argument('--url', help='Repositorio original o ZIP directo de un proveedor admitido')
    p.add_argument('--inspect-url', action='store_true', help='Resolver el origen sin descargar')
    p.add_argument('--confirm-original', action='store_true', help='Confirmar que un origen fuera del catálogo pertenece al autor')
    p.add_argument('--revision', help='Rama, etiqueta o commit del repositorio original')
    p.add_argument('--destination', type=Path,
                   default=Path(os.environ.get('OPENRAILSRS_CONTENT_DIR', player_data_dir() / 'official-content')))
    p.add_argument('--cancel-file', type=Path)
    a = p.parse_args()
    data = catalog()
    if a.list:
        print(json.dumps(data, ensure_ascii=False, indent=2))
        return
    if a.url:
        try:
            entry = source_from_url(a.url, a.confirm_original)
            if a.inspect_url:
                print(json.dumps(inspect_source(entry), ensure_ascii=False))
                return
            if entry.get('requires_confirmation'):
                p.error('Este origen no está en el catálogo. Verificá que sea el repositorio original del autor y usá --confirm-original')
        except (ValueError, OSError) as error:
            p.error(f'No se pudo resolver el origen: {error}. Revisá el enlace público y la conexión.')
    else:
        entry = next((e for e in [*data['routes'], *registered_sources()] if e['id'] == a.package
                  or any(source['id'] == a.package for source in e.get('previousSources', []))), None)
    if entry is None:
        p.error('Seleccioná un id del catálogo mediante --package; --list muestra las opciones')
    if a.revision:
        if provider(entry) != 'github' or len(a.revision) > 256 or not re.fullmatch(r'[A-Za-z0-9_.\-/]+', a.revision):
            p.error('Revisión inválida')
        entry = entry | {'requested_ref': a.revision}
    try:
        install(entry, a.destination, a.cancel_file)
    except Exception as error:
        message = str(error)
        if isinstance(error, zipfile.BadZipFile):
            message = 'El archivo no es un ZIP válido: puede ser una página web, requerir inicio de sesión o estar corrupto. Obtené el archivo directo del autor.'
        emit('error', message=message)
        raise SystemExit(1)


if __name__ == '__main__':
    main()
