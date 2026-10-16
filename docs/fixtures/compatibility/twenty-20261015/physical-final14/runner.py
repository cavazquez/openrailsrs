import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import time

root = Path.cwd()
base = root / 'tmp/twenty-20261015'
sha = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()
build = json.loads((base / 'build-14-result.json').read_text())
assert build['status'] == 'pass'
sources = json.loads((base / 'runtime-before-build14.json').read_text())
fixtures = json.loads((base / 'compile-fixtures-before-build14.json').read_text())
protected = json.loads((base / 'protected-traces-before.json').read_text())
before = {**sources, **fixtures}
assert all(sha(root / p) == h for p, h in before.items())
assert all(sha(root / p) == h for p, h in build['binaries'].items())
binary = root / 'target/debug/openrailsrs'
commands = [
    ('chiltern-service', ['python3', 'scripts/run_oracles.py', '--suite', 'service', '--out-dir', str(base / 'service-geometry14')]),
    ('historical-short', ['python3', 'scripts/run_oracles.py', '--out-dir', str(base / 'historical-short14')]),
    ('class47', ['python3', 'scripts/run_oracles.py', '--suite', 'class47', '--out-dir', str(base / 'class47-final14')]),
    ('station-service', [str(binary), 'play-service', 'examples/chiltern_local/scenario.toml', '--out-dir', str(base / 'station-service14')]),
    ('lirr', ['python3', str(base / 'lirr-replay-from-native.py'), '--native-dir', str(base / 'lirr-native-07'), '--binary', str(binary), '--replay-binary', str(root / 'target/debug/examples/activity_trajectory'), '--captured-pin', str(base / 'openrails-reference-captured07.toml'), '--out-dir', str(base / 'lirr-replay-12')]),
    ('steam', [str(root / 'target/debug/examples/steam_native_compare'), '--capture-dir', str(base / 'physical-native-05'), '--model', 'native-or161', '--static-context-capture', str(base / 'steam-native-cycle03'), '--out-dir', str(base / 'steam-native-compare10')]),
]
report = {'schema': 1, 'status': 'running', 'build_receipt_sha256': sha(base / 'build-14-result.json'), 'runner_sha256': sha(__file__), 'commands': []}
receipt = base / 'physical-final14-result.json'
assert not receipt.exists(), 'Preserve earlier attempts; use a new output name'
for name, command in commands:
    log = base / f'physical-final14-{name}.log'
    assert not log.exists()
    entry = {'name': name, 'command': command, 'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat()}
    report['commands'].append(entry)
    receipt.write_text(json.dumps(report, indent=2) + '\n')
    started = time.monotonic()
    with log.open('w') as stream:
        result = subprocess.run(command, cwd=root, stdin=subprocess.DEVNULL, stdout=stream, stderr=subprocess.STDOUT)
    entry.update(exit_code=result.returncode, wall_time_s=time.monotonic() - started, log_sha256=sha(log))
    print(f'{name}: exit={result.returncode}', flush=True)
    receipt.write_text(json.dumps(report, indent=2) + '\n')
report['inputs_changed'] = [p for p, h in before.items() if sha(root / p) != h]
report['binaries_changed'] = [p for p, h in build['binaries'].items() if sha(root / p) != h]
report['human_traces_unchanged'] = {p: sha(root / p) == h for p, h in protected.items()}
report['status'] = 'pass' if not report['inputs_changed'] and not report['binaries_changed'] and all(report['human_traces_unchanged'].values()) and all(c['exit_code'] == 0 for c in report['commands']) else 'fail'
report['completed_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
report['scope'] = 'Historical native references and authored station service plus complete LIRR and bounded80s steam; not a substitute for the workspace-wide final gate.'
receipt.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({'status': report['status'], 'receipt': str(receipt)}), flush=True)
raise SystemExit(0 if report['status'] == 'pass' else 1)
