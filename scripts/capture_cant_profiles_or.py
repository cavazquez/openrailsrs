#!/usr/bin/env python3
"""Capture the unchanged OR 1.6.1 MarkSections algorithm in an isolated harness."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

COMMIT = 'd16e670da333d26d2edfc97d5631a19dadf49ce5'
SOURCE = 'Source/Orts.Simulation/Simulation/SuperElevation.cs'
STANDARD = 'Source/Orts.Formats.Msts/RouteFile.cs'
CONVERSIONS = 'Source/Orts.Common/Conversions.cs'
ROOT = Path(__file__).resolve().parents[1]


def method(source, start):
    a = source.index(start)
    brace = source.index('{', a)
    depth = 1
    end = brace + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[a:end]


def capture(source_root, dotnet, output):
    if output.exists():
        raise ValueError('Refusing to overwrite an oracle')
    source = subprocess.check_output(['git', '-C', str(source_root), 'show', COMMIT + ':' + SOURCE], text=True)
    route = subprocess.check_output(['git', '-C', str(source_root), 'show', COMMIT + ':' + STANDARD], text=True)
    conversions = subprocess.check_output(['git', '-C', str(source_root), 'show', COMMIT + ':' + CONVERSIONS], text=True)
    algorithm = method(source, 'void MarkSections(')
    a = route.index('public class SuperElevationStandard')
    b = route.index('// Initialize new instance from superelevation interpolator', a)
    standard = route[a:b] + '\n}'
    inputs = []
    for name, metric, high, lengths, radii, passenger, freight, direction in [
        ('metric-long', True, False, [40, 300, 40], [0, 650, 0], 30, 24, 1),
        ('imperial-long', False, False, [40, 300, 40], [0, 650, 0], 30, 24, 1),
        ('imperial-reverse', False, False, [40, 300, 40], [0, 650, 0], 30, 24, -1),
        ('short-curve', True, False, [2], [100], 20, 20, 1),
        ('slow-yard', False, False, [200], [150], 3, 3, 1),
        ('short-straight', False, False, [200, 5, 200], [500, 0, 500], 25, 25, 1),
        ('cusp', True, False, [20, 15, 20], [0, 400, 0], 30, 30, 1),
        ('varying-radius', True, False, [60, 120, 70, 90, 60], [0, 450, 1200, 450, 0], 32, 22, 1),
        ('high-speed', True, True, [120, 900, 120], [0, 2500, 0], 60, 40, 1),
        ('imperial-high-speed', False, True, [100, 500, 100], [0, 1500, 0], 55, 35, -1),
    ]:
        inputs.append(dict(name=name, metric=metric, high_speed=high, gauge_m=1.435, direction=direction,
                           sections=[dict(length_m=l, radius_m=r, passenger_speed_mps=passenger, freight_speed_mps=freight)
                                     for l, r in zip(lengths, radii)]))
    harness = r'''
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
class Interpolator {
 public float[] X,Y;
 public Interpolator(float[] x,float[] y) {X=x;Y=y;}
 public void ScaleY(float v) {for(int i=0;i<Y.Length;i++) Y[i]*=v;}
 public float this[float x] => 0;
}
static class MathHelper {public static float Clamp(float v,float min,float max)=>Math.Clamp(v,min,max);}
static class Me {public static float FromIn(float v)=>v*0.0254f;}
static class MpS {public static float FromKpH(float v)=>v/3.6f;public static float FromMpH(float v)=>v*0.44704f;}
class SectionCurve {public float Radius;}
class TrackSection {public SectionCurve? SectionCurve;}
class TrackSections:Dictionary<uint,TrackSection> {public TrackSection Get(uint i)=>this[i];}
class TSectionDat {public TrackSections TrackSections=new();}
class Route {public List<SuperElevationStandard> SuperElevation=new();public Interpolator? SuperElevationHgtpRadiusM=null;}
class TRK {public Route Tr_RouteFile=new();}
class Simulator {public TRK TRK=new();public TSectionDat TSectionDat=new();public float RouteTrackGaugeM=1.435f;}
class TrVectorSection {public uint SectionIndex;public float NomElevM=-1,PassSpeedMpS,FreightSpeedMpS;public Interpolator? PhysElevTable,VisElevTable;}
record Section(float LengthM,float RadiusM,float PassengerSpeedMps,float FreightSpeedMps);
record Case(string Name,bool Metric,bool HighSpeed,float GaugeM,int Direction,Section[] Sections);
class Reference {
 List<List<TrVectorSection>> Curves=new();
 void MapWFiles2Sections(List<TrVectorSection> sections) {}
 public object Run(Case input) {
  var simulator=new Simulator {RouteTrackGaugeM=input.GaugeM};
  simulator.TRK.Tr_RouteFile.SuperElevation.Add(new SuperElevationStandard(input.Metric,input.HighSpeed));
  var sections=new List<TrVectorSection>();
  for(uint i=0;i<input.Sections.Length;i++) {
   var s=input.Sections[i];simulator.TSectionDat.TrackSections[i]=new TrackSection {SectionCurve=s.RadiusM>0 ? new SectionCurve {Radius=s.RadiusM}:null};
   sections.Add(new TrVectorSection {SectionIndex=i,PassSpeedMpS=s.PassengerSpeedMps,FreightSpeedMpS=s.FreightSpeedMps});
  }
  var lengths=input.Sections.Select(s=>s.LengthM).ToList();
  MarkSections(simulator,sections,lengths.Sum(),lengths,input.Direction);
  return new {input.Name,input.Metric,input.HighSpeed,input.GaugeM,input.Direction,input.Sections,
   profiles=sections.Select(s=>new {positions=s.PhysElevTable?.X ?? [],elevations_m=s.PhysElevTable?.Y ?? [],angles_rad=s.VisElevTable?.Y ?? []}).ToArray()};
 }
 ALGORITHM
}
class Program {
 static void Main(string[] args) {
  var options=new JsonSerializerOptions {PropertyNamingPolicy=JsonNamingPolicy.SnakeCaseLower};
  var inputs=JsonSerializer.Deserialize<Case[]>(System.IO.File.ReadAllText(args[0]),options)!;
  Console.WriteLine(JsonSerializer.Serialize(inputs.Select(c=>new Reference().Run(c)).ToArray(),options));
 }
}
'''.replace('ALGORITHM', algorithm) + standard
    harness = harness.replace('public static float FromIn(float v)=>v*0.0254f;', method(conversions, 'public static float FromIn('))
    harness = harness.replace('public static float FromKpH(float v)=>v/3.6f;', method(conversions, 'public static float FromKpH('))
    harness = harness.replace('public static float FromMpH(float v)=>v*0.44704f;', method(conversions, 'public static float FromMpH('))
    (ROOT / 'tmp').mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='cant-or-', dir=ROOT / 'tmp') as temp:
        work = Path(temp)
        (work / 'oracle.csproj').write_text('<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><Nullable>enable</Nullable></PropertyGroup></Project>')
        (work / 'Program.cs').write_text(harness)
        (work / 'inputs.json').write_text(json.dumps(inputs))
        env = os.environ | {'DOTNET_CLI_HOME': str(ROOT / 'tmp/dotnet-home'), 'DOTNET_CLI_TELEMETRY_OPTOUT': '1'}
        built = subprocess.run([str(dotnet), 'build', str(work), '-o', str(work / 'bin')], capture_output=True, text=True, env=env)
        if built.returncode:
            raise ValueError(built.stdout + built.stderr)
        captured = json.loads(subprocess.check_output([str(dotnet), str(work / 'bin/oracle.dll'), str(work / 'inputs.json')], text=True, env=env))
    oracle = dict(reference='Open Rails 1.6.1, isolated unchanged MarkSections with synthetic section/speed inputs',
                  commit=COMMIT, source=SOURCE, source_sha256=hashlib.sha256(source.encode()).hexdigest(),
                  algorithm_sha256=hashlib.sha256(algorithm.encode()).hexdigest(),
                  standard_source=STANDARD, standard_source_sha256=hashlib.sha256(route.encode()).hexdigest(),
                  conversions_source=CONVERSIONS, conversions_source_sha256=hashlib.sha256(conversions.encode()).hexdigest(),
                  tolerance_m=0.00001, tolerance_rad=0.000001, cases=captured)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(oracle, indent=2) + '\n')
    print(f'Captured {len(captured)} unchanged OR cant groups: {output}')


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source', type=Path, default=ROOT.parent / 'openrails')
    p.add_argument('--dotnet', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    a = p.parse_args()
    capture(a.source, a.dotnet, a.out)
