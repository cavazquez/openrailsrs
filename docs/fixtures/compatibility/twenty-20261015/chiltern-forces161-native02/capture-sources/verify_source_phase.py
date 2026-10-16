#!/usr/bin/env python3
"""Source-order diagnosis only; this script never generates expected oracle forces."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import struct


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def single(value):
    return struct.unpack('<f', struct.pack('<f', float(value)))[0]


def add(a, b):
    return single(single(a) + single(b))


def mul(a, b):
    return single(single(a) * single(b))


def div(a, b):
    return single(single(a) / single(b))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--capture-dir', type=Path, required=True)
    parser.add_argument('--out-file', type=Path, required=True)
    args = parser.parse_args()
    raw = args.capture_dir
    if args.out_file.exists():
        raise ValueError('Preserve any existing report')
    rich = json.loads((raw / 'native/adhesion-observation.json').read_bytes())
    samples = {0: rich['initial'], **{row['tick']: row['vehicles'] for row in rich['samples']}}
    previous = None
    frame = []
    maximum_force_error = 0.
    maximum_rear_coefficient_error = 0.
    calls = 0
    with (raw / 'native/car-diagnostics.csv').open(newline='') as stream:
        for row in csv.DictReader(stream):
            frame.append(row)
            if len(frame) < 8:
                continue
            tick = int(row['tick'])
            if previous is not None:
                for index, row in enumerate(frame):
                    loco = 0 if index < 7 else 7
                    speed = single(previous[loco]['AbsSpeedMpS'])
                    base = single(samples[tick][0 if loco == 0 else 1]['base_factor'])
                    coefficient = mul(add(div(7.5, add(mul(speed, 3.6), 44)), .161), base)
                    if index == 7:
                        maximum_rear_coefficient_error = max(maximum_rear_coefficient_error, abs(coefficient - single(row['WagonCoefficientFriction'])))
                    radius = single(previous[index]['CurrentCurveRadiusM'])
                    cant = single(previous[index]['SuperElevationM'])
                    gauge = single(row['TrackGaugeM'])
                    rigid = single(row['RigidWheelBaseM'])
                    mass = single(row['MassKG'])
                    car_speed = single(row['AbsSpeedMpS'])
                    calculated = 0.
                    if radius > 0:
                        base_force = div(mul(mul(mul(9.80665, mass), coefficient), add(gauge, rigid)), mul(2, radius))
                        speed_term = mul(1.5, abs(single(div(mul(car_speed, car_speed), radius) - div(mul(9.80665, cant), gauge))))
                        variable = mul(9.80665, mul(mul(mass, div(1, 1000)), speed_term))
                        calculated = add(base_force, variable)
                    maximum_force_error = max(maximum_force_error, abs(calculated - single(row['CurveForceN'])))
                    calls += 1
            previous, frame = frame, []
    if calls != 120016 or maximum_force_error != 0 or maximum_rear_coefficient_error != 0:
        raise ValueError('The diagnosed source phase is not confirmed')
    result = {'schema': 1, 'kind': 'python_single_source_phase_diagnosis_only',
              'source_car_observation_sha256': sha(raw / 'native/car-diagnostics.csv'),
              'source_iir_observation_sha256': sha(raw / 'native/adhesion-observation.json'),
              'diagnostic_script_sha256': sha(Path(__file__)), 'calls': calls,
              'maximum_force_error_n': maximum_force_error,
              'maximum_rear_coefficient_error': maximum_rear_coefficient_error,
              'geometry_phase': 'CurrentCurveRadiusM/SuperElevationM from previous sample; AbsSpeedMpS from current sample before integration',
              'coefficient_phase': 'head current coefficient for cars0..6; rear current coefficient for car7, each using previous AbsSpeedMpS and current observed base factor',
              'genuine_oracle': 'The CSV CurveForceN field is the original DLL measurement. This Python calculation diagnoses the source phase and is not an oracle or expected-force generator.',
              'rust_executed': False, 'closed_trajectory_outputs_prescribed': False}
    args.out_file.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
