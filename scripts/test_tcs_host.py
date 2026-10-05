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
    print('PASS C# host: compiler subset rejection, exceptions, sequences, SI limits, clean protocol, ACK/menu')

if __name__=='__main__':main()
