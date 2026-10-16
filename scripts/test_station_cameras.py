import copy
import hashlib
import json
import math
from pathlib import Path
import tempfile
import unittest

from check_station_cameras import STATIONS, check, compare
import compare_station_views


class StationCameras(unittest.TestCase):
    def setUp(self):
        self.native = {
            "version": "1.6.1", "tile_x": -1, "tile_z": 2,
            "location": "10,20,30",
            "view_matrix": {"M13": 0, "M23": 0, "M33": 1},
            "projection_matrix": {"M11": 1, "M22": 16/9},
        }
        self.camera = {
            "position_world": [-2038, 20, -4126],
            "rotation_xyzw": [0, 0, 0, 1],
            "fov_y_rad": 2*math.atan(9/16), "aspect_ratio": 16/9,
        }

    def test_native_tile_sign_and_identity_projection(self):
        self.assertTrue(all(value == 0 for value in compare(self.native, self.camera).values()))
        shifted = copy.deepcopy(self.camera)
        shifted["position_world"][0] += 9.4
        self.assertAlmostEqual(compare(self.native, shifted)["position_error_m"], 9.4)
        turned = copy.deepcopy(self.camera)
        turned["rotation_xyzw"] = [0, math.sin(math.pi/8), 0, math.cos(math.pi/8)]
        self.assertAlmostEqual(compare(self.native, turned)["direction_error_deg"], 45)
        turned["rotation_xyzw"] = [0, 0, 0, 0]
        with self.assertRaises(ValueError):
            compare(self.native, turned)

    def test_pose_budget_and_incomplete_terrain_reject_acceptance(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            report = {}
            for station in STATIONS:
                for view in ("cab", "exterior"):
                    name = f"{station}-{view}"
                    png = folder / f"{name}.png"
                    png.write_bytes(b"unittest screenshot")
                    native = dict(self.native, image_sha256=hashlib.sha256(png.read_bytes()).hexdigest())
                    (folder / f"{name}.json").write_text(json.dumps(native))
                    key = station + ("-cab" if view == "cab" else "")
                    report[key] = {"pending_gpu_uploads": 0, "pending_terrain_tiles": 0,
                                   "unactivated_near_shapes": 0, "camera": copy.deepcopy(self.camera)}
            self.assertEqual(len(check(report, folder)["views"]), 12)
            report["northolt-park"]["pending_terrain_tiles"] = 1
            with self.assertRaisesRegex(ValueError, "Incomplete scenery"):
                check(report, folder)
            report["northolt-park"]["pending_terrain_tiles"] = 0
            report["northolt-park"]["camera"]["position_world"][0] += 9.4
            with self.assertRaisesRegex(ValueError, "exceeds"):
                check(report, folder)

    def test_station_light_comparison_rejects_mismatch_and_changed_original(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            reference = folder / "native"
            reference.mkdir()
            for view, hash_key in [("exterior", "image_sha256"), ("cab", "png_sha256")]:
                png = reference / f"test-{view}.png"
                png.write_bytes(b"native captured pixels")
                native = dict(self.native, time_s=35700, solar_direction=[0.6, 0.8, 0],
                              **{hash_key: hashlib.sha256(png.read_bytes()).hexdigest()})
                (reference / f"test-{view}.json").write_text(json.dumps(native))
                name = "test" + ("-cab" if view == "cab" else "")
                (folder / f"{name}.png").write_bytes(b"bevy captured pixels")
                bevy = {"camera": self.camera, "solar_direction": [0.6, 0.8, 0], "clock_time_s": 35700,
                        "pending_gpu_uploads": 0, "pending_terrain_tiles": 0, "unactivated_near_shapes": 0,
                        "performance": {"gameplay_frame_p95_ms": 25.0}, "draw_calls": 100,
                        "floating_origin_shift": 0, "renderer": "test"}
                (folder / f"{name}.stream.json").write_text(json.dumps(bevy))
            self.assertTrue(compare_station_views.compare(folder, ["test"], references=reference)["passed"])
            capture = folder / "test.stream.json"
            bevy = json.loads(capture.read_text())
            bevy["solar_direction"] = [0, 1, 0]
            capture.write_text(json.dumps(bevy))
            result = compare_station_views.compare(folder, ["test"], references=reference)
            self.assertFalse(result["passed"])
            self.assertFalse(result["views"][0]["checks"]["native_sun"])
            bevy["camera"]["position_world"][0] += 4
            capture.write_text(json.dumps(bevy))
            result = compare_station_views.compare(folder, ["test"], require_native_sun=False, references=reference)
            self.assertFalse(result["passed"])
            (reference / "test-exterior.png").write_bytes(b"changed reference")
            with self.assertRaisesRegex(ValueError, "Original reference changed"):
                compare_station_views.compare(folder, ["test"], references=reference)


if __name__ == "__main__":
    unittest.main()
