#!/usr/bin/env python3
"""Drive production save/load transfers with different formations on hardware GPU."""
import argparse
import hashlib
import json
import re
import tomllib
from pathlib import Path
from check_player_expansion import options, settings
from check_viewer_streaming import run_checkpoint, validate_formation


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--viewer', type=Path, required=True)
    p.add_argument('--out-dir', type=Path, required=True)
    p.add_argument('--cases', nargs='+', choices=['control', 'coupling'])
    a = p.parse_args()
    a.repo = Path(__file__).resolve().parents[1]
    a.viewer = a.viewer.resolve()
    a.out_dir = a.out_dir.resolve()
    a.out_dir.mkdir(parents=True, exist_ok=True)
    reports = {}
    for name, action, cars in [('control', 'take_control', 3), ('coupling', 'couple_service', 4)]:
        if a.cases and name not in a.cases:
            continue
        source = a.repo / f'examples/traffic_passing/scenario_{name}.toml'
        original = tomllib.loads(source.read_text())
        text = source.read_text()
        for key, value in [('path', original['route']['path']), ('consist', original['train']['consist'])]:
            text = re.sub(r'(?m)^' + key + r'\s*=\s*' + re.escape(json.dumps(value)), key + ' = ' + json.dumps(str((source.parent / value).resolve())), text, count=1)
        for entry in original['extra_trains']:
            value = entry['consist']
            text = text.replace('consist=' + json.dumps(value), 'consist=' + json.dumps(str((source.parent / value).resolve())))
        path = a.out_dir / f'{name}.toml'
        path.write_text(text)
        settings(a, quality_limits={'enabled': False, 'target_fps': 60, 'min_level': 0, 'max_level': 3})
        o = options(a, path, Path(tomllib.loads(text)['route']['path']))
        # The donor branch lies outside the owned WORLD tile. Require the full
        # train transforms and shaders; this case makes no scenery claim.
        o.require_world_scenery = False
        o.capture_or_focus = True
        o.follow = 'orbit'
        o.after_ready = False
        o.timeout_s = 70
        o.capture_delay_s = 50
        o.ready_frames = 10
        actions = [{'at_s': 1, 'operation': {'action': action, 'service': 0}},
                   {'at_s': 12, 'operation': {'action': 'pause', 'value': True}},
                   {'at_s': 15, 'operation': {'action': 'restore'}}]
        o.capture_actions = json.dumps(actions)
        r = run_checkpoint(o, name, 0, False)
        assert len(r['equipment_actions']) == len(actions), r['equipment_actions']
        assert all(e['result'] == {'Ok': None} for e in r['equipment_actions']), r['equipment_actions']
        ops = r['traffic_operations']
        assert ops['cars'] == cars and ops['coupled'] == cars, ops
        if name == 'control':
            assert ops['player_service'] == 'other' and ops['control_history'] == ['other'], ops
            assert ops['other_services'][0]['id'] == 'Jugador' and ops['other_services'][0]['cars'] == 2, ops
        else:
            assert ops['coupling_receipts'] == 1 and not ops['other_services'], ops
        r['formation_validation'] = validate_formation(r, cars)
        r['binary_sha256'] = hashlib.sha256(a.viewer.read_bytes()).hexdigest()
        reports[name] = r
        (a.out_dir / 'report.json').write_text(json.dumps(reports, indent=2) + '\n')
        print('PASS ' + name, flush=True)


if __name__ == '__main__':
    main()
