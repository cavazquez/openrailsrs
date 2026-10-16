#!/usr/bin/env python3
"""Project actual DLL force fields; derive only the labelled coefficient input."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import shutil
import struct

EXPECTED_TRACE = 'ad9e690435d784df243c8a49bfdb37137c6e5f98f8941045adfde27e4500a7e2'
EXPECTED_CARS = '255304a33d0f365a23cabdb8fdc43c60c01e0cbb4ef9e540d153a21f4c781356'
EXPECTED_IIR = '3392f303463a7d54b48cda1bfd8bd260855ffcc952fb77747e9bf3eea71a1103'


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


def frames(path):
    frame = []
    with path.open(newline='') as stream:
        for row in csv.DictReader(stream):
            frame.append(row)
            if len(frame) == 8:
                yield int(row['tick']), frame
                frame = []
    if frame:
        raise ValueError('Incomplete eight-car frame')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--capture-dir', type=Path, required=True)
    parser.add_argument('--protocol', type=Path, required=True)
    parser.add_argument('--out-dir', type=Path, required=True)
    args = parser.parse_args()
    raw, output = args.capture_dir, args.out_dir
    output.mkdir(parents=True, exist_ok=False)
    cars = raw / 'native/car-diagnostics.csv'
    iir = raw / 'native/adhesion-observation.json'
    if sha(raw / 'native/trace.csv') != EXPECTED_TRACE or sha(cars) != EXPECTED_CARS or sha(iir) != EXPECTED_IIR:
        raise ValueError('The genuine original observations differ')
    protocol = json.loads(args.protocol.read_bytes())
    if protocol['force_absolute_tolerance_n'] != .01 or not protocol['previous_service_motion_thresholds_unchanged']:
        raise ValueError('Predeclared comparison protocol differs')
    rich = json.loads(iir.read_bytes())
    samples = {0: rich['initial'], **{row['tick']: row['vehicles'] for row in rich['samples']}}
    maximum = (-1, None)
    for tick, frame in frames(cars):
        value = sum(float(row['CurveForceN']) for row in frame)
        if value > maximum[0]:
            maximum = value, tick
    ticks = {1, 2, 20, 500, 560, 600, 800, 1000, 2000, 2595, 3000, 3500, 4000, 4400, 4800, 5000,
             5500, 6000, 6500, 7000, 7500, 8000, 9000, 10000, 11000, 12000, 13000, 14000, 14900, 15000, 15001, 15002}
    if maximum[1] not in ticks:
        ticks.remove(14000)
        ticks.add(maximum[1])
    needed = ticks | {tick - 1 for tick in ticks} | {0}
    context = {tick: frame for tick, frame in frames(cars) if tick in needed}
    cases = []
    for tick in sorted(ticks):
        frame, previous = context[tick], context[tick - 1]
        for index, row in enumerate(frame):
            locomotive = 0 if index < 7 else 7
            speed = single(previous[locomotive]['AbsSpeedMpS'])
            base = single(samples[tick][0 if locomotive == 0 else 1]['base_factor'])
            coefficient = mul(add(div(7.5, add(mul(speed, 3.6), 44)), .161), base)
            inputs = {key: single(value) for key, value in {
                'mass_kg': row['MassKG'], 'speed_mps': row['AbsSpeedMpS'],
                'radius_m': previous[index]['CurrentCurveRadiusM'], 'cant_m': previous[index]['SuperElevationM'],
                'gauge_m': row['TrackGaugeM'], 'rigid_wheelbase_m': row['RigidWheelBaseM'],
                'wagon_coefficient': coefficient}.items()}
            cases.append({'tick': tick, 'time_s': float(row['time_s']), 'vehicle': index, 'input': inputs,
                          'expected_curve_force_n': single(row['CurveForceN']),
                          'coefficient_context': {'locomotive_vehicle': locomotive, 'previous_abs_speed_mps': speed,
                                                  'current_base_factor': base,
                                                  'post_update_rear_coefficient_observed': single(row['WagonCoefficientFriction'])},
                          'post_update_geometry_not_force_input': {'radius_m': single(row['CurrentCurveRadiusM']),
                                                                   'cant_m': single(row['SuperElevationM'])}})
    fixture = {'schema': 1, 'kind': 'genuine_or161_curve_force_projection', 'reference_version': '1.6.1',
               'native_car_observation_sha256': sha(cars), 'native_adhesion_observation_sha256': sha(iir),
               'protocol_sha256': sha(args.protocol), 'tolerance_n_abs': .01, 'cases': cases,
               'coverage': {'frames': len(ticks), 'cars': 8, 'calls': len(cases),
                            'nonzero_force_calls': sum(case['expected_curve_force_n'] != 0 for case in cases),
                            'native_maximum_train_curve_force_n': maximum[0], 'native_maximum_train_curve_tick': maximum[1],
                            'selected_ticks': sorted(ticks), 'radius_m_values': sorted({case['input']['radius_m'] for case in cases}),
                            'initial_tick0_state_used_as_context_only': True}}
    (output / 'curve-force-sparse.json').write_text(json.dumps(fixture, separators=(',', ':'), allow_nan=False) + '\n')
    shutil.copyfile(args.protocol, output / 'curve-force-protocol.json')
    receipt = {'schema': 1, 'kind': 'deterministic_projection_of_original_fields',
               'extractor_sha256': sha(Path(__file__)), 'source_car_observation_sha256': sha(cars),
               'source_adhesion_observation_sha256': sha(iir), 'source_trace_sha256': EXPECTED_TRACE,
               'fixture_sha256': sha(output / 'curve-force-sparse.json'), 'protocol_sha256': sha(args.protocol),
               'rows': len(cases), 'expected_force_method': 'direct original CurveForceN field, never a formula',
               'derived_input': 'Single source expression for the shared wagon coefficient; labelled per-locomotive context',
               'rust_executed': False}
    (output / 'extraction.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
