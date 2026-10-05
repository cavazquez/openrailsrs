#!/usr/bin/env python3
"""Export Class 47 numeric fixtures from its original ENG/WAG and Include files.

No images, shapes, sounds or downloaded archives enter the repository. Includes
are bounded to the selected original trainset, checked for cycles and ambiguity.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

from prepare_chiltern_physics import FIELDS
from prepare_chiltern_service import msts_text, msts_blocks
from prepare_native_pilot import resolve

ROOT = Path(__file__).resolve().parents[1]
VEHICLES = [
    ("MT_DD_CLASS_47_47706", "MT_DD_CLASS_47_47706.eng"),
    ("MT_DB_MKII_BlueGrey", "MT_DB_MK2_TSO_SC5169.wag"),
    ("MT_DB_MKII_BlueGrey", "MT_DB_MK2_TSO_SC5134.wag"),
    ("MT_DB_MKII_BlueGrey", "MT_DB_MK2_TSO_SC5147.wag"),
    ("MT_DB_MKII_BlueGrey", "MT_DB_MK2_FK_SC13415.wag"),
    ("MT_DB_MKII_BlueGrey", "MT_DB_MK2_TSO_SC5134.wag"),
    ("MT_DB_MKII_BlueGrey", "MT_DB_MK2_BSO_SC9411.wag"),
]
EXTRA_FIELDS = """Length DieselPowerTab ThrottleRPMTab ORTSElectricTrainSupply ORTSDieselEngineMinIdleRPM
ORTSDieselEngineMaxIdleRPM ORTSBrakeShoeFriction NumWheels ORTSMaxServiceCylinderPressure
BrakeCylinderPressureForMaxBrakeForce ORTSMaxTripleValveCylinderPressure
AirBrakesBrakeCylinderVolume ORTSBrakeCylinderVolume ORTSAuxiliaryResVolume
ORTSMaxAuxilaryChargingRate MaxAuxilaryChargingRate TripleValveRatio ORTSBrakeRelayValveRatio
ORTSNumberBrakeCylinders BrakePipeVolume ORTSBrakePipeChargingRate
TrainBrakesControllerMaxSystemPressure TrainBrakesControllerMaxReleaseRate
TrainBrakesControllerMaxApplicationRate TrainBrakesControllerFullServicePressureDrop""".split()


def expand(path, root, hashes, ancestors=()):
    path = resolve(path)
    if not path.is_relative_to(root) or path in ancestors or len(ancestors) >= 32:
        raise ValueError("Include escapes trainset, cycles or exceeds 32 levels")
    if path.stat().st_size > 8 * 1024 * 1024:
        raise ValueError("Native parameter file exceeds 8 MiB")
    hashes[str(path.relative_to(root))] = hashlib.sha256(path.read_bytes()).hexdigest()
    text = msts_text(path)
    pattern = r'(?im)^\s*Include\s*\(\s*(?:"([^"]+)"|([^\s)]+))\s*\)'

    def include(match):
        name = (match[1] or match[2]).replace("\\", "/")
        return "\n" + expand(path.parent / name, root, hashes, (*ancestors, path)) + "\n"

    text = re.sub(pattern, include, text)
    if len(text) > 16 * 1024 * 1024:
        raise ValueError("Expanded numeric source exceeds 16 MiB")
    return text


def prepare(stock, destination):
    stock = stock.resolve(strict=True)
    sources = {}
    for folder, name in dict.fromkeys(VEHICLES):
        text = expand(stock / folder / name, stock, sources)
        fields = []
        for key in dict.fromkeys([*FIELDS, *EXTRA_FIELDS]):
            matches = list(re.finditer(r"(?m)^\s*" + re.escape(key) + r"\s*\(", text))
            body = ("\n".join(msts_blocks(text, key)) if key == "ORTSElectricTrainSupply" else
                    next(msts_blocks(text[matches[-1].start():], key), None)) if matches else None
            if body is not None:
                fields.append(f"  {key} ( {body.strip()} )")
        kind = "Engine" if name.endswith(".eng") else "Wagon"
        fixture = f'{kind} ( "{Path(name).stem}"\n  Name ( "{Path(name).stem}" )\n'
        fixture += "\n".join(fields) + "\n)\n"
        path = destination / "trains" / folder / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("\n".join(line.rstrip() for line in fixture.splitlines()) + "\n")
    con = destination / "consists/class47-mk2.con"
    con.parent.mkdir(parents=True, exist_ok=True)
    con.write_text("(Train\n" + "".join(
        f'  ({"Engine" if name.endswith(".eng") else "Wagon"} "trains/{folder}/{name}")\n'
        for folder, name in VEHICLES) + ")\n")
    (destination / "provenance.json").write_text(json.dumps({
        "reference_version": "Open Rails 1.6.1", "original_package": "Demo Model 1",
        "generator": "scripts/prepare_formation_physics.py", "source_sha256": sources,
        "scope": "numeric physical parameters; original downloads remain outside Git",
    }, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stock-root", required=True, type=Path)
    parser.add_argument("--out-dir", type=Path, default=ROOT / "examples/class47_reference/physics")
    args = parser.parse_args()
    prepare(args.stock_root, args.out_dir)
