#!/usr/bin/env python3
"""Compare the six station cameras with the unchanged OR 1.6.1 references.

Use capture_route_views.py --with-cab --capture-or-focus --cab-fov-deg 45.
This verifies position/direction/projection, not lighting or pixel parity.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import sys
import tomllib

from run_oracles import ROOT, verify

STATIONS = ("northolt-park", "south-ruislip", "west-ruislip")


def compare(native, camera):
    x, y, z = map(float, native["location"].split(","))
    position = [native["tile_x"] * 2048 + x, y, -(native["tile_z"] * 2048 + z)]
    x, y, z, w = camera["rotation_xyzw"]
    if not all(math.isfinite(value) for value in (x, y, z, w)) or abs(x*x + y*y + z*z + w*w - 1) > 0.001:
        raise ValueError("Camera quaternion must be finite and normalized")
    forward = [-2 * (x*z + w*y), -2 * (y*z - w*x), -(1 - 2*(x*x + y*y))]
    matrix = native["view_matrix"]
    reference_forward = [-matrix["M13"], -matrix["M23"], -matrix["M33"]]
    length = math.sqrt(sum(v*v for v in forward) * sum(v*v for v in reference_forward))
    cosine = sum(a*b for a, b in zip(forward, reference_forward)) / length
    projection = native["projection_matrix"]
    return {
        "position_error_m": math.dist(position, camera["position_world"]),
        "direction_error_deg": math.degrees(math.acos(max(-1, min(1, cosine)))),
        "fov_error_deg": abs(math.degrees(camera["fov_y_rad"] - 2*math.atan(1/projection["M22"]))),
        "aspect_error": abs(camera["aspect_ratio"] - projection["M22"]/projection["M11"]),
    }


def check(report, references):
    results = {}
    limits = {"position_error_m": 3.0, "direction_error_deg": 1.0,
              "fov_error_deg": 0.05, "aspect_error": 0.001}
    for station in STATIONS:
        for view in ("exterior", "cab"):
            key = station + ("-cab" if view == "cab" else "")
            path = references / f"{station}-{view}.json"
            native = json.loads(path.read_text())
            if native["version"] != "1.6.1":
                raise ValueError(f"Unpinned native camera: {path}")
            png = path.with_suffix(".png")
            if hashlib.sha256(png.read_bytes()).hexdigest() != native.get("image_sha256", native.get("png_sha256")):
                raise ValueError(f"Native camera screenshot changed: {png}")
            actual = report[key]
            if actual["pending_gpu_uploads"] or actual["pending_terrain_tiles"] or actual["unactivated_near_shapes"]:
                raise ValueError(f"Incomplete scenery at {key}")
            errors = compare(native, actual["camera"])
            failures = [name for name, value in errors.items()
                        if not math.isfinite(value) or value > limits[name]]
            if failures:
                raise ValueError(f"Camera {key} exceeds {failures}: {errors}")
            results[key] = errors
    return {"reference_version": "1.6.1", "limits": limits, "views": results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--references", type=Path, default=ROOT / "docs/fixtures/visual/or_reference/chiltern_station_views")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    source = ROOT.parent / "openrails"
    verify(pin, source if source.is_dir() else None)
    result = check(json.loads(args.report.read_text()), args.references)
    result["reference_commit"] = pin["commit"]
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(result, indent=2) + "\n")
    for key, errors in result["views"].items():
        print(f"PASS {key}: {errors['position_error_m']:.3f} m, {errors['direction_error_deg']:.3f}°")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError) as error:
        print(f"Station camera oracle FAIL: {error}", file=sys.stderr)
        sys.exit(1)
