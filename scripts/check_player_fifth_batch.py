#!/usr/bin/env python3
"""Owned fixtures and author-route captures for the fifth forty-issue batch.

Uses a private hardware renderer, isolated player files and unchanged author
assets. Output includes the binary hash and all action results.
"""
import argparse,hashlib,json,re,tomllib
from pathlib import Path
from check_player_expansion import options,settings
from check_viewer_streaming import run_checkpoint

def scenario(args,source,name):
    text=source.read_text();parsed=tomllib.loads(text);base=source.parent
    text=re.sub(r'(?m)^path\s*=\s*'+re.escape(json.dumps(parsed['route']['path'])), 'path = '+json.dumps(str(base/parsed['route']['path'])), text,count=1)
    con=parsed['train']['consist']
    text=text.replace('consist = '+json.dumps(con),'consist = '+json.dumps(str((base/con).resolve())),1)
    output=args.out_dir/'scenarios'/f'{name}.toml';output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(text);return output

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--viewer',type=Path,required=True);p.add_argument('--out-dir',type=Path,required=True)
    p.add_argument('--cases',nargs='+');a=p.parse_args();a.repo=Path(__file__).resolve().parents[1]
    a.viewer=a.viewer.resolve();a.out_dir=a.out_dir.resolve();a.out_dir.mkdir(parents=True,exist_ok=True)
    names=['connections','connections-cut','connections-curve','jacobs','jacobs-curve','mirrors','mirrors-rain','boarding','boarding-restore','boarding-rebase','boarding-close','alighting','wet','dry','snow','snow-curve','camera-collision','refill-restore']
    report_path=a.out_dir/'report.json';report=json.loads(report_path.read_text()) if report_path.exists() else {}
    for name in names:
        if a.cases and name not in a.cases:continue
        src=a.repo/'examples/player_lab'/('scenario_jacobs_curve.toml' if name=='jacobs-curve' else 'scenario_jacobs.toml' if name=='jacobs' else 'scenario_connections_curve.toml' if name=='connections-curve' else 'scenario_connections.toml' if name.startswith('connections') else 'scenario_jacobs_curve.toml' if name=='snow-curve' else 'scenario_refill.toml' if name=='refill-restore' else 'scenario.toml')
        path=scenario(a,src,name);root=Path(tomllib.loads(path.read_text())['route']['path'])
        actions=[];follow='orbit';weather='clear'
        if name.startswith('mirrors'):follow='driver'
        if name in ['mirrors-rain','wet']:weather='rain'
        if name=='connections-cut':
            actions=[{'at_s':1,'operation':{'action':'car','vehicle':1,'operation':'handbrake'}},
                     {'at_s':2,'operation':{'action':'uncouple','after':0}},
                     {'at_s':3,'operation':{'action':'restore'}},
                     {'at_s':4,'operation':{'action':'pause','value':True}}]
        if name.startswith('boarding') or name=='alighting':
            text=path.read_text().replace('destination = "yard_b"','destination = "yard_b"\n[[route.stops]]\nnode="yard_b"\noffset_m=-9920.0\nname="Andén de prueba"\narrive_s=0.0\ndepart_s=45.0\ndwell_s=45.0\npassengers_on=30\npassengers_off='+('12' if name=='alighting' else '0')+'\n')
            text+='\n[[train.platform_sides]]\nstop=0\nside="left"\n'
            path.write_text(text)
            actions=[{'at_s':1,'operation':{'action':'doors','vehicle':1,'side':'left','open':True}}]
            if name=='alighting':actions.insert(0,{'at_s':0,'operation':{'action':'fixture_passengers','count':12}})
            if name in ['boarding-restore','boarding-rebase']:
                actions += [{'at_s':9,'operation':{'action':'pause','value':True}}, {'at_s':10,'operation':{'action':'restore'}}, {'at_s':12,'operation':{'action':'pause','value':True}}]
            if name=='boarding-rebase':
                actions += [{'at_s':14,'operation':{'action':'fly_camera','position_m':[500.,30.,0.],'look_at_m':[0.,0.,0.]}}, {'at_s':17,'operation':{'action':'orbit','yaw':3.5,'pitch':0.25,'distance_m':30.}}]
            if name=='boarding-close':
                actions += [{'at_s':3.5,'operation':{'action':'doors','vehicle':1,'side':'left','open':False}},
                            {'at_s':3.6,'operation':{'action':'pause','value':True}},
                            {'at_s':4.,'operation':{'action':'restore'}},
                            {'at_s':5.,'operation':{'action':'pause','value':False}},
                            {'at_s':10.,'operation':{'action':'pause','value':True}}]
        if name.startswith('snow'):weather='snow'
        if name=='camera-collision':
            # Reach the near side by going around its end first. A direct
            # "teleport" from the initial orbit is also collision-constrained.
            actions=[{'at_s':1,'operation':{'action':'fly_camera','position_m':[-65.,2.5,20.],'look_at_m':[-65.,2.5,30.]}},
                     {'at_s':2,'operation':{'action':'fly_camera','position_m':[-72.,2.5,20.],'look_at_m':[-72.,2.5,30.]}},
                     {'at_s':3,'operation':{'action':'fly_camera','position_m':[-72.,2.5,29.],'look_at_m':[-72.,2.5,35.]}},
                     {'at_s':4,'operation':{'action':'pause','value':True}}]

        if name=='refill-restore':
            actions=[{'at_s':2,'operation':{'action':'refill','station':'0:0:30','vehicle':0}},
                     {'at_s':3,'operation':{'action':'pause','value':True}},
                     {'at_s':4,'operation':{'action':'restore'}},
                     {'at_s':8,'operation':{'action':'pause','value':True}}]
        settings(a,improved_transparency=True,quality_limits={'enabled':False,'target_fps':60,'min_level':0,'max_level':3})
        o=options(a,path,root);o.capture_or_focus=True;o.follow=follow;o.weather=weather;o.after_ready=False;o.timeout_s=32;o.ready_frames=10;o.camera_distance=38;o.camera_yaw=0.35
        if actions:o.capture_actions=json.dumps(actions)
        # The authorised left platform lies on -Z in this fixture.
        # Show that side rather than counting figures hidden behind the car.
        if name.startswith('boarding') or name=='alighting':
            o.camera_yaw=3.5;o.camera_distance=30
        if name=='refill-restore':
            o.camera_yaw=.35;o.camera_distance=14;o.camera_pitch=.18
        moving=name.endswith('-curve')
        if moving:o.autodrive=0.5;o.speed_mul=4;o.timeout_s=70
        if name=='snow-curve':o.capture_delay_s=15
        r=run_checkpoint(o,name,35 if moving else 0,pause=moving)
        r['binary_sha256']=hashlib.sha256(a.viewer.read_bytes()).hexdigest()
        r['runtime_asset_sha256']={str(p.relative_to(a.repo)):hashlib.sha256(p.read_bytes()).hexdigest() for p in (a.repo/'crates/openrailsrs-bevy-scenery/assets/shaders').glob('*.wgsl')}
        r['fixture_sha256']={str(p.relative_to(a.repo)):hashlib.sha256(p.read_bytes()).hexdigest() for p in (a.repo/'examples/player_lab').rglob('*') if p.is_file() and p.suffix.lower() in ['.s','.eng','.wag','.con','.ace','.toml','.w']}

        if actions:
            assert len(r['equipment_actions'])==len(actions),r['equipment_actions']
            assert all(x.get('result')=={'Ok':None} for x in r['equipment_actions']),r['equipment_actions']
        if name.startswith('connections'):
            c=r['flexible_connections'];assert c['maximum_endpoint_error_m']<.001,c
            assert c['eligible_parts']>0 and c['deformed']>0,c
            if name=='connections-cut':assert c['disconnected']>0,c
            else:assert c['connected_parts']>0 and c['span_rejected']==0,c
        if name.startswith('jacobs'):assert len(r['jacobs_joints'])==1,r['jacobs_joints']
        if name.startswith('mirrors'):
            m=r['mirrors'];assert m['views']==2 and m['frames']>2 and m['resolution']==[256,160],m
        if name.startswith('boarding') or name=='alighting':
            if name!='boarding-close':assert r['platform_people']['visible']>0,r['platform_people']
            assert r['platform_people']['rejected_paths']==0,r['platform_people']
            if name=='alighting':
                people=r['platform_people']
                # A mixed stop must not send every alighting figure outside
                # before its proportional transfer has happened. Allow one
                # active slot and the integer grouping boundary.
                expected=30-people['represented_boarding']+people['represented_alighting']
                assert 0<people['represented_alighting']<12,people
                assert abs(people['visible']-expected)<=2,people
            if name in ['boarding-restore','boarding-rebase']:
                states=r['equipment_actions'][1:4]
                assert abs(states[0]['boarding_progress']-states[1]['boarding_progress'])<1e-9,states
                assert abs(states[0]['time_s']-states[2]['time_s'])<1e-9,states
            if name=='alighting':assert r['platform_people']['represented_alighting']>0,r['platform_people']
            if name=='boarding-rebase':assert any(abs(x['origin_m'][0])>=256. for x in r['equipment_actions']),r['equipment_actions']
            if name=='boarding-close':
                close,paused,restored=r['equipment_actions'][1:4]
                equipment_index,car=next((index,car) for index,car in
                    enumerate(r['vehicle_equipment']['cars'])
                    if car['definition']['vehicle']==1)
                assert close['door_fractions'][equipment_index][0]>=.98,close
                assert paused['door_fractions']==restored['door_fractions'],r['equipment_actions']
                assert abs(paused['time_s']-restored['time_s'])<1e-9,r['equipment_actions']
                assert car['door_fraction'][0]<.001 and not car['door_target'][0],car
                assert car['door_close_at_progress'][0] is None,car
                assert r['equipment_actions'][-1]['boarding_progress']<.2,r['equipment_actions']
        if name=='camera-collision':
            assert r['camera']['position_world'][2] < -18.2,r['camera']
            assert r['camera_collision']['corrections']>0,r['camera_collision']
            assert r['obstacles']['solid_count']>0,r['obstacles']

        if name=='refill-restore':
            assert r['refilling'] and 0<r['refilling']['opening']<1.,r['refilling']
            before,after=r['equipment_actions'][1:3]
            assert before['refilling']==after['refilling'],r['equipment_actions']
            assert abs(before['time_s']-after['time_s'])<1e-9,r['equipment_actions']
        if name.startswith('snow'):
            assert r['snow_cover']>0.05,r['snow_cover']
            if moving:assert r['snow_tracks']['gpu_segments']>0,r['snow_tracks']
        report[name]=r;report_path.write_text(json.dumps(report,indent=2)+'\n');print('PASS '+name,flush=True)
if __name__=='__main__':main()
