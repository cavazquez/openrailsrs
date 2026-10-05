#!/usr/bin/env python3
"""Verify the fixed original DLL formation capture without certifying a full service."""
import csv,hashlib,json,math
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
BASELINE=ROOT/'examples/baselines/class47'

def verify_capture(directory,pin):
    manifest=json.loads((directory/'manifest.json').read_text())
    if manifest['reference_version']!=pin['version'] or manifest['reference_commit']!=pin['commit']:
        raise ValueError('Formation uses a different original version')
    if manifest['full_service'] or manifest['duration_s']!=250 or manifest['step_s']!=.05:
        raise ValueError('Different formation capture scope')
    for entry in pin['binaries']:
        if manifest['binaries'].get(entry['name'])!=entry['sha256']:
            raise ValueError('Different original formation DLL')
    for name,digest in manifest['files'].items():
        path=(directory/name).resolve()
        if not path.is_relative_to(directory.resolve()) or hashlib.sha256(path.read_bytes()).hexdigest()!=digest:
            raise ValueError('Formation capture changed: '+name)
    if hashlib.sha256((ROOT/'tools/openrails-reference/FormationCapture.cs').read_bytes()).hexdigest()!=manifest['helper_sha256']:
        raise ValueError('Formation capture client changed; review and recapture separately')
    with (directory/'capture/trace.csv').open() as stream:
        rows=list(csv.DictReader(stream))
    if len(rows)!=5001:
        raise ValueError('Incomplete formation trace')
    for i,row in enumerate(rows):
        if abs(float(row['time_s'])-i*.05)>1e-8 or not all(math.isfinite(float(row[k])) for k in ('velocity_mps','odometer_m','throttle','brake','diesel_rpm','brake_pipe_psi','cylinder_psi')):
            raise ValueError('Invalid formation telemetry')
    initial=json.loads((directory/'capture/initial.json').read_text())
    if initial['cars']!=7 or len(initial['vehicles'])!=7:
        raise ValueError('Different native formation')
    return {'samples':len(rows),'full_service':False,'vehicles':7}
