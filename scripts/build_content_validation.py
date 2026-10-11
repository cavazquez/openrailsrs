#!/usr/bin/env python3
"""Index exact effective native sources for the pinned subsystem captures.

Requires the original TRAINSET, including Includes; copies no content assets.
Use --check to verify the committed index against an installed original package.
"""
import argparse
import hashlib
import json
from pathlib import Path
from prepare_formation_physics import expand

ROOT = Path(__file__).resolve().parents[1]


def fingerprint(sources):
    digest = hashlib.sha256()
    for name, sha in sorted((n.replace("\\", "/").lower(), sha)
                            for n, sha in sources.items()):
        encoded = name.encode()
        digest.update(len(encoded).to_bytes(8, "little"))
        digest.update(encoded)
        digest.update(sha.encode())
    return digest.hexdigest()


def build(trainset):
    fixed = json.loads((ROOT / "examples/brake_supply_native/provenance.json").read_text())["source_sha256"]
    evidence = ROOT / "oracles/openrails-brake-power.json"
    capture = json.loads(evidence.read_text())
    provenance = json.loads((ROOT / "oracles/openrails-brake-power-provenance.json").read_text())
    records = []
    for group, scope in [("vacuum", "Freno de vacío"),
                         ("legacy_ep", "Freno EP original sin geometría de cilindro")]:
        for item in capture[group]:
            package, relative = item["source"].split("/", 1)
            sources = {}
            expand(trainset / relative, trainset, sources)
            if not sources or any(fixed.get(package + "/" + name) != sha
                                  for name, sha in sources.items()):
                raise ValueError("Sources differ from captured provenance: " + item["source"])
            records.append({
                "source": item["source"], "source_sha256": fingerprint(sources),
                "source_files": dict(sorted(sources.items())),
                "scope": scope, "points": len(item["points"]),
                "evidence": "oracles/openrails-brake-power.json",
                "evidence_sha256": hashlib.sha256(evidence.read_bytes()).hexdigest(),
                "provenance": "oracles/openrails-brake-power-provenance.json",
            })
    return {"schema": 1, "reference_version": provenance["reference_version"],
            "reference_commit": provenance["reference_commit"], "records": records,
            "scope": "Comparaciones de subsistemas con entradas prescritas; no certifica circulación ni todos los sistemas del vehículo."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trainset", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    result = build(args.trainset.resolve(strict=True))
    destination = ROOT / "oracles/content-validation.json"
    if args.check:
        if json.loads(destination.read_text()) != result:
            raise SystemExit("Content evidence index is stale")
    else:
        destination.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    print(f"Verified content evidence: {len(result['records'])} profiles")


if __name__ == "__main__":
    main()
