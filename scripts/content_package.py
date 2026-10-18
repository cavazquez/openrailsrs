"""Versioned author packages and direct public HTTPS transport (standard library)."""
import http.client
import ipaddress
import json
from pathlib import Path, PurePosixPath
import re
import socket
from urllib.parse import urlparse
from urllib.request import HTTPSHandler, HTTPRedirectHandler, ProxyHandler, Request, build_opener

MANIFEST = 'openrailsrs-package.json'
MAX_MANIFEST = 128 * 1024
ASSET_SUFFIXES = frozenset(('.eng', '.wag', '.con', '.s', '.sd', '.ace', '.dds',
                            '.cvf', '.sms', '.wav', '.inc', '.dat', '.ktx2'))


def global_unicast(address):
    ip = ipaddress.ip_address(address)
    if isinstance(ip, ipaddress.IPv6Address):
        if ip.ipv4_mapped:
            return global_unicast(ip.ipv4_mapped)
        if ip in ipaddress.IPv6Network('64:ff9b::/96'):
            return global_unicast(ipaddress.IPv4Address(int(ip) & 0xffffffff))
        if ip.sixtofour or ip.teredo or ip in ipaddress.IPv6Network('64:ff9b:1::/48'):
            return False
    return ip.is_global and not ip.is_multicast and not ip.is_reserved


def public_url(url):
    if (not isinstance(url, str) or len(url) > 2048
            or any(c.isspace() or ord(c) < 32 or ord(c) == 127 for c in url)):
        raise ValueError('URL HTTPS del paquete inválida')
    try:
        parsed = urlparse(url)
        valid = (parsed.scheme == 'https' and parsed.hostname and not parsed.username
                 and not parsed.password and parsed.port in (None, 443)
                 and not parsed.query and not parsed.fragment and '\\' not in url)
    except ValueError:
        valid = False
    if not valid:
        raise ValueError('El paquete requiere HTTPS sin credenciales, parámetros ni fragmentos')
    host = parsed.hostname.lower().rstrip('.')
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        if ('.' not in host or host.endswith(('.localhost', '.local', '.internal'))
                or not re.fullmatch(r'[a-z0-9.-]+', host)):
            raise ValueError('El paquete debe estar en un servidor público')
    else:
        if not global_unicast(address):
            raise ValueError('No se admiten direcciones locales o privadas para los paquetes')
    return parsed


def public_addresses(host, port):
    addresses = []
    for _, _, _, _, sockaddr in socket.getaddrinfo(host, port, type=socket.SOCK_STREAM):
        value = sockaddr[0]
        ip = ipaddress.ip_address(value)
        if not global_unicast(ip):
            raise ValueError('El servidor del paquete resuelve a una dirección no pública')
        if value not in addresses:
            addresses.append(value)
    if not addresses or len(addresses) > 16:
        raise ValueError('No se pudo resolver un servidor público para el paquete')
    return addresses


class PublicConnection(http.client.HTTPSConnection):
    def connect(self):
        # Connect to the exact validated address. A second DNS lookup would
        # allow rebinding between validation and the actual HTTP connection.
        if self._tunnel_host:
            raise ValueError('No se admiten túneles para descargar paquetes')
        last_error = None
        for address in public_addresses(self.host, self.port):
            sock = None
            try:
                sock = socket.create_connection((address, self.port), self.timeout)
                self.sock = self._context.wrap_socket(sock, server_hostname=self.host)
                return
            except OSError as error:
                last_error = error
                if sock is not None:
                    sock.close()
        raise last_error or OSError('No se pudo conectar con el servidor del paquete')


class PublicHTTPSHandler(HTTPSHandler):
    def https_open(self, request):
        public_url(request.full_url)
        return self.do_open(PublicConnection, request, context=self._context)


def same_origin(source, target):
    origin, redirected = public_url(source), public_url(target)
    if origin.hostname.lower() != redirected.hostname.lower():
        raise ValueError('La redirección cambia el servidor del paquete; usá su URL directa')


class PublicRedirectHandler(HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, newurl):
        try:
            same_origin(request.full_url, newurl)
        except ValueError:
            fp.close()
            raise
        return super().redirect_request(request, fp, code, message, headers, newurl)


