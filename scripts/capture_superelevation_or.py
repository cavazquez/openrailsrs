#!/usr/bin/env python3
"""Compile OR 1.6.1's isolated TrainCar comfort formula; never replace a baseline."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

COMMIT = "d16e670da333d26d2edfc97d5631a19dadf49ce5"
SOURCE = "Source/Orts.Simulation/Simulation/RollingStocks/TrainCar.cs"

def capture(source_root, dotnet, output):
    source = subprocess.check_output(["git", "-C", str(source_root), "show", f"{COMMIT}:{SOURCE}"])
    text = source.decode("utf-8-sig")
    expression = re.search(r"float MaxSafeCurveSpeedMps = (.*?);", text).group(1)
    gravity = re.search(r"const float GravitationalAccelerationMpS2 = ([0-9.]+f);", text).group(1)
    cases = [
        dict(name=name, radius_m=r, cant_m=c, gauge_m=g, max_unbalanced_m=d)
        for name, r, c, g, d in [
            ("passenger_500m_150mm",500,.150,1.435,.0762),
            ("freight_500m_150mm",500,.150,1.435,.0762),
            ("engine_500m_150mm",500,.150,1.435,.1524),
            ("turnout_200m_zero_cant",200,0,1.435,.0762),
            ("high_speed_4000m",4000,.180,1.435,.0762),
            ("tilting_4000m",4000,.180,1.435,.300),
            ("argentina_broad_gauge",500,.150,1.676,.0762),
            ("metre_gauge",300,.100,1.0,.0762),
            ("explicit_imperial_gauge",500,.150,1.4351,.0762),
            ("unknown_wagon_default",200,0,1.435,.000254),
        ]
    ]
    # A recapture must use a fresh directory, including when the requested
    # path is a symlink; never replace a prior capture or a reference tree.
    output.mkdir(parents=True)
    (output / "Formula.csproj").write_text('<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings></PropertyGroup></Project>\n')
    program = '''using System.Text.Json;
var cases = JsonSerializer.Deserialize<List<Dictionary<string,JsonElement>>>(Console.In.ReadToEnd())!;
var results = new List<object>();
foreach (var c in cases) {
 float SuperElevationM=c["cant_m"].GetSingle();
 float MaxUnbalancedSuperElevationM=c["max_unbalanced_m"].GetSingle();
 float CurrentCurveRadiusM=c["radius_m"].GetSingle();
 float TrackGaugeM=c["gauge_m"].GetSingle();
 const float GravitationalAccelerationMpS2=GRAVITY;
 float MaxSafeCurveSpeedMps=EXPRESSION;
 results.Add(new { name=c["name"].GetString(),radius_m=c["radius_m"].GetDouble(),cant_m=c["cant_m"].GetDouble(),gauge_m=c["gauge_m"].GetDouble(),max_unbalanced_m=c["max_unbalanced_m"].GetDouble(),comfortable_speed_mps=MaxSafeCurveSpeedMps });
}
Console.WriteLine(JsonSerializer.Serialize(results));
'''.replace("GRAVITY", gravity).replace("EXPRESSION", expression)
    (output / "Program.cs").write_text(program)
    env = os.environ | {"DOTNET_CLI_TELEMETRY_OPTOUT":"1", "DOTNET_CLI_HOME":str(output.resolve()/"dotnet-home")}
    subprocess.run([dotnet, "build", str(output/"Formula.csproj"), "--ignore-failed-sources", "-o", str(output/"bin")],env=env,check=True,capture_output=True,text=True)
    result = subprocess.run([dotnet,str(output/"bin/Formula.dll")],input=json.dumps(cases),env=env,check=True,capture_output=True,text=True)
    report = dict(reference_version="1.6.1", reference_commit=COMMIT, source=SOURCE,
        source_sha256=hashlib.sha256(source).hexdigest(), expression=expression, gravity=gravity,
        scope="Isolated original C# comfort-speed expression with float arithmetic; not a whole-simulator or generated-track capture",
        tolerance_mps=0.00002,cases=json.loads(result.stdout))
    (output/"oracle.json").write_text(json.dumps(report,ensure_ascii=False,indent=2)+"\n")
    print(f"Captured {len(cases)} original formula cases: {output/'oracle.json'}")

if __name__ == "__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source-root",required=True,type=Path)
    p.add_argument("--dotnet",default="dotnet")
    p.add_argument("--out-dir",required=True,type=Path)
    a=p.parse_args()
    capture(a.source_root,a.dotnet,a.out_dir)
