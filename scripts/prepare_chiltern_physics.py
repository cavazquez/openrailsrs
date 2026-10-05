#!/usr/bin/env python3
"""Extract authored physics parameters; never copy scenery or downloaded assets.

The accepted short oracles retain their historical converted stock. The full
service uses this independent native formation, with provenance per source file.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

from prepare_chiltern_service import msts_blocks, msts_text

ROOT = Path(__file__).resolve().parents[1]
NAMES = ["RF_WP_DMBSA.eng", "RF_WP_PSB.wag", "RF_WP_KFC.wag",
         "RF_WP_PCFD.wag", "RF_WP_PCFE.wag", "RF_WP_KFF.wag",
         "RF_WP_PSG.wag", "RF_WP_DMBSH.eng"]
FIELDS = """Mass Size MaxPower MaxForce MaxContinuousForce MaxVelocity MaxSpeed
MaxBrakeForce ORTSMaxBrakeShoeForce ORTSNumberCarBrakeShoes ORTSBrakeShoeType
BrakeSystemType BrakeEquipmentType ORTSEPBrakeControlsBrakePipe
ORTSBrakeCylinderDiameter ORTSBrakeCylinderPistonTravel ORTSCylinderSpringPressure
ORTSAuxiliaryResCapacity ORTSMaxServiceApplicationRate MaxApplicationRate
MaxReleaseRate MaxAuxiliaryChargingRate ORTSBrakeForceReferencePressure
BrakeCylinderPressureForMaxBrakeBrakeForce ORTSTwoStageLowPressure
ORTSMaxServiceCylinderPressure ORTSMaxTripleValveCylinderPressure
ORTSTwoStageIncreasingSpeed ORTSTwoStageDecreasingSpeed ORTSBearingType
ORTSDavis_A ORTSDavis_B ORTSDavis_C ORTSDavisDragConstant Friction
ORTSDriveWheelWeight WheelRadius NumWheels ORTSNumberAxles ORTSNumberDriveAxles
IdleRPM MaxRPM DieselEngineIdleRPM DieselEngineMaxRPM DieselEngineMaxRPMChangeRate
RunUpTimeToMaxForce ORTSDieselEngines ORTSMaxTractiveForceCurves
ORTSMaxRailOutputPower ORTSTractiveForcePowerLimited ORTSUnloadingSpeed
TractionForceRampUpNpS TractionForceRampDownNpS TractionForceRampDownToZeroNpS
TractionPowerRampUpWpS TractionPowerRampDownWpS ContinuousForceTimeFactor
DoesBrakeCutPower BrakeCutsPowerAtBrakeCylinderPressure Throttle ORTSLengthBogieCentre
CouplingHasRigidConnection
""".split()


def extract(source, name):
    # Only field openers at the start of a line, never words in Comment bodies.
    matches = list(re.finditer(r"(?m)^\s*" + re.escape(name) + r"\s*\(", source))
    if not matches:
        return None
    # Engine NumWheels controls the driven axle count; a Wagon NumWheels with
    # the same name earlier in an .eng describes the visible wheelset instead.
    match = matches[-1] if name == "NumWheels" else matches[0]
    bodies = list(msts_blocks(source[match.start():], name))
    return bodies[0] if bodies else None


def prepare(content, destination):
    stock = content / "TRAINS/TRAINSET/RF_Blue_Pullman"
    trains = destination / "trains/RF_Blue_Pullman"
    trains.mkdir(parents=True, exist_ok=True)
    sources = {}
    for name in NAMES:
        path = stock / name
        source = msts_text(path)
        fields = []
        for key in FIELDS:
            body = extract(source, key)
            if body is not None:
                fields.append(f"  {key} ( {body.strip()} )")
        kind = "Engine" if name.endswith(".eng") else "Wagon"
        text = f'{kind} ( "{Path(name).stem}"\n  Name ( "{Path(name).stem}" )\n' + "\n".join(fields) + "\n)\n"
        # Normalize authored CRLF and trailing spaces without changing tokens.
        text = "\n".join(line.rstrip() for line in text.splitlines()) + "\n"
        output = trains / name
        output.write_text(text, encoding="utf-8")
        sources[name] = {"source_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                         "fixture_sha256": hashlib.sha256(output.read_bytes()).hexdigest()}
    con = destination / "consists/birmingham_pullman.con"
    con.parent.mkdir(parents=True, exist_ok=True)
    con.write_text("(Train\n" + "".join(
        f'  ({"Engine" if name.endswith(".eng") else "Wagon"} "trains/RF_Blue_Pullman/{name}")\n'
        for name in NAMES) + ")\n", encoding="utf-8")
    (destination / "provenance.json").write_text(json.dumps({
        "content": "Chiltern original RF Blue Pullman, Birmingham Pullman formation",
        "generator": "scripts/prepare_chiltern_physics.py",
        "reference_version": "Open Rails 1.6.1",
        "sources": sources}, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--content", required=True, type=Path)
    parser.add_argument("--out-dir", type=Path, default=ROOT / "examples/chiltern_local/physics")
    args = parser.parse_args()
    prepare(args.content.resolve(), args.out_dir.resolve())