def public_open(url):
    public_url(url)
    # Proxies are disabled for this transport: all sockets must use the public
    # addresses checked above, with normal TLS hostname/certificate validation.
    return build_opener(ProxyHandler({}), PublicRedirectHandler(), PublicHTTPSHandler()).open(
        Request(url, headers={'User-Agent': 'openrailsrs-content/1'}), timeout=20)


def relative_path(value):
    if (not isinstance(value, str) or not value or len(value) > 512
            or '\\' in value or any(ord(c) < 32 for c in value)):
        raise ValueError('Ruta relativa inválida en el manifiesto')
    path = PurePosixPath(value)
    if (path.is_absolute() or any(p in ('.', '..', '') or ':' in p for p in value.split('/'))):
        raise ValueError('El manifiesto no puede salir de la raíz del paquete')
    return path


def _text(value, name, limit=256):
    if (not isinstance(value, str) or not value.strip() or len(value) > limit
            or any(ord(c) < 32 and c not in '\n\t' for c in value)):
        raise ValueError(f'Campo {name} inválido en el manifiesto')
    return value


def validate_manifest(value, require_archive=False):
    if not isinstance(value, dict) or value.get('format') != 'openrailsrs-package' or type(value.get('format_version')) is not int or value['format_version'] != 1:
        raise ValueError('Formato de paquete desconocido: se requiere openrailsrs-package versión1')
    allowed = {'format', 'format_version', 'id', 'name', 'version', 'author', 'license',
               'contents', 'dependencies', 'archive'}
    if set(value) - allowed:
        raise ValueError('Campos desconocidos en el manifiesto del paquete')
    if not isinstance(value.get('id'), str) or not re.fullmatch(r'[a-z0-9][a-z0-9._-]{0,79}', value['id']):
        raise ValueError('El id del paquete debe usar letras minúsculas, números, punto, guion o guion bajo')
    for key in ('name', 'version', 'license'):
        _text(value.get(key), key)
    if not isinstance(value.get('author'), dict) or set(value['author']) != {'name'}:
        raise ValueError('El autor requiere un nombre')
    _text(value['author']['name'], 'author.name')
    contents = value.get('contents')
    if (not isinstance(contents, list) or not contents or len(contents) > 2
            or any(c not in ('ROUTES', 'TRAINS') for c in contents)
            or len(set(contents)) != len(contents)):
        raise ValueError('contents debe declarar ROUTES, TRAINS o ambas')
    dependencies = value.get('dependencies', [])
    if not isinstance(dependencies, list) or len(dependencies) > 16:
        raise ValueError('Lista de dependencias inválida')
    ids = set()
    for dependency in dependencies:
        if (not isinstance(dependency, dict)
                or set(dependency) - {'id', 'name', 'files', 'instructions', 'source_url'}):
            raise ValueError('Dependencia inválida')
        _text(dependency.get('id'), 'dependency.id', 80)
        if dependency['id'] in ids:
            raise ValueError('Dependencia duplicada')
        ids.add(dependency['id'])
        _text(dependency.get('name'), 'dependency.name')
        _text(dependency.get('instructions'), 'dependency.instructions', 1024)
        files = dependency.get('files')
        if not isinstance(files, list) or not files or len(files) > 64:
            raise ValueError('Cada dependencia requiere entre 1 y 64 archivos relativos')
        for file in files:
            path = relative_path(file)
            if path.parts[0] not in ('TRAINS', 'ROUTES', 'GLOBAL') or path.suffix.lower() not in ASSET_SUFFIXES:
                raise ValueError('Archivo de dependencia fuera de TRAINS,ROUTES o GLOBAL, o tipo no admitido')
        if 'source_url' in dependency:
            public_url(dependency['source_url'])
    if require_archive or 'archive' in value:
        archive = value.get('archive')
        if not isinstance(archive, dict) or set(archive) != {'url', 'sha256'}:
            raise ValueError('archive requiere url y sha256')
        if not public_url(archive['url']).path.lower().endswith('.zip'):
            raise ValueError('archive.url debe ser un ZIP HTTPS directo')
        if not isinstance(archive['sha256'], str) or not re.fullmatch('[a-f0-9]{64}', archive['sha256']):
            raise ValueError('archive.sha256 debe contener 64 dígitos hexadecimales minúsculos')
    return value


