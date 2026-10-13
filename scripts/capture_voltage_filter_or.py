#!/usr/bin/env python3
"""Capture the unchanged OR 1.6.1 IIRFilter class in an isolated .NET harness."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
from capture_cant_profiles_or import method

ROOT = Path(__file__).resolve().parents[1]
COMMIT = "d16e670da333d26d2edfc97d5631a19dadf49ce5"
SOURCE = "Source/Orts.Common/Filter.cs"


def capture(source_root, dotnet, output):
    if output.exists():
        raise ValueError("Refusing to overwrite an oracle")
    source = subprocess.check_output(["git", "-C", str(source_root), "show", COMMIT + ":" + SOURCE], text=True)
    algorithm = method(source, "public class IIRFilter")
    cases = []
    for name, voltage, dt in [("conductor-rail", 750, .05), ("ac-overhead", 25000, .05),
                              ("fast-sampling", 1500, .005), ("variable-sampling", 3000, .1)]:
        samples = [dict(input_v=voltage, dt_s=dt) for _ in range(60)]
        samples += [dict(input_v=0, dt_s=dt) for _ in range(60)]
        samples += [dict(input_v=voltage, dt_s=dt) for _ in range(60)]
        samples += [dict(input_v=voltage, dt_s=0), dict(input_v=voltage, dt_s=1.6)]
        cases.append(dict(name=name, samples=samples))
    harness = """
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
record Sample(float InputV,float DtS);
record Case(string Name,Sample[] Samples);
class Program {
 static void Main(string[] args) {
  var o=new JsonSerializerOptions {PropertyNamingPolicy=JsonNamingPolicy.SnakeCaseLower};
  var inputs=JsonSerializer.Deserialize<Case[]>(System.IO.File.ReadAllText(args[0]),o)!;
  var result=inputs.Select(c=>{
   var p=new IIRFilter(IIRFilter.FilterTypes.Butterworth,1,IIRFilter.HzToRad(0.7f),0.001f);
   return new {c.Name,samples=c.Samples.Select(s=>new {input_v=s.InputV,dt_s=s.DtS,output_v=p.Filter(s.InputV,s.DtS)}).ToArray()};
  }).ToArray();
  Console.WriteLine(JsonSerializer.Serialize(result,o));
 }
}
""" + algorithm
    with tempfile.TemporaryDirectory(prefix="voltage-filter-or-", dir=ROOT / "tmp") as temp:
        work = Path(temp)
        (work / "oracle.csproj").write_text('<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><Nullable>enable</Nullable></PropertyGroup></Project>')
        (work / "Program.cs").write_text(harness)
        (work / "inputs.json").write_text(json.dumps(cases))
        env = os.environ | {"DOTNET_CLI_HOME": str(ROOT / "tmp/dotnet-home"), "DOTNET_CLI_TELEMETRY_OPTOUT": "1"}
        build = subprocess.run([str(dotnet), "build", str(work), "-o", str(work / "bin")], capture_output=True, text=True, env=env)
        if build.returncode:
            raise ValueError(build.stdout + build.stderr)
        result = json.loads(subprocess.check_output([str(dotnet), str(work / "bin/oracle.dll"), str(work / "inputs.json")], text=True, env=env))
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(dict(reference="Open Rails 1.6.1 unchanged IIRFilter at the DefaultElectricPowerSupply 0.7 Hz cutoff", commit=COMMIT,
                                     source=SOURCE, source_sha256=hashlib.sha256(source.encode()).hexdigest(),
                                     algorithm_sha256=hashlib.sha256(algorithm.encode()).hexdigest(), tolerance_v=.01, cases=result), indent=2) + "\n")
    print(f"Captured {sum(len(c['samples']) for c in result)} native voltage-filter samples")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=ROOT.parent / "openrails")
    parser.add_argument("--dotnet", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    capture(args.source, args.dotnet, args.out)
