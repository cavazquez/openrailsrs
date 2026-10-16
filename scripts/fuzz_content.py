#!/usr/bin/env python3
"""Offline deterministic mutations of URLs, ZIP metadata and ZIP bytes.

Only synthetic inputs are created. No HTTP calls or installers are executed.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import random
import stat
import zipfile
import download_official_content as content

LIMIT = 65536

def exercise(data):
    text = data.decode('utf-8', errors='replace')
    try:
        source = content.source_from_url(text)
    except (ValueError, UnicodeError):
        pass
    else:
        assert source['url'].startswith('https://')
        assert not source.get('user_source') or source['requires_confirmation']
    try:
        content.validate_redirect('https://github.com/OwnAuthor/OwnRoute/archive/main.zip', text)
    except (ValueError, UnicodeError):
        pass
    else:
        assert content.repository_identity(text) == 'ownauthor/ownroute'
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            members, total = content.safe_members(archive, LIMIT)
            assert total <= LIMIT
            for info, path in members:
                assert path.parts and not path.is_absolute() and '..' not in path.parts
                assert not stat.S_ISLNK(info.external_attr >> 16)
    except (ValueError, zipfile.BadZipFile, NotImplementedError, UnicodeError):
        pass
    # Exercise metadata paths even when mutations invalidate the ZIP directory.
    class Archive:
        def infolist(self):
            return [zipfile.ZipInfo(text)]
    try:
        members, _ = content.safe_members(Archive(), LIMIT)
    except (ValueError, UnicodeError):
        return
    for _, path in members:
        assert path.parts and not path.is_absolute() and '..' not in path.parts
        assert all(':' not in p for p in path.parts)
        assert PurePosixPath(str(path)) == path

def seeds():
    archive = io.BytesIO()
    with zipfile.ZipFile(archive, 'w') as z:
        z.writestr('OwnRoute/ROUTES/Example/route.trk', 'Tr_RouteFile ( )')
    return [b'https://github.com/OwnAuthor/OwnRoute', archive.getvalue(),
            b'../escape', b'package/../outside', b'C:\\outside', b'\x00hidden',
            b'https://github.com/OwnAuthor/OwnRoute\x00', b'.',
            b'https://github.com/OwnAuthor/OwnRoute/archive/refs/heads/feature/own.zip',
            b'https://codeload.github.com/OwnAuthor/OwnRoute/zip/main']

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runs', type=int, default=128)
    parser.add_argument('--seed', type=int, default=1612026)
    parser.add_argument('--out', type=Path, default=Path('tmp/fuzz-content'))
    parser.add_argument('--replay', type=Path)
    args = parser.parse_args()
    if not 1 <= args.runs <= 1000000:
        parser.error('--runs must be 1..1000000')
    args.out.mkdir(parents=True, exist_ok=True)
    if (args.out/'last-input.bin').exists() or (args.out/'result.json').exists():
        parser.error('Choose a fresh --out directory; previous inputs are preserved')
    rng = random.Random(args.seed)
    count = 0
    corpus = seeds()
    if args.replay:
        with args.replay.open('rb') as stream:
            data = stream.read(LIMIT + 1)
        if len(data) > LIMIT:
            parser.error('Replay exceeds input budget')
        corpus = [data]
        args.runs = 1
    for original in corpus:
        for run in range(args.runs):
            data = bytearray(original)
            if run and data:
                for _ in range(rng.randrange(1, 5)):
                    i = rng.randrange(len(data))
                    data[i] = rng.randrange(256)
                if run % 3 == 0:
                    del data[rng.randrange(len(data)):]
            data = bytes(data[:LIMIT])
            (args.out / 'last-input.bin').write_bytes(data)
            (args.out / 'last-case.json').write_text(json.dumps(dict(seed=args.seed, run=count, sha256=hashlib.sha256(data).hexdigest())))
            exercise(data)
            count += 1
    (args.out / 'result.json').write_text(json.dumps(dict(status='pass', cases=count, seed=args.seed, max_input=LIMIT, network=False)))
    print(f'PASS installer fuzz smoke: {count} synthetic cases; no network')

if __name__ == '__main__':
    main()
