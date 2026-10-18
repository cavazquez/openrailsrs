#!/usr/bin/env python3
"""Build an author ZIP and optional HTTPS download descriptor without changing sources."""
import argparse
import hashlib
import json
from pathlib import Path
import os
import tempfile
import zipfile

import content_package as package


def build(source, output, archive_url=None, manifest_output=None):
    root, metadata = package.find_manifest(source.resolve(), required=True)
    package.validate_layout(root, metadata)
    output = output.absolute()
    descriptor = (manifest_output or output.parent / package.MANIFEST).absolute()
    for path in [output, *([descriptor] if archive_url else [])]:
        if path.is_symlink() or path.resolve().is_relative_to(root):
            raise ValueError('Guardá el ZIP y el manifiesto de descarga fuera del paquete fuente')
    if archive_url and output == descriptor:
        raise ValueError('El ZIP y su manifiesto requieren destinos distintos')
    files = sorted(p for p in root.rglob('*')
                   if not any(part in {'.git', '.hg', '.svn', '__pycache__'} for part in p.relative_to(root).parts))
    if len(files) > 500000 or any(p.is_symlink() for p in files):
        raise ValueError('El paquete excede el límite de archivos o contiene enlaces simbólicos')
    if sum(p.stat().st_size for p in files if p.is_file()) > 32 * 1024**3:
        raise ValueError('El paquete excede 32 GiB instalado')
    output.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix='.package-', dir=output.parent)
    os.close(fd)
    try:
        with zipfile.ZipFile(temporary, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
            for path in files:
                archive.write(path, path.relative_to(root).as_posix())
        from download_official_content import safe_members
        with zipfile.ZipFile(temporary) as archive:
            safe_members(archive, 32 * 1024**3)
        if Path(temporary).stat().st_size > 8 * 1024**3:
            raise ValueError('El ZIP excede el límite de descarga de 8 GiB')
        checksum = hashlib.sha256()
        with open(temporary, 'rb') as stream:
            while block := stream.read(256 * 1024):
                checksum.update(block)
        digest = checksum.hexdigest()
        if archive_url:
            metadata = package.package_metadata(metadata) | {'archive': {'url': archive_url, 'sha256': digest}}
            package.validate_manifest(metadata, require_archive=True)
        os.replace(temporary, output)
        if archive_url:
            descriptor.parent.mkdir(parents=True, exist_ok=True)
            fd, pending = tempfile.mkstemp(prefix='.manifest-', dir=descriptor.parent)
            try:
                with os.fdopen(fd, 'w') as stream:
                    stream.write(json.dumps(metadata, ensure_ascii=False, indent=2) + '\n')
                os.replace(pending, descriptor)
            finally:
                Path(pending).unlink(missing_ok=True)
        return {'zip': str(output), 'sha256': digest,
                'manifest': str(descriptor) if archive_url else None}
    finally:
        Path(temporary).unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--archive-url', help='URL HTTPS pública del ZIP que vas a publicar')
    parser.add_argument('--manifest-output', type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(build(args.source, args.output, args.archive_url, args.manifest_output), ensure_ascii=False))
    except (ValueError, OSError) as error:
        parser.error(str(error))


if __name__ == '__main__':
    main()
