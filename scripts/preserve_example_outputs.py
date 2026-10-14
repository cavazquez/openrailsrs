#!/usr/bin/env python3
"""Preserve tracked example traces overwritten by historical integration tests."""
import argparse, hashlib, json, shutil, subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def snapshot(destination):
    destination.mkdir(parents=True, exist_ok=False)
    files = subprocess.check_output(['git', 'ls-files', '-z', 'examples'], cwd=ROOT).decode().split('\0')
    entries = []
    for relative in files:
        source = ROOT / relative
        if source.suffix not in ('.csv', '.json') or not source.is_file() or source.is_symlink():
            continue
        output = destination / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, output)
        entries.append({'path': relative, 'sha256': hashlib.sha256(output.read_bytes()).hexdigest()})
    (destination / 'manifest.json').write_text(json.dumps(entries))

def restore(destination):
    entries = json.loads((destination / 'manifest.json').read_text())
    for entry in entries:
        relative = Path(entry['path'])
        if relative.is_absolute() or '..' in relative.parts or relative.parts[0] != 'examples':
            raise ValueError('Invalid example output path')
        source = destination / relative
        if hashlib.sha256(source.read_bytes()).hexdigest() != entry['sha256']:
            raise ValueError('Preserved output checksum mismatch: ' + str(relative))
    for entry in entries:
        source = destination / entry['path']
        target = ROOT / entry['path']
        if target.is_symlink():
            raise ValueError('Example output became a symbolic link: ' + entry['path'])
        if not target.exists() or hashlib.sha256(target.read_bytes()).hexdigest() != entry['sha256']:
            shutil.copy2(source, target)

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['snapshot', 'restore'])
    parser.add_argument('destination', type=Path)
    args = parser.parse_args()
    globals()[args.operation](args.destination.resolve())
