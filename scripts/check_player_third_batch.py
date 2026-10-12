#!/usr/bin/env python3
"""Render third-batch fixtures with actual hardware and isolated player data.

No author files are changed. This checks lighting, mixed precipitation and
shared resource reports; manual listening and physical joystick checks are
separate from this graphics acceptance run.
"""
import argparse
import json
from pathlib import Path
from check_player_expansion import options, settings
from check_viewer_streaming import run_checkpoint, validate_formation


def scenario(args, name, hour=21, weather=False, traffic=False):
    lab = args.repo / 'examples/player_lab'
    text = (lab / 'scenario.toml').read_text()
    text = text.replace('[scenario]\n', f'[scenario]\nstart_time_s = {hour * 3600}.0\n', 1)
    text = text.replace('path = "route"', 'path = ' + json.dumps(str(lab / 'route')))
    text = text.replace('consist = "consists/lab.con"', 'consist = ' + json.dumps(str(lab / 'consists/lab.con')))
    if weather:
        text += '''\n[simulation.weather_track]\nschema = 1\nseed = 42\n[[simulation.weather_track.phases]]\nstart_s = 0.0\ntransition_s = 0.0\nkind = "rain"\nintensity = 0.5\ntemperature_c = 3.0\nwind_mps = [1.0, 0.3]\n[[simulation.weather_track.sectors]]\nedge = "e1"\nstart_m = 20.0\nend_m = 3500.0\nblend_m = 20.0\n[simulation.weather_track.sectors.weather]\nstart_s = 0.0\ntransition_s = 0.0\nkind = "snow"\nintensity = 0.5\ntemperature_c = 0.5\nwind_mps = [1.0, 0.3]\n'''
    if traffic:
        parallel = args.out_dir / 'scenarios' / 'parallel'
        parallel.mkdir(exist_ok=True, parents=True)
        track = (lab / 'route/track.toml').read_text() + '\n[[nodes]]\nid = "p0"\nx_m = 0.0\ny_m = 12.0\n[[nodes]]\nid = "p1"\nx_m = 10000.0\ny_m = 12.0\n[[edges]]\nid = "parallel"\nfrom = "p0"\nto = "p1"\nlength_m = 10000.0\nspeed_limit_kmh = 80.0\n'
        (parallel / 'track.toml').write_text(track)
        text = text.replace(json.dumps(str(lab / 'route')), json.dumps(str(parallel)))
        con = json.dumps(str(lab / 'consists/lab.con'))
        text += f'''\n[[extra_trains]]\nid = "fog-service"\nconsist = {con}\nstart = "p0"\ndestination = "p1"\nstart_offset_m = 80.0\nstart_time_s = 0.0\ncruise_throttle = 0.4
output_csv = "traffic.csv"\n'''
    path = args.out_dir / 'scenarios' / (name + '.toml')
    path.parent.mkdir(exist_ok=True, parents=True)
    path.write_text(text)
    return path


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--viewer', type=Path, required=True)
    p.add_argument('--out-dir', type=Path, required=True)
    p.add_argument('--cases', nargs='+')
    a = p.parse_args()
    a.repo = Path(__file__).resolve().parents[1]
    a.viewer, a.out_dir = a.viewer.resolve(), a.out_dir.resolve()
    a.out_dir.mkdir(parents=True, exist_ok=True)
    previous = a.out_dir / "report.json"
    reports = json.loads(previous.read_text()) if previous.exists() else {}
    cases = [('night-home',21,'orbit',None,False),('night-late',3,'orbit',None,False),
             ('night-cab',21,'driver',None,False),('traffic-fog',21,'orbit',None,True),
             ('sleet-cpu',12,'orbit','cpu',False),('sleet-gpu',12,'driver','gpu',False),
             ('sleet-hybrid',12,'orbit','hybrid',False)]
    for name,hour,follow,execution,traffic in cases:
        if a.cases and name not in a.cases:
            continue
        settings(a, quality_limits={'enabled':not traffic,'target_fps':60,'min_level':0,'max_level':3})
        path = scenario(a,name,hour,execution is not None,traffic)
        o = options(a,path,a.repo/'examples/player_lab/route')
        o.follow, o.camera_distance, o.camera_pitch = follow, 26, .3
        o.after_ready = False
        o.timeout_s = 45 if traffic else 28
        o.capture_headlights = 2
        if execution:
            o.weather_profile, o.weather_execution = 'edited', execution
        if traffic:
            o.weather, o.fog_quality = 'fog', 'volumetric64'
        report = run_checkpoint(o,name,0,False)
        report['formation_validation'] = validate_formation(report,2)
        assert report['shader_pipelines']['failed'] == 0
        assert report['shared_geometry']['keys'] > 0, report['shared_geometry']
        if execution:
            atmosphere = report['weather_state']['atmosphere']
            assert atmosphere['rain'] > .1 and atmosphere['snow'] > .1, atmosphere
            assert abs(atmosphere['rain']+atmosphere['snow']-.5)<.002
            assert abs(atmosphere['temperature_c']-.5)<.002
            assert abs(report['weather_particles']['snow_fraction']-.5)<.002, report['weather_particles']
            assert report['weather_particles']['gpu_particles'] + report['weather_particles']['cpu_particles'] <= 1024
        else:
            assert report['building_windows']['controlled_windows']>=2, report['building_windows']
            passenger = [lamp for lamp in report['train_lamps'] if lamp['passenger']]
            if not traffic:
                assert passenger, report['train_lamps']
            else:
                assert report['traffic'][0]['departed'], report['traffic']
                assert any(lamp['service']==1 for lamp in report['train_lamps']), report['train_lamps']
                assert report['fog']['volumetric'] and report['fog']['visibility_m']<=121
        reports[name] = report
        (a.out_dir/'report.json').write_text(json.dumps(reports,indent=2)+'\n')
        print('PASS '+name,flush=True)
    if 'night-home' in reports and 'night-late' in reports:
        assert reports['night-home']['building_windows']['bright'] > reports['night-late']['building_windows']['bright']
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
