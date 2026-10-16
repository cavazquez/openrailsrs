#!/usr/bin/env python3
"""NEW OR1.6.1 property/emergency evidence; never overwrites a reference.

Implementation phase: this file is prepared, not executed. At the joint gate,
use a fresh --out-dir and private --work-dir. All Wine/csc/helper execution is
inside OracleWine; no inherited desktop, audio device or shared prefix.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import shutil
import tomllib

from capture_chiltern_service_or import clone_prefix, windows_path
from oracle_wine import OracleWine
from prepare_chiltern_service import msts_blocks, msts_text
from run_oracles import verify

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "tools/openrails-reference/PhysicalSystemsReference.cs"
PROTOCOL = ROOT / "oracles/physical-systems-seven-protocol.json"
REFERENCES = ["RunActivity.exe", "Orts.Simulation.dll", "Orts.Settings.dll", "Orts.Common.dll",
              "Orts.Formats.Msts.dll", "Orts.Parsers.Msts.dll", "MonoGame.Framework.dll", "Newtonsoft.Json.dll", "GNU.Gettext.dll"]
CONTENT = Path.home() / "Documentos/Open Rails/Content"
DEFAULT_AIR = CONTENT / "Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/RF_WP_DMBSH.eng"


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def profile(path):
    text = msts_text(path)
    keys = ["BrakeSystemType", "BrakeEquipmentType", "BrakeCylinderPressureForMaxBrakeBrakeForce",
            "ORTSBrakeForceReferencePressure", "MaxApplicationRate", "MaxReleaseRate",
            "MaxAuxilaryChargingRate", "MaxAuxiliaryChargingRate", "TripleValveRatio",
            "EmergencyResCapacity", "EmergencyResVolumeMultiplier", "EmergencyResChargingRate",
            "ORTSEmergencyValveActuationRate", "ORTSEmergencyDumpValveRate", "ORTSEmergencyDumpValveTimer",
            "ORTSEmergencyQuickAction", "ORTSBrakeCylinderDiameter", "ORTSBrakeCylinderPistonTravel",
            "ORTSAuxiliaryResCapacity", "BrakePipeVolume", "ORTSInitialApplicationThreshold",
            "ORTSBrakeInsensitivity", "ORTSMaxServiceApplicationRate", "ORTSMaxServiceCylinderPressure",
            "TrainBrakesControllerMaxSystemPressure", "TrainBrakesControllerFullServicePressureDrop",
            "TrainBrakesControllerMaxApplicationRate", "TrainBrakesControllerMaxReleaseRate",
            "EngineBrakesControllerMaxSystemPressure", "EngineBrakesControllerMaxApplicationRate",
            "EngineBrakesControllerMaxReleaseRate", "ORTSBrakePipeTimeFactor", "ORTSBrakeServiceTimeFactor",
            "ORTSBrakeEmergencyTimeFactor"]
    fields = {}
    for key in keys:
        declarations = list(msts_blocks(text, key))
        if declarations:
            fields[key] = declarations[-1].strip()
    force = list(msts_blocks(text, "MaxBrakeForce"))
    if not force or "air_single_pipe" not in fields.get("BrakeSystemType", "").lower():
        raise ValueError("Expected identified original single-pipe air stock with MaxBrakeForce")
    return {"id": "rf_blue_pullman_author_legacy_emergency", "source_path": str(path.resolve()),
            "source_sha256": digest(path), "derivation": "Original numeric brake/controller fields; no vehicle speed/force is prescribed.",
            "fields": fields, "length_m": 20.0, "max_force": force[-1].strip(),
            "charged_pressure": fields.get("TrainBrakesControllerMaxSystemPressure", "90psi")}


def inputs(path):
    original = profile(path)
    modern = copy.deepcopy(original)
    modern.update(id="rf_author_derived_rate_valve_dump", derivation="Original hardware numeric base, explicit modern actuation 15 psi/s, dump 25 psi/s and timer 5 s. An OR-supported author-derived variant, not unchanged original stock.")
    modern["fields"].update(ORTSEmergencyValveActuationRate="15psi/s", ORTSEmergencyDumpValveRate="25psi/s", ORTSEmergencyDumpValveTimer="5s")
    quick = copy.deepcopy(modern)
    quick.update(id="rf_author_derived_quick_action", derivation="Declared OR-supported quick-action variant without emergency reservoir; not unchanged original stock.")
    quick["fields"]["BrakeEquipmentType"] = "Triple_valve, Auxilary_reservoir"
    quick["fields"]["ORTSEmergencyQuickAction"] = "1"
    charged = float(original["charged_pressure"].lower().removesuffix("psi").strip())
    return {"schema": 1, "dt_s": 0.05,
            "pressure_psi": sorted(set(range(0, 301, 2)) | {0.01, 9.99, 10.01, 149.99, 150.01, 299.99}),
            "brake_profiles": [original, modern, quick],
            "brake_phases": [
                {"id": "charged", "ticks": 40, "pipe_psi": charged, "independent_psi": 0},
                {"id": "service_ramp", "ticks": 80, "pipe_psi": charged - .25, "pipe_delta_per_tick": -.25, "independent_psi": 0},
                {"id": "service_hold", "ticks": 80, "pipe_psi": charged - 20, "independent_psi": 0},
                {"id": "release_ramp", "ticks": 100, "pipe_psi": charged - 19.8, "pipe_delta_per_tick": .2, "independent_psi": 0},
                {"id": "release", "ticks": 240, "pipe_psi": charged, "independent_psi": 0},
                {"id": "independent_only", "ticks": 80, "pipe_psi": charged, "independent_psi": 25},
                {"id": "independent_release", "ticks": 80, "pipe_psi": charged, "independent_psi": 0},
                {"id": "emergency", "ticks": 240, "pipe_psi": 0, "independent_psi": 0},
                {"id": "rearm", "ticks": 1200, "pipe_psi": charged, "independent_psi": 0}]}


def steam_facade(original_path, engine, destination):
    """Symlink read-only original route/assets, create only the owned consist."""
    route_container=next(p for p in original_path.parents if p.name.casefold()=="routes")
    content=route_container.parent
    destination.mkdir()
    for entry in content.iterdir():
        if entry.name.casefold()!="trains":
            (destination/entry.name).symlink_to(entry,target_is_directory=entry.is_dir())
    trains=destination/"TRAINS"; trains.mkdir(); trainset=trains/"TRAINSET"; trainset.mkdir()
    original_trains=next(p for p in content.iterdir() if p.name.casefold()=="trains")
    original_set=next(p for p in original_trains.iterdir() if p.name.casefold()=="trainset")
    for entry in original_set.iterdir():
        (trainset/entry.name).symlink_to(entry,target_is_directory=entry.is_dir())
    alias="openrailsrs_physical_seven_original_steam"
    (trainset/alias).symlink_to(engine.parent,target_is_directory=True)
    consists=trains/"CONSISTS"; consists.mkdir(); con=consists/"physical_seven_original_steam.con"
    con.write_text('SIMISA@@@@@@@@@@JINX0D0t______\nTrain ( TrainCfg ( physical_seven_original_steam\n'
        ' Name ( "Original steam diagnostic" ) Serial (1) MaxVelocity (30 1) NextWagonUID (1) Durability (1)\n'
        f' Engine ( UiD (0) EngineData ( "{engine.stem}" "{alias}" ) ) ) )\n',encoding="utf-16")
    return destination/original_path.relative_to(content),con


def lexical_windows(path):
    return "Z:"+str(path.absolute()).replace("/","\\")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--vehicle", type=Path, default=DEFAULT_AIR)
    p.add_argument("--installation-root", type=Path, default=Path.home()/"wine64-OpenRails/drive_c/Program Files/Open Rails")
    p.add_argument("--wine-source-prefix", type=Path, default=Path.home()/"wine64-OpenRails")
    p.add_argument("--work-dir", type=Path, required=True)
    p.add_argument("--out-dir", type=Path, required=True)
    p.add_argument("--prepare-only", action="store_true")
    p.add_argument("--steam-engine", type=Path)
    p.add_argument("--steam-path", type=Path)
    args = p.parse_args()
    if bool(args.steam_engine)!=bool(args.steam_path):
        raise ValueError("Original steam engine and original PAT must be supplied together")
    out=args.out_dir.resolve(); work=args.work_dir.resolve(); prefix=work/"wine-prefix"
    protected=[CONTENT.resolve(),args.installation_root.resolve(),args.wine_source_prefix.resolve(),(ROOT.parent/"openrails").resolve()]
    for directory in (out,work):
        if any(directory.is_relative_to(path) for path in protected):
            raise ValueError("Capture must stay outside original content, source and prefix")
    if out.exists() or prefix.exists():
        raise ValueError("Choose fresh evidence and private-prefix directories")
    pin=tomllib.loads((ROOT/"oracles/openrails-reference.toml").read_text())
    verify(pin,ROOT.parent/"openrails",args.installation_root)
    out.mkdir(parents=True); work.mkdir(parents=True,exist_ok=True)
    write(out/"inputs.json",inputs(args.vehicle))
    shutil.copy2(PROTOCOL,out/"protocol.json")
    shutil.copy2(Path(__file__),out/"capture-script.py")
    shutil.copy2(HELPER,out/HELPER.name)
    registration={"schema":1,"reference_version":pin["version"],"reference_commit":pin["commit"],
        "phase":"registered_before_wine_and_rust_comparison","helper_sha256":digest(HELPER),
        "capture_script_sha256":digest(__file__),"inputs_sha256":digest(out/"inputs.json"),
        "protocol_sha256":digest(PROTOCOL),"wine_isolation_helper_sha256":digest(ROOT/"scripts/oracle_wine.py"),
        "assemblies":{n:digest(args.installation_root/n) for n in REFERENCES},
        "source_files":{str(path):digest(path) for path in [args.vehicle,
            ROOT.parent/"openrails/Source/Orts.Simulation/Common/SteamTable.cs",
            ROOT.parent/"openrails/Source/Orts.Simulation/Simulation/RollingStocks/SubSystems/Brakes/MSTS/AirSinglePipe.cs",
            ROOT.parent/"openrails/Source/Orts.Simulation/Simulation/RollingStocks/MSTSSteamLocomotive.cs"]}}
    write(out/"registration.json",registration)
    if args.steam_engine:
        steam_config={"ticks":1600,"dt_s":0.05,"kind":"native_diagnostic_drive_no_full_cycle_parity_claim",
            "rust_cycle_options":{"startup":"or_hot","boiler_efficiency":0.7,"heat_loss_w":15000.0,"feedwater_c":20.0,
                "cylinder_efficiency":0.12,"idle_steam_kg_s":0.05,
                "volume_and_coal_heat":"Original initialized numeric parameters, not fitted from trajectory"},
            "commands":[{"tick":tick,"phase":phase,"throttle":throttle,"brake":brake,"cutoff":cutoff,"damper":damper,
                         "firing":firing,"injector1":inj1,"injector2":inj2}
                for tick,phase,throttle,brake,cutoff,damper,firing,inj1,inj2 in [
                    (0,"idle_fire",0,1,.65,.2,.2,False,False),
                    (200,"steam_load",.65,0,.65,.9,.8,False,False),
                    (800,"injection",.65,0,.45,.9,.8,True,False),
                    (1100,"coast",0,0,.45,.3,.3,False,False),
                    (1400,"braking",0,1,.45,.3,.3,False,False)]]}
        write(out/"steam-drive-config.json",steam_config)
        registration["steam_originals"]={str(p.resolve()):digest(p) for p in [args.steam_engine,args.steam_path]}
        registration["steam_drive_config_sha256"]=digest(out/"steam-drive-config.json")
        write(out/"registration.json",registration)
    if args.prepare_only:
        print(json.dumps(registration,indent=2)); return
    clone_prefix(args.wine_source_prefix,prefix)
    runtime=out/"runtime"; shutil.copytree(args.installation_root,runtime)
    with OracleWine(prefix,out/"wine-isolation") as wine:
        wine.run(["wine",r"C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe",
                  "/nologo","/out:PhysicalSystemsReference.exe"]+["/r:"+n for n in REFERENCES]+[windows_path(HELPER)],
                 cwd=runtime,log=out/"compile.log",timeout=90)
        wine.run(["wine","PhysicalSystemsReference.exe",windows_path(out/"inputs.json"),windows_path(out/"reference.json")],
                 cwd=runtime,log=out/"capture.log",timeout=120)
        if args.steam_engine:
            path,con=steam_facade(args.steam_path.resolve(),args.steam_engine.resolve(),out/"content")
            wine.run(["wine","PhysicalSystemsReference.exe","steam-drive",lexical_windows(path),windows_path(con),
                      windows_path(out/"native-steam"),windows_path(out/"steam-drive-config.json")],
                     cwd=runtime,log=out/"native-steam.log",timeout=180)
    verify(pin,ROOT.parent/"openrails",runtime)
    reference=json.loads((out/"reference.json").read_text())
    if len(reference["brakes"]) != 3 or len(reference["brake_networks"]) != 3 or not reference["properties"]:
        raise RuntimeError("Original helper returned incomplete coverage")
    coverage={"properties":len(reference["properties"]),"subsystems":[],"networks":[]}
    expected_ticks=sum(p["ticks"] for p in json.loads((out/"inputs.json").read_text())["brake_phases"])
    for case in reference["brakes"]:
        rows=case["points"]
        summary={"id":case["id"],"rows":len(rows),
            "emergency_seen":any(r["valve"]=="Emergency" for r in rows),
            "independent_cylinder_seen":any(r["phase"]=="independent_only" and r["cylinder_psi"]>0 for r in rows),
            "rearm_release_seen":bool(rows and rows[-1]["phase"]=="rearm" and rows[-1]["valve"]=="Release")}
        coverage["subsystems"].append(summary)
        if len(rows)!=expected_ticks or not all(summary[k] for k in ["emergency_seen","independent_cylinder_seen","rearm_release_seen"]):
            raise RuntimeError("Insufficient original valve phase coverage: "+json.dumps(summary))
    for case in reference["brake_networks"]:
        rows=case["points"]
        summary={"id":case["id"],"rows":len(rows),
            "three_measured_cars":all(len(r["cars"])==3 for r in rows),
            "tail_emergency_seen":any(r["cars"][-1]["valve"]=="Emergency" for r in rows),
            "nonuniform_pressure_seen":any(abs(r["cars"][0]["pipe_psi"]-r["cars"][-1]["pipe_psi"])>1e-5 for r in rows),
            "tail_rearm_release_seen":bool(rows and rows[-1]["phase"]=="rearm" and rows[-1]["cars"][-1]["valve"]=="Release")}
        coverage["networks"].append(summary)
        if len(rows)!=expected_ticks or not all(summary[k] for k in ["three_measured_cars","tail_emergency_seen","nonuniform_pressure_seen","tail_rearm_release_seen"]):
            raise RuntimeError("Insufficient original propagation phase coverage: "+json.dumps(summary))
    if args.steam_engine:
        drive=json.loads((out/"native-steam/steam-drive.json").read_text())
        rows=drive["rows"]
        coverage["steam_drive"]={"rows":len(rows),"moved":any(r["speed_mps"]>0.1 for r in rows),
            "production_seen":any(r["evaporation_lb_s"]>0 for r in rows),
            "steam_consumption_seen":any(r["steam_usage_lb_s"]>0 for r in rows),
            "injection_seen":any(r["water_consumption_lb_s"]>0 for r in rows)}
        if len(rows)!=drive["input"]["ticks"]+1 or not all(v for k,v in coverage["steam_drive"].items() if k!="rows"):
            raise RuntimeError("Insufficient original SteamDrive phase coverage")
    write(out/"coverage.json",coverage)
    files=["inputs.json","protocol.json","registration.json","capture-script.py",HELPER.name,"reference.json",
           "wine-isolation/isolation.json","wine-isolation/release.json","coverage.json"]
    if args.steam_engine:
        files += ["steam-drive-config.json","native-steam/steam-drive.json"]
    manifest={**registration,"capture_kind":"original_dll_calls","status":"captured_not_compared",
              "files":{n:digest(out/n) for n in files},"coverage":coverage,"native_steam_drive_captured":bool(args.steam_engine),
              "scope":"Prescribed pressure/pipe3 subsystem and three-car native propagation with only head pipe1 prescribed, original legacy RF numeric profile and two declared variants; property table queries. Optional original steam drive is a diagnostic reference. No substituted formula oracle or full-vehicle steam parity."}
    write(out/"manifest.json",manifest)
    print(json.dumps(manifest,indent=2))


if __name__=="__main__":
    main()
