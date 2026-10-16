#!/usr/bin/env python3
"""Check equivalent station camera/light conditions before reviewing content.

OR and Bevy use different renderers: this deliberately does not assign a global
pixel similarity score. Original PNG hashes, camera poses, frozen solar vectors
and actual render costs remain independently inspectable in the receipt.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path

from check_station_cameras import LIMITS, compare as compare_camera

ROOT = Path(__file__).resolve().parents[1]
REFERENCE = ROOT / "docs/fixtures/visual/or_reference/chiltern_station_views"
TOLERANCES = {"camera": LIMITS, "solar_component": 5e-6, "clock_s": 1e-6}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def compare(directory, stations, require_native_sun=True, references=REFERENCE):
    if not stations or len(set(stations)) != len(stations):
        raise ValueError("Choose unique, nonempty station views")
    rows = []
    for station in stations:
        for view in ("exterior", "cab"):
            label = station if view == "exterior" else station + "-cab"
            native_path = references / (station + "-" + view + ".json")
            native = json.loads(native_path.read_text())
            if native.get("version") != "1.6.1":
                raise ValueError("Unpinned native viewer version")
            native_png = native_path.with_suffix(".png")
            if digest(native_png) != native.get("image_sha256", native.get("png_sha256")):
                raise ValueError(f"Original reference changed: {native_png}")
            bevy_path = directory / (label + ".stream.json")
            bevy = json.loads(bevy_path.read_text())
            for solar_vector in (native["solar_direction"], bevy["solar_direction"]):
                if len(solar_vector) != 3 or not all(math.isfinite(v) for v in solar_vector):
                    raise ValueError("Solar vectors need three finite components")
                if abs(sum(v*v for v in solar_vector)-1) > 0.001:
                    raise ValueError("Solar vector must have unit length")
            image = directory / (label + ".png")
            camera = compare_camera(native, bevy["camera"])
            position = camera["position_error_m"]
            angle = math.radians(camera["direction_error_deg"])
            solar = max(abs(a-b) for a, b in zip(native["solar_direction"], bevy["solar_direction"]))
            checks = {
                **{name: math.isfinite(error) and error <= LIMITS[name] for name, error in camera.items()},
                "clock": abs(native["time_s"]-bevy["clock_time_s"]) <= TOLERANCES["clock_s"],
                "native_sun": not require_native_sun or solar <= TOLERANCES["solar_component"],
                "ready_uploads": bevy["pending_gpu_uploads"] == 0,
                "ready_terrain": bevy["pending_terrain_tiles"] == 0,
                "ready_shapes": not bevy["unactivated_near_shapes"],
                "finite_frame_cost": math.isfinite(bevy["performance"]["gameplay_frame_p95_ms"]),
            }
            rows.append({"station": station, "view": view, "checks": checks,
                         "position_error_m": position, "forward_error_rad": angle,
                         "solar_max_component_error": solar,
                         "native_metadata_sha256": digest(native_path),
                         "native_image_sha256": digest(native_png),
                         "bevy_metadata_sha256": digest(bevy_path),
                         "bevy_image_sha256": digest(image),
                         "performance": bevy["performance"], "draw_calls": bevy["draw_calls"],
                         "floating_origin_shift": bevy["floating_origin_shift"],
                         "renderer": bevy["renderer"]})
    return {"schema": 1, "scope": "Camera/light equivalence and readiness; content requires visual inspection and geometry oracles, not a global pixel score",
            "reference": "Open Rails 1.6.1", "tolerances_predeclared": TOLERANCES,
            "native_sun_required": require_native_sun, "views": rows,
            "passed": all(all(row["checks"].values()) for row in rows)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--captures", type=Path, required=True)
    parser.add_argument("--station", action="append", default=[])
    parser.add_argument("--allow-continuous-sun", action="store_true")
    parser.add_argument("--references", type=Path, default=REFERENCE)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = compare(args.captures, args.station or ["northolt-park", "south-ruislip", "west-ruislip"], not args.allow_continuous_sun, args.references)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"views": len(result["views"]), "passed": result["passed"]}))
    if not result["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
