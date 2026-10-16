#!/usr/bin/env python3
"""Publish additive native body diagnostics; leave historical captures untouched."""
from pathlib import Path
import gzip
import hashlib
import json
import shutil

ROOT = Path('/home/cristian/repos/propios/ProyectoOpenRails/openrailsrs')
PREP = Path(__file__).resolve().parent
RAW = ROOT / 'tmp/twenty-20261015/chiltern-adhesion-native02'
PUBLIC = ROOT / 'docs/fixtures/compatibility/twenty-20261015/chiltern-forces161-native02'
VERIFY = ROOT / 'tmp/twenty-20261015/chiltern-curve-extractor-verification02'


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def read(path):
    return json.loads(path.read_bytes())


def write(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False) + '\n')


assert sorted(path.name for path in PUBLIC.iterdir()) == ['curve-force-protocol.json', 'curve-force-sparse.json']
assert sha(PUBLIC / 'curve-force-sparse.json') == 'dfdd41d166da63f5e2a3a0c681f2effe3e11aa272d5aac91a457d36a50ae47c5'
assert sha(PUBLIC / 'curve-force-protocol.json') == 'b553a66098b7e538fa15855db787050383af2db59d51935cba093586d20a8b16'
assert all(sha(PUBLIC / name) == sha(VERIFY / name) for name in ['curve-force-protocol.json', 'curve-force-sparse.json'])
receipt = read(RAW / 'receipt.json')
assert receipt['inputs_unchanged'] and receipt['native_rows'] == 15003 and receipt['car_diagnostic_rows'] == 120024
assert receipt['native_trace_sha256'] == sha(RAW / 'native/trace.csv') == 'ad9e690435d784df243c8a49bfdb37137c6e5f98f8941045adfde27e4500a7e2'
assert receipt['rich_observation_sha256'] == '3392f303463a7d54b48cda1bfd8bd260855ffcc952fb77747e9bf3eea71a1103'
release = read(RAW / 'wine-isolation/release.json')
assert release['remaining_processes'] == release['forbidden_drivers_seen'] == []
protected = read(ROOT / 'tmp/twenty-20261015/protected-traces-before.json')
assert all(sha(ROOT / path) == expected for path, expected in protected.items())
old_public = ROOT / 'docs/fixtures/compatibility/twenty-20261015/chiltern-adhesion161-native01'
assert sha(old_public / 'publication.json') == 'f88e6314c8f1390ab465ea4f2c05f7efefa4bf9e1a0161d7175b04a72086df2a'
assert sha(ROOT / 'examples/chiltern_local/physics/native-adhesion-inputs.csv') == '355be676ad0d64120aa66014efd3f31f18d5848e863451d61ecc01d5570155a7'
files = {name: {'sha256': sha(PUBLIC / name), 'transformation': 'genuine sparse projection with separately declared protocol'}
         for name in ['curve-force-protocol.json', 'curve-force-sparse.json']}
for name in ['receipt.json', 'compile.log', 'capture.log', 'trace-integrity.json', 'diagnostics-integrity.json',
             'native/initial.json', 'native/outcome.json', 'wine-isolation/isolation.json', 'wine-isolation/release.json',
             'selected-force-geometry.json', 'source-phase-diagnostic.json']:
    target = PUBLIC / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(RAW / name, target)
    files[name] = {'sha256': sha(target), 'transformation': 'literal captured or post-capture diagnostic file; no sanitization'}
for source in sorted((RAW / 'capture-sources').iterdir()):
    target = PUBLIC / 'capture-sources' / source.name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)
    files[target.relative_to(PUBLIC).as_posix()] = {'sha256': sha(target), 'transformation': 'literal own capture source or pin snapshot used by this attempt'}
for name in ['extract_curve_reference.py', 'verify_source_phase.py']:
    target = PUBLIC / 'capture-sources' / name
    shutil.copyfile(PREP / name, target)
    files[target.relative_to(PUBLIC).as_posix()] = {'sha256': sha(target), 'transformation': 'own deterministic extraction or diagnostic script executed after capture'}
shutil.copyfile(VERIFY / 'extraction.json', PUBLIC / 'extraction.json')
files['extraction.json'] = {'sha256': sha(PUBLIC / 'extraction.json'), 'transformation': 'literal successful deterministic extractor receipt'}
source = RAW / 'native/car-diagnostics.csv'
target = PUBLIC / 'native/car-diagnostics.csv.gz'
with source.open('rb') as incoming, target.open('wb') as output:
    with gzip.GzipFile(filename='', fileobj=output, mode='wb', mtime=0, compresslevel=9) as zipped:
        shutil.copyfileobj(incoming, zipped)
with gzip.open(target, 'rb') as decoded:
    assert hashlib.file_digest(decoded, 'sha256').hexdigest() == sha(source)
files['native/car-diagnostics.csv.gz'] = {'sha256': sha(target), 'raw_sha256': sha(source),
                                         'raw_bytes': source.stat().st_size, 'transformation': 'lossless gzip level9/mtime0; decoded bytes unchanged'}
