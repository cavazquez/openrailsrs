#!/usr/bin/env python3
"""Actual C# compilation/JSONL boundaries; optional SDK acceptance, no native route needed."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]

def run(dotnet,host,script,requests):
    with tempfile.TemporaryDirectory(prefix='tcs-script-',dir=ROOT/'tmp') as directory:
        path=Path(directory)/'Script.cs';path.write_text(script)
        result=subprocess.run([dotnet,str(host),'--script',str(path),'--type','MinimalTcs'],
            input=''.join(json.dumps(r)+'\n' for r in requests),text=True,capture_output=True,timeout=15)
        return [json.loads(line) for line in result.stdout.splitlines()]

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--dotnet',default='dotnet');p.add_argument('--host',required=True,type=Path);a=p.parse_args()
    script=(ROOT/'docs/fixtures/tcs/MinimalTcs.cs').read_text()
    context=dict(time_s=0,dt_s=0.05,speed_mps=0,speed_limit_mps=15,next_signal_distance_m=8,next_signal_stop=False,next_stop_distance_m=0)
    first=dict(version=1,seq=1,kind='initialize',context=context,events=[])
    tick=first|dict(seq=2,kind='tick',events=[dict(kind='acknowledge',message='C# TCS listo: confirmar')])
    menu=first|dict(seq=3,kind='tick',events=[dict(kind='menu',action='restrict')])
    replies=run(a.dotnet,a.host,script,[first,tick,menu])
    assert len(replies)==3
    assert replies[0]['status']['emergency_brake']
    assert not replies[1]['status']['emergency_brake']
    assert replies[1]['status']['messages'][0]['acknowledged']
    assert replies[2]['status']['allowed_mps']==5
    # Unsupported OR members must fail loudly instead of being silently stubbed.
    bad=script.replace('Activated = true;', 'UnsupportedNativeMember();')
    assert run(a.dotnet,a.host,bad,[first])[0]['error']
    throwing=script.replace('Activated = true;', 'throw new System.Exception("fixture failure");')
    assert 'fixture failure' in run(a.dotnet,a.host,throwing,[first])[0]['error']
    assert run(a.dotnet,a.host,script,[first|dict(seq=99)])[0]['error']
    assert run(a.dotnet,a.host,script,[first|dict(context=context|dict(speed_mps=-1))])[0]['error']
    logging=script.replace('Activated = true;', 'System.Console.WriteLine("should not reach protocol"); Activated = true;')
    assert run(a.dotnet,a.host,logging,[first])[0]['status']
    lookahead='''using ORTS.Scripting.Api;
public class MinimalTcs : TrainControlSystem {
 public override void Initialize() { Activated=true; }
 public override void Update() {
  Aspect[] expected = {Aspect.Stop,Aspect.StopAndProceed,Aspect.Restricted,Aspect.Approach_1,Aspect.Approach_2,Aspect.Approach_3,Aspect.Clear_1,Aspect.Clear_2};
  for(int i=0;i<8;i++) if(NextSignalAspect(i)!=expected[i] || NextSignalDistanceM(i)!=100*(i+1)) throw new System.Exception("native aspect/distance mismatch");
  if(NextSignalAspect(8)!=Aspect.None || NextSignalDistanceM(8)!=float.MaxValue) throw new System.Exception("absent signal must be None");
  if(NextDistanceSignalAspect()!=Aspect.Approach_2 || NextDistanceSignalDistanceM()!=50) throw new System.Exception("distance head mismatch");
  if(TrainMaxSpeedMpS()!=30 || CurrentPostSpeedLimitMpS()!=12 || NextPostDistanceM(1)!=400 || NextPostSpeedLimitMpS(1)!=20 || NextPostSpeedLimitMpS(2)!=-1) throw new System.Exception("speed post/train max mismatch");
  if((int)TCSEvent.AlerterPressed!=3 || (int)TCSEvent.GenericTCSButtonPressed!=14) throw new System.Exception("OR enum mismatch");
  HostMessage("Native lookahead OK",false);
 }
 public override void HandleEvent(TCSEvent e,string m) {}
}'''
    extended=context|dict(train_max_speed_mps=30,current_post_speed_limit_mps=12,
        signals=[dict(distance_m=100*(i+1),aspect=i) for i in range(8)],distance_signal=dict(distance_m=50,aspect=4),
        speed_posts=[dict(distance_m=200,speed_limit_mps=10),dict(distance_m=400,speed_limit_mps=20)])
    reply=run(a.dotnet,a.host,lookahead,[first|dict(context=extended)])[0]
    assert not reply['error'],reply
    assert reply['status']['messages'][0]['text']=='Native lookahead OK'
    for invalid in [extended|dict(signals=[dict(distance_m=10,aspect=8)]),
                    extended|dict(signals=[dict(distance_m=20,aspect=7),dict(distance_m=10,aspect=7)]),
                    extended|dict(speed_posts=[dict(distance_m=-1,speed_limit_mps=10)]),
                    extended|dict(train_max_speed_mps=201)]:
        assert run(a.dotnet,a.host,script,[first|dict(context=invalid)])[0]['error']
    assert run(a.dotnet,a.host,lookahead.replace('NextSignalAspect(8)','NextSignalAspect(32)'),[first|dict(context=extended)])[0]['error']
    print('PASS C# host: compiler rejection, exceptions, sequences, SI limits, clean protocol, ACK/menu, eight native aspects, indexed lookahead, speed posts, train max, OR enum values')

if __name__=='__main__':main()
