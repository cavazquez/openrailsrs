#!/usr/bin/env python3
"""Fresh real OR1.6.1 steam-cycle evidence, OracleWine only.

The programme distinguishes prescribed subsystem inputs and an original drive.
No model equations or post-start observed outputs are inputs to Rust. This
script has been written but not executed in the implementation phase.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tomllib

ROOT=next(p for p in Path(__file__).resolve().parents if (p/"scripts/oracle_wine.py").is_file())
sys.path.insert(0,str(ROOT/"scripts"))
from oracle_wine import OracleWine
from capture_chiltern_service_or import clone_prefix, windows_path
from capture_physical_systems_reference import steam_facade, lexical_windows, REFERENCES
from run_oracles import verify

PACKAGE=Path(__file__).resolve().parents[1]
HELPER=PACKAGE/"tools/openrails-reference/SteamCycleReference.cs"
PROTOCOL=PACKAGE/"oracles/steam-cycle-161-protocol.json"

def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def write(p,v):p.write_text(json.dumps(v,indent=2,ensure_ascii=False)+"\n")
def programme(case):
    original=json.loads((ROOT/"docs/fixtures/compatibility/twenty-20261015/physical-native05/steam-drive-config.json").read_text())
    commands=original["commands"]
    subsystem=case.endswith("subsystem")
    if subsystem:
        # Authorial test speeds/boundaries, registered before any DLL call.
        # They are not samples copied from the historic original trajectory.
        for command,speed,pipe in zip(commands,[0.,8.,16.,12.,4.],[14.5037738,4.2,4.2,4.2,14.5037738]):
            command.update(speed_mps=speed,pipe_absolute_psi=pipe)
    return {"schema":1,"case":case,"ticks":1600,"dt_s":0.05,
        "kind":"prescribed_speed_subsystem" if subsystem else "original_closed_loop_drive",
        "hot_start":not case.startswith("cold_"),"paused":case!="hot_drive_unpaused",
        "same_driver_commands_as_historic05":True,"commands":commands,
        "generator_context":"Simulator only; no steam viewer constructed; native GeneratorSteamEffects is captured explicitly",
        "native_05_limits":"unchanged; see protocol. NativeSteam161 port is evaluated separately from the own finite vessel"}

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--installation-root",type=Path,default=Path.home()/"wine64-OpenRails/drive_c/Program Files/Open Rails")
    ap.add_argument("--wine-source-prefix",type=Path,default=Path.home()/"wine64-OpenRails")
    ap.add_argument("--engine",type=Path,required=True);ap.add_argument("--path",type=Path,required=True)
    ap.add_argument("--case",choices=["hot_subsystem","cold_subsystem","hot_drive_paused","hot_drive_unpaused"],required=True)
    ap.add_argument("--out-dir",type=Path,required=True);ap.add_argument("--work-dir",type=Path,required=True)
    ap.add_argument("--existing-private-prefix",type=Path,
        help="Sequential reuse of a marked idle task prefix; OracleWine checks ownership and live processes")
    ap.add_argument("--prepare-only",action="store_true")
    a=ap.parse_args();out=a.out_dir.resolve();work=a.work_dir.resolve()
    prefix=a.existing_private_prefix.resolve() if a.existing_private_prefix else work/"wine-prefix"
    protected=[a.installation_root.resolve(),a.wine_source_prefix.resolve(),(ROOT.parent/"openrails").resolve(),
        Path.home()/"Documentos/Open Rails/Content",a.engine.resolve().parent]
    for directory in [out,work]:
        if any(directory==p or directory.is_relative_to(p) for p in protected):raise ValueError("Use fresh private evidence outside originals")
    if out.exists() or work.exists():raise ValueError("Both evidence/work directories must be fresh; preserve previous attempts")
    pin=tomllib.loads((ROOT/"oracles/openrails-reference.toml").read_text())
    verify(pin,ROOT.parent/"openrails",a.installation_root)
    out.mkdir(parents=True);work.mkdir(parents=True)
    config=programme(a.case);write(out/"input.json",config)
    shutil.copy2(PROTOCOL,out/"protocol.json");shutil.copy2(HELPER,out/HELPER.name);shutil.copy2(__file__,out/"capture-script.py")
    sources=[ROOT.parent/"openrails/Source/Orts.Simulation/Simulation/RollingStocks/MSTSSteamLocomotive.cs",
        ROOT.parent/"openrails/Source/Orts.Simulation/Common/SteamTable.cs",
        ROOT.parent/"openrails/Source/Orts.Common/SmoothedData.cs",
        ROOT.parent/"openrails/Source/Orts.Parsers.Msts/Interpolator.cs",
        ROOT.parent/"openrails/Source/Orts.Simulation/Simulation/RollingStocks/SubSystems/Brakes/MSTS/VacuumSinglePipe.cs"]
    registration={"schema":1,"reference_version":pin["version"],"reference_commit":pin["commit"],
        "phase":"registered_before_capture_and_rust_comparison","input_sha256":digest(out/"input.json"),
        "protocol_sha256":digest(out/"protocol.json"),"helper_sha256":digest(HELPER),
        "capture_script_sha256":digest(__file__),"wine_isolation_helper_sha256":digest(ROOT/"scripts/oracle_wine.py"),
        "originals":{str(p.resolve()):digest(p) for p in [a.engine,a.path]},
        "source_files":{str(p.relative_to(ROOT.parent/"openrails")):digest(p) for p in sources},
        "assemblies":{n:digest(a.installation_root/n) for n in REFERENCES}}
    write(out/"registration.json",registration)
    if a.prepare_only:print(json.dumps({"status":"prepared_not_executed","registration":str(out/"registration.json")}));return
    if a.existing_private_prefix:
        if not (prefix/".openrailsrs-private-prefix.json").is_file():raise ValueError("Existing prefix must have private-task marker")
    else:clone_prefix(a.wine_source_prefix,prefix)
    runtime=work/"runtime";shutil.copytree(a.installation_root,runtime)
    path,consist=steam_facade(a.path.resolve(),a.engine.resolve(),out/"content")
    try:
        with OracleWine(prefix,out/"wine-isolation") as wine:
            wine.run(["wine",r"C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe","/nologo","/out:SteamCycleReference.exe"]
                +["/r:"+n for n in REFERENCES]+[windows_path(out/HELPER.name)],cwd=runtime,log=out/"compile.log",timeout=90)
            wine.run(["wine","SteamCycleReference.exe",lexical_windows(path),windows_path(consist),
                windows_path(out/"native"),windows_path(out/"input.json")],cwd=runtime,log=out/"capture.log",timeout=240)
        verify(pin,ROOT.parent/"openrails",runtime)
        data=json.loads((out/"native/reference.json").read_text());rows=data["rows"]
        if len(rows)!=config["ticks"]+1:raise RuntimeError("Incomplete native tick coverage")
        if {r["phase"] for r in rows[1:]}!={c["phase"] for c in config["commands"]}:raise RuntimeError("Missing native phase")
        files=["registration.json","input.json","protocol.json","capture-script.py",HELPER.name,
            "compile.log","capture.log","native/reference.json","native/initial.json","native/tables.json",
            "wine-isolation/isolation.json","wine-isolation/release.json"]
        write(out/"manifest.json",dict(registration,status="captured_not_compared",method=data["method"],rows=len(rows),
            case=config["case"],files={p:digest(out/p) for p in files},
            helper_executable_sha256=digest(runtime/"SteamCycleReference.exe"),rust_acceptance="not_run"))
        print(json.dumps({"status":"captured_not_compared","manifest":str(out/"manifest.json"),"rows":len(rows)}))
    except BaseException as e:
        write(out/"failure.json",{"status":"failed_capture","type":type(e).__name__,"error":str(e),
            "registered_protocol_sha256":registration["protocol_sha256"],"rust_acceptance":"not_run"})
        raise

if __name__=="__main__":main()