def read_remote_manifest(url, opener=None):
    opener = opener or public_open
    with opener(url) as response:
        same_origin(url, response.geturl() if hasattr(response, 'geturl') else url)
        data = response.read(MAX_MANIFEST + 1)
    return validate_manifest(load_manifest(data), require_archive=True)


def load_manifest(data):
    if len(data) > MAX_MANIFEST:
        raise ValueError('El manifiesto supera 128 KiB')
    def unique(pairs):
        value = {}
        for key, item in pairs:
            if key in value:
                raise ValueError('El manifiesto contiene campos JSON duplicados')
            value[key] = item
        return value
    try:
        return json.loads(data.decode('utf-8-sig'), object_pairs_hook=unique)
    except (RecursionError, UnicodeError) as error:
        raise ValueError('El manifiesto JSON no es válido') from error


def package_metadata(value):
    return {k: v for k, v in value.items() if k != 'archive'}


def find_manifest(directory, required=False):
    # Accept a root or one wrapper directory (e.g. a GitHub ZIP). A nested
    # dependency cannot silently become the root manifest of its parent pack.
    candidates = [directory / MANIFEST]
    candidates += [p / MANIFEST for p in directory.iterdir() if p.is_dir()]
    found = [p for p in candidates if p.is_file()]
    if len(found) > 1:
        raise ValueError('El paquete contiene varios manifiestos de raíz')
    if not found:
        if required:
            raise ValueError(f'El ZIP requiere {MANIFEST} en la raíz o dentro de una carpeta contenedora')
        return None, None
    path = found[0]
    if (path.is_symlink() or path.parent.is_symlink()
            or not path.resolve().is_relative_to(directory.resolve())
            or path.stat().st_size > MAX_MANIFEST):
        raise ValueError('Manifiesto del paquete inseguro o demasiado grande')
    with path.open('rb') as stream:
        value = load_manifest(stream.read(MAX_MANIFEST + 1))
    return path.parent, validate_manifest(value)


def validate_layout(root, manifest):
    members = list(root.rglob('*'))
    if len(members) > 500000 or any(p.is_symlink() or not (p.is_file() or p.is_dir()) for p in members):
        raise ValueError('El paquete contiene enlaces o archivos especiales, o excede su límite de recursos')
    declared = set(manifest['contents'])
    children = {p.name.upper(): p for p in root.iterdir() if p.is_dir()}
    actual = set(children) & {'ROUTES', 'TRAINS'}
    if actual != declared:
        raise ValueError('Las carpetas ROUTES/TRAINS no coinciden con contents')
    routes, stock, trainsets = [], [], []
    if 'ROUTES' in declared:
        for trk in children['ROUTES'].rglob('*'):
            if trk.is_file() and trk.suffix.lower() == '.trk':
                if len(trk.relative_to(root).parts) != 3:
                    raise ValueError('Cada TRK debe estar en ROUTES/NombreDeRuta/')
                if any(p.is_dir() and p.name.lower() == 'world' for p in trk.parent.iterdir()):
                    routes.append(trk.parent.relative_to(root).as_posix())
        if not routes:
            raise ValueError('ROUTES requiere al menos una ruta con TRK y WORLD')
    if 'TRAINS' in declared:
        trains = children['TRAINS']
        files = [p for p in trains.rglob('*') if p.is_file()]
        if not any(p.suffix.lower() in ASSET_SUFFIXES for p in files):
            raise ValueError('TRAINS no contiene formaciones, vehículos, cabinas ni sonidos')
        stock = [p.relative_to(root).as_posix() for p in files if p.suffix.lower() in ('.eng', '.wag')]
        if any(len(PurePosixPath(p).parts) < 4 or PurePosixPath(p).parts[1].upper() != 'TRAINSET' for p in stock):
            raise ValueError('Los vehículos deben conservar TRAINS/TRAINSET/NombreDelTren/')
        trainsets = [p.relative_to(root).as_posix() for p in trains.iterdir()
                     if p.is_dir() and p.name.lower() == 'trainset']
        if any(p.suffix.lower() in ('.eng', '.wag') for p in files) and not trainsets:
            raise ValueError('Los vehículos deben conservar TRAINS/TRAINSET')
    return sorted(set(routes)), sorted(stock), trainsets
