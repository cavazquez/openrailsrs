#!/usr/bin/env python3
"""Measure a native import fresh/cold/warm and require byte-identical graphs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--cli', type=Path, default=Path('target/debug/openrailsrs'))
    p.add_argument('--route-root', type=Path, required=True)
    p.add_argument('--out-dir', type=Path, required=True)
    a = p.parse_args()
    repo = Path(__file__).resolve().parents[1]
    out = a.out_dir.resolve(); out.mkdir(parents=True, exist_ok=False)
    cli = a.cli.resolve(strict=True); route = a.route_root.resolve(strict=True)
    rows = []; expected = None
    for name in ('fresh', 'cold', 'warm', 'warm-repeat'):
        destination = out/name
        env = {k:v for k,v in os.environ.items() if not k.startswith('OPENRAILSRS_')}
        env['OPENRAILSRS_PLAYER_DIR'] = str(out/'.private-data')
        if name == 'fresh': env['OPENRAILSRS_SOURCE_CACHE'] = 'off'
        cache_dir=out/'.private-data/cache/source'
        before={str(p):p.stat().st_mtime_ns for p in cache_dir.glob('*.json')}
        start = time.monotonic()
        process = subprocess.run([str(cli), 'import-msts', str(route), '--out-dir', str(destination)], cwd=repo, env=env, capture_output=True, text=True, timeout=180)
        elapsed = time.monotonic()-start
        (out/f'{name}.log').write_text(process.stdout+process.stderr)
        process.check_returncode()
        after={str(p):p.stat().st_mtime_ns for p in cache_dir.glob('*.json')}
        if name != 'fresh' and not after: raise RuntimeError('Native import did not fit or populate the persistent cache')
        if name.startswith('warm') and before != after: raise RuntimeError('Warm import recompiled instead of using the unchanged cache')
        data = (destination/'track.toml').read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if expected is None:expected=digest
        if digest != expected:raise RuntimeError('Cached native import differs from fresh graph and SIGSCR programs')
        rows.append({'mode':name,'elapsed_s':elapsed,'graph_sha256':digest,'graph_bytes':len(data),'cache_files':len(after),'cache_unchanged':bool(after and before==after)})
    (out/'report.json').write_text(json.dumps({'binary_sha256':hashlib.sha256(cli.read_bytes()).hexdigest(),'hardware':platform.uname()._asdict(),'route':str(route),'results':rows,'scope':'same executable, fresh/cold/warm import including native graph, metadata and compiled SIGSCR; rendering is measured separately'},indent=2)+'\n')
    print(json.dumps(rows,indent=2))


if __name__ == '__main__':main()
