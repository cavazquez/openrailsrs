#!/usr/bin/env python3
"""Capture original OR 1.6.1 injector tables and interpolation with .NET."""
import argparse,hashlib,json,os,re,subprocess,tempfile
from pathlib import Path
from capture_cant_profiles_or import method
ROOT=Path(__file__).resolve().parents[1]
PIN='d16e670da333d26d2edfc97d5631a19dadf49ce5'
TABLE='Source/Orts.Simulation/Common/SteamTable.cs'
INTERPOLATION='Source/Orts.Parsers.Msts/Interpolator.cs'
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--dotnet',type=Path,required=True);p.add_argument('--source',type=Path,default=ROOT.parent/'openrails');p.add_argument('--out',type=Path,required=True);a=p.parse_args()
 if a.out.exists():raise ValueError('Refusing to overwrite oracle')
 read=lambda path:subprocess.check_output(['git','-C',str(a.source),'show',PIN+':'+path],text=True)
 source=read(TABLE);interpolation=read(INTERPOLATION);linear=method(interpolation,'public float this[float x]')
 names=['PressureTableGaugePSI']+[f'Injector{i:02}FlowTableUKGpM' for i in [9,10,11,13,14,15]]
 declarations=[];data={}
 for name in names:
  block=re.search(r'static float\[\] '+name+r' = new float\[\]\s*\{[^}]+\};',source).group()
  declarations.append(block);data[name]=[float(s.replace('f','')) for s in re.findall(r'[\d.]+f?',block.split('{')[1].split('}')[0])]
 methods='\n'.join(method(source,f'public static Interpolator Injector{i:02}FlowrateInterpolatorPSItoUKGpM()') for i in [9,10,11,13,14,15])
 program='''using System;using System.Linq;using System.Text.Json;
class Interpolator {public float[] X,Y;float[]? Y2=null;int Size,PrevIndex;public Interpolator(float[] x,float[] y){X=x;Y=y;Size=x.Length;}LINEAR}
class Tables {TABLES METHODS}
class Program{static void Main(){var tables=new[]{Tables.Injector09FlowrateInterpolatorPSItoUKGpM(),Tables.Injector10FlowrateInterpolatorPSItoUKGpM(),Tables.Injector11FlowrateInterpolatorPSItoUKGpM(),Tables.Injector13FlowrateInterpolatorPSItoUKGpM(),Tables.Injector14FlowrateInterpolatorPSItoUKGpM(),Tables.Injector15FlowrateInterpolatorPSItoUKGpM()};int[] size={9,10,11,13,14,15};var cases=tables.SelectMany((t,i)=>Enumerable.Range(0,67).Select(n=>new{size_mm=size[i],pressure_psi=n*5f,flow_kg_s=(t[n*5f]/60f*10.021f)*.45359237f})).ToArray();Console.WriteLine(JsonSerializer.Serialize(cases));}}
'''.replace('LINEAR',linear).replace('TABLES','\n'.join(declarations)).replace('METHODS',methods)
 with tempfile.TemporaryDirectory(dir=ROOT/'tmp',prefix='injector-or-') as d:
  d=Path(d);(d/'Program.cs').write_text(program);(d/'oracle.csproj').write_text('<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><Nullable>enable</Nullable></PropertyGroup></Project>')
  env=os.environ|{'DOTNET_CLI_HOME':str(ROOT/'tmp/dotnet-home'),'DOTNET_CLI_TELEMETRY_OPTOUT':'1'}
  r=subprocess.run([str(a.dotnet.resolve()),'build',str(d),'-o',str(d/'bin')],env=env,capture_output=True,text=True)
  if r.returncode:raise ValueError(r.stdout+r.stderr)
  cases=json.loads(subprocess.check_output([str(a.dotnet.resolve()),str(d/'bin/oracle.dll')],env=env,text=True))
 a.out.write_text(json.dumps({'commit':PIN,'sources':{TABLE:hashlib.sha256(source.encode()).hexdigest(),INTERPOLATION:hashlib.sha256(interpolation.encode()).hexdigest()},'algorithms':{'tables':hashlib.sha256(('\n'.join(declarations)+methods).encode()).hexdigest(),'interpolation':hashlib.sha256(linear.encode()).hexdigest()},'flow_tolerance_kg_s':0.000002,'tables':data,'cases':cases},indent=2)+'\n')
 print('Captured',len(cases),'original injector flow samples')
if __name__=='__main__':main()