readme = '''# Chiltern original all-car diagnostics02

This is an additive observation of the original OR1.6.1 Chiltern service, in its historical paused context. The original20column CSV retains all15003rows and SHA256 `ad9e690435d784df243c8a49bfdb37137c6e5f98f8941045adfde27e4500a7e2`. The IIR observation is also byte-identical to observation01; neither historical reference was replaced.

The lossless CSV contains120024car rows, with direct field reads after every Simulator.Update and one initial sample. CurveForceN is the raw body resistance actually used by Train; CurveForceNFiltered is a separate smoothed audio value. Wind and tunnel forces are zero throughout this captured service. Stored brake friction coefficient fields absent from the DLL are empty, never recalculated by an observer getter.

Force sampleN uses radius and physical cant from sampleN-1. The current locomotive coefficient is published in car order: head for cars0..6, rear for car7. Its speed comes from that locomotive's previous AbsSpeedMpS cache. Source-phase diagnosis reproduces120016original force fields exactly using Single operations, but this Python calculation is a diagnosis, not an oracle or an executed Rust test. Expected forces in the sparse fixture are exclusively direct DLL fields.

The sparse fixture has256calls at32post-update ticks, all8cars, including zero-radius boundaries, 250m curves and the native maximum train curve resistance6482.75N. The separate protocol declares0.01N absolute force error before the first Rust comparison; existing service motion tolerances remain unchanged. The coefficient input is explicitly source-derived from independently observed environmental base and prior speed; the final rear train coefficient is directly observed as an additional cross-check.

Only the isolated subsystem test may prescribe these mass/speed/geometry/coefficient inputs. Full trajectories independently import authored TDB/TSECTION geometry and evolve their own positions and forces. No force, radius, cant, speed, pose, pressure, filter output or wheel state recorded here is an input to a closed trajectory.

Original ENG/WAG/CON/ACT/SRV/PAT/TDB/TRK/DLL assets, the Wine runtime, six human trace contents and duplicate IIR/environment inputs are excluded. `receipt.json` and the isolation/release receipts preserve the actual capture metadata and an empty private-process release. Publication01, canonical environmental CSV and human trace hashes remain unchanged. No Rust execution or workspace gate result is claimed by this publication.
'''
(PUBLIC / 'README.md').write_text(readme)
files['README.md'] = {'sha256': sha(PUBLIC / 'README.md'), 'transformation': 'own scope and phase documentation'}
shutil.copyfile(__file__, PUBLIC / 'publish.py')
files['publish.py'] = {'sha256': sha(PUBLIC / 'publish.py'), 'transformation': 'own publisher executed for this directory'}
publication = {'schema': 1, 'kind': 'original_all_car_force_and_geometry_additive_observation',
               'status': 'captured_with_sparse_fixture_not_compared_in_rust_by_publisher',
               'reference_version': '1.6.1', 'reference_commit': receipt['reference_commit'],
               'original_capture_receipt_sha256': sha(RAW / 'receipt.json'),
               'native_trace_sha256': receipt['native_trace_sha256'], 'native_trace_rows': 15003, 'native_trace_columns': 20,
               'native_trace_public_path': 'examples/baselines/chiltern_local/capture/trace.csv',
               'native_trace_byte_identical': True, 'native_all_car_rows': 120024,
               'native_car_observation_sha256': sha(source),
               'parent_observation01_sha256': receipt['rich_observation_sha256'],
               'parent_iir_public_path': 'docs/fixtures/compatibility/twenty-20261015/chiltern-adhesion161-native01/native/adhesion-observation.json.gz',
               'parent_publication01_sha256_unchanged': sha(old_public / 'publication.json'),
               'all_original_inputs_unchanged': receipt['inputs_unchanged'],
               'human_trace_sha256_unchanged': protected, 'private_wine_release_sha256': sha(RAW / 'wine-isolation/release.json'),
               'private_processes_remaining': [], 'forbidden_drivers_seen': [], 'original_assets_bundled': False,
               'closed_trajectory_observed_outputs_prescribed': False, 'isolated_force_tolerance_n_abs': .01,
               'original_service_limits_unchanged': True, 'rust_executed_by_publisher': False, 'global_gate_claim': False,
               'files': files,
               'excluded': ['original content and DLL assets', 'runtime and private Wine prefix', 'human trace contents',
                            'duplicate original20column trace', 'duplicate parent IIR observation and runtime environmental CSV']}
write(PUBLIC / 'publication.json', publication)
additions = [{'path': path.relative_to(ROOT).as_posix(), 'sha256': sha(path)} for path in sorted(PUBLIC.rglob('*')) if path.is_file()]
plan = {'schema': 1, 'kind': 'additive_original_chiltern_all_car_diagnostic_pins_only', 'additions': additions,
        'replacements': [], 'gitattributes_additions': [PUBLIC.relative_to(ROOT).as_posix() + '/** -text'],
        'pin_applied': False, 'historical_capture01_or_native_trace_changed': False}
write(ROOT / 'tmp/twenty-20261015/chiltern-forces02-pin-additions.json', plan)
fragment = '\n# Additive original Chiltern body-force/geometry diagnostics02; old service trace byte-identical.\n'
fragment += ''.join('\n[[files]]\npath = "' + row['path'] + '"\nsha256 = "' + row['sha256'] + '"\n' for row in additions)
(ROOT / 'tmp/twenty-20261015/chiltern-forces02-pin-additions.toml').write_text(fragment)
print(json.dumps({'public_directory': PUBLIC.relative_to(ROOT).as_posix(), 'files': len(additions),
                  'publication_sha256': sha(PUBLIC / 'publication.json'), 'gzip_sha256': sha(target),
                  'gzip_bytes': target.stat().st_size, 'pin_additions': len(additions), 'pin_applied': False}, indent=2))
