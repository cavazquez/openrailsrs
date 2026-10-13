#!/usr/bin/env python3
"""Run unchanged OR 1.6.1 pantograph, tilt and dynamic-force kernels in .NET."""
import argparse, hashlib, json, os, subprocess, tempfile
from pathlib import Path
from capture_cant_profiles_or import method
ROOT=Path(__file__).resolve().parents[1]
COMMIT="d16e670da333d26d2edfc97d5631a19dadf49ce5"
FILES={"panto":"Source/Orts.Simulation/Simulation/RollingStocks/SubSystems/PowerSupplies/Pantograph.cs","tilt":"Source/Orts.Simulation/Simulation/RollingStocks/TrainCar.cs","brake":"Source/Orts.Simulation/Simulation/RollingStocks/MSTSLocomotive.cs","interpolation":"Source/Orts.Parsers.Msts/Interpolator.cs"}
def capture(source,dotnet,out):
    if out.exists():raise ValueError("Refusing to overwrite an oracle")
    texts={k:subprocess.check_output(["git","-C",str(source),"show",COMMIT+":"+v],text=True) for k,v in FILES.items()}
    algorithms={"pantograph":method(texts["panto"].split("public class Pantograph :",1)[1],"public void Update(float"),"tilt":method(texts["tilt"],"internal void UpdateTilting("),"brake":method(texts["brake"],"public float GetAvailableDynamicBrakeForceN("),"linear":method(texts["interpolation"],"public float this[float x]"),"bilinear":method(texts["interpolation"].split("public class InterpolatorDiesel2D",1)[1],"public float Get(float")}
    harness=r'''
using System;using System.Linq;using System.Text.Json;
static class MathHelper {public static float Clamp(float v,float a,float b)=>Math.Clamp(v,a,b);}
class Traveller {public int CurveDirection;}
class Train {public bool IsTilting=true;}
enum PantographState {Down,Lowering,Raising,Up}
class Panto {public PantographState State=PantographState.Down;public float TimeS,DelayS;PANTO}
class Tilt {public Train Train=new();public float CurrentCurveRadiusM,SuperElevationAngleRad,TiltingZRot;float MinTiltSpeedMpS=50f/3.6f,MaxTiltAngleRad=8f*(float)Math.PI/180f;const float GravitationalAccelerationMpS2=9.80665f;TILT}
class Interpolator {public float[] X,Y;float[]? Y2=null;int Size,PrevIndex;public Interpolator(float[] x,float[] y){X=x;Y=y;Size=x.Length;}LINEAR}
class Curves {public float[] X={0,1};public Interpolator[] Y;int Size=2,PrevIndex;public Curves(float[] speed,float[] force){Y=new[]{new Interpolator(speed,new float[force.Length]),new Interpolator(speed,force)};}BILINEAR}
class Supply {public float MaximumDynamicBrakePowerW;}
class Brake {public Curves? DynamicBrakeForceCurves;public float AbsSpeedMpS,PowerReduction;public Supply LocomotivePowerSupply=new();BRAKE}
class Program {static void Main(){
 var panto=new[]{0f,.5f,2f}.Select(delay=>{var p=new Panto {DelayS=delay};return new {delay_s=delay,samples=Enumerable.Range(0,160).Select(i=>{bool up=(i>=10&&i<70)||(i>=90&&i<130);if(up&&p.State!=PantographState.Up&&p.State!=PantographState.Raising)p.State=PantographState.Raising;if(!up&&p.State!=PantographState.Down&&p.State!=PantographState.Lowering)p.State=PantographState.Lowering;p.Update(.05f);return new{up,dt_s=.05f,time_s=p.TimeS,fraction=p.State==PantographState.Up?1f:delay>0?p.TimeS/delay:0f};}).ToArray()};}).ToArray();
 var tilt=new[]{(400f,.03f,30f,1),(400f,.03f,30f,-1),(0f,0f,30f,1),(1000f,.08f,12f,1)}.Select(input=>{var t=new Tilt {CurrentCurveRadiusM=input.Item1,SuperElevationAngleRad=input.Item2};return new {radius_m=input.Item1,cant_rad=input.Item2,speed_mps=input.Item3,direction=input.Item4,samples=Enumerable.Range(0,160).Select(i=>{t.UpdateTilting(new Traveller {CurveDirection=input.Item4},.05f,input.Item3,1);return t.TiltingZRot;}).ToArray()};}).ToArray();
 float[] speeds={0,2,10,50,70,100};float[] forces={0,0,80000,80000,0,0};
 var brake=new[]{0f,.25f,.5f,1f}.SelectMany(handle=>new[]{0f,1f,5f,10f,20f,55f,80f}.SelectMany(speed=>new[]{0f,400000f}.Select(power=>{var b=new Brake {DynamicBrakeForceCurves=new Curves(speeds,forces),AbsSpeedMpS=speed,LocomotivePowerSupply=new Supply {MaximumDynamicBrakePowerW=power}};return new {handle,speed_mps=speed,maximum_power_w=power,force_n=b.GetAvailableDynamicBrakeForceN(handle)};}))).ToArray();
 Console.WriteLine(JsonSerializer.Serialize(new {pantographs=panto,tilt,brake,speed_curve=speeds,force_curve=forces}));
}}
'''
    for key in ["bilinear","linear","pantograph","tilt","brake"]:
        value=algorithms[key];harness=harness.replace({"pantograph":"PANTO","tilt":"TILT","brake":"BRAKE","linear":"LINEAR","bilinear":"BILINEAR"}[key],value)
    with tempfile.TemporaryDirectory(prefix="equipment-or-",dir=ROOT/"tmp") as d:
        work=Path(d);(work/"Program.cs").write_text(harness);(work/"oracle.csproj").write_text('<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><Nullable>enable</Nullable></PropertyGroup></Project>')
        env=os.environ|{"DOTNET_CLI_HOME":str(ROOT/"tmp/dotnet-home"),"DOTNET_CLI_TELEMETRY_OPTOUT":"1"}
        result=subprocess.run([str(dotnet),"build",str(work),"-o",str(work/"bin")],capture_output=True,text=True,env=env)
        if result.returncode:raise ValueError(result.stdout+result.stderr)
        cases=json.loads(subprocess.check_output([str(dotnet),str(work/"bin/oracle.dll")],text=True,env=env))
    out.parent.mkdir(parents=True,exist_ok=True);out.write_text(json.dumps({"reference":"Open Rails 1.6.1 unchanged component kernels; extensions use independent conservation/trajectory tests","commit":COMMIT,"sources":{k:{"path":FILES[k],"sha256":hashlib.sha256(v.encode()).hexdigest()} for k,v in texts.items()},"algorithms":{k:hashlib.sha256(v.encode()).hexdigest() for k,v in algorithms.items()},"tolerances":{"pantograph_fraction":.00001,"tilt_rad":.000002,"brake_force_n":.02},**cases},indent=2)+"\n")
    print("Captured 480 pantograph steps, 640 tilt steps and 56 dynamic-brake cases")
if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__);p.add_argument("--source",type=Path,default=ROOT.parent/"openrails");p.add_argument("--dotnet",type=Path,required=True);p.add_argument("--out",type=Path,required=True);a=p.parse_args();capture(a.source,a.dotnet.resolve(),a.out)
