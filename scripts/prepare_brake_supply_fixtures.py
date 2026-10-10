#!/usr/bin/env python3
"""Extract numeric ENG/WAG parameters for the seven formations already in QA.

Original downloads, shapes, cab images, sounds and custom code remain outside
Git. Includes are bounded by the existing native-content reader.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shlex

from prepare_formation_physics import expand, EXTRA_FIELDS
from prepare_chiltern_physics import FIELDS
from prepare_chiltern_service import msts_blocks, msts_text
from prepare_native_pilot import resolve

FORMATIONS = {
    "Chiltern": ["Bristol Pullman.con", "121single.con", "1960CentralWR8Car.con",
                 "R Stock 6 Car.con", "Downton Hall LE.con", "KingLE.con"],
    "Demo Model 1": ["MT_MT_Class 47 & 6 mk2 PP.con"],
}
EXTRA = """Type BrakeEquipmentType ORTSBrakeCylinderSize ORTSBrakeCylinderDiameter
ORTSBrakeCylinderPistonTravel ORTSNumberBrakeCylinders ORTSDirectAdmissionValve
ORTSAuxiliaryResCapacity ORTSAuxilaryResCapacity ORTSAuxPowerOnDelay ORTSPowerOnDelay
ORTSTractionCutOffRelayClosingDelay ORTSCircuitBreakerClosingDelay ORTSPantographs
ORTSElectricTrainSupply ORTSBattery ORTSMasterKey ORTSEPBrakeControlsBrakePipe
ORTSEPBrakeInhibitsTripleValve ORTSTrainBrakeController ORTSEngineBrakeController
ORTSPowerSupply EngineBrakesControllerMaxSystemPressure
EngineBrakesControllerMaxApplicationRate EngineBrakesControllerMaxReleaseRate
TrainBrakesControllerEmergencyApplicationRate TrainBrakesControllerMinPressureDrop
TrainBrakesControllerFullServicePressureDrop TrainBrakesControllerMaxApplicationRate
TrainBrakesControllerMaxReleaseRate ORTSMaxServiceCylinderPressure
ORTSBrakeForceReferencePressure ORTSCylinderSpringPressure ORTSTwoStageLowPressure
ORTSTwoStageIncreasingSpeed ORTSTwoStageDecreasingSpeed ORTSNumberBrakeShoes""".split()


def prepare(content, out):
    content=content.resolve(strict=True)
    out=out.resolve()
    if out.is_relative_to(content):
        raise ValueError("Fixtures must stay outside the original content")
    if out.exists():
        raise ValueError("Choose a fresh fixture directory")
    out.mkdir(parents=True)
    profiles, provenance = [], {}
    for package, names in FORMATIONS.items():
        root = resolve(content / package / "TRAINS/TRAINSET")
        for name in names:
            con = resolve(content / package / "TRAINS/CONSISTS" / name)
            con_sources = []
            con_text = msts_text(con)
            for match in re.finditer(r"\b(EngineData|WagonData)\s*\(", con_text):
                kind = match[1]
                for body in [next(msts_blocks(con_text[match.start():], kind))]:
                    tokens = shlex.split(body)
                    if len(tokens) != 2:
                        raise ValueError(f"Unexpected native vehicle reference in {con.name}")
                    stem, folder = tokens
                    path = resolve(root / folder / (stem + (".eng" if kind == "EngineData" else ".wag")))
                    override = path.parent / "OpenRails" / path.name
                    if override.exists():
                        path = override
                    relative = f"{package}/{path.relative_to(root)}"
                    source_hashes = {}
                    text = expand(path, root, source_hashes)
                    source_kind = "Engine" if kind == "EngineData" else "Wagon"
                    slug = hashlib.sha256(relative.encode()).hexdigest()[:12]
                    destination = f"vehicles/{slug}{path.suffix.lower()}"
                    raw = {}
                    for key in dict.fromkeys([*FIELDS, *EXTRA_FIELDS, *EXTRA]):
                        blocks = list(msts_blocks(text, key))
                        if blocks:
                            raw[key] = blocks[-1].strip()
                    fixture = source_kind + ' ( "' + path.stem + '"\n'
                    fixture += "\n".join(f"  {key} ( {value} )" for key, value in raw.items()) + "\n)\n"
                    file = out / destination
                    file.parent.mkdir(exist_ok=True)
                    file.write_text(fixture)
                    if not any(p["source"] == relative for p in profiles):
                        profiles.append({"source": relative, "file": destination,
                                         "kind": source_kind, "fields": raw})
                    provenance.update({f"{package}/{p}": sha for p, sha in source_hashes.items()})
                    con_sources.append(destination)
            slug = re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")
            (out / (slug + ".con")).write_text("(Train\n" + "".join(
                f'  ({"Engine" if p.endswith(".eng") else "Wagon"} "{p}")\n'
                for p in con_sources) + ")\n")
    (out / "profiles.json").write_text(json.dumps(profiles, indent=2) + "\n")
    (out / "provenance.json").write_text(json.dumps({
        "reference_version": "1.6.1", "source_sha256": provenance,
        "scope": "Numeric brake/power parameters of seven installed formations; no original assets or scripts",
    }, indent=2) + "\n")
    print(f"Extracted {len(profiles)} numeric vehicle profiles")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--content-root", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    prepare(args.content_root.resolve(strict=True), args.out_dir.resolve())
