"""Regressions for native service capture, input timing and reference rejection."""

import csv
import hashlib
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

from capture_chiltern_service_or import clone_prefix, driver_keyframes, validate_replay, write_native
from verify_chiltern_service_capture import BASELINE, verify_capture
import prepare_chiltern_extended as extended
from sync_chiltern_assets import extract_curve_values, write_eng, write_wag


class CurveImportTests(unittest.TestCase):
    def test_export_keeps_authored_imperial_curve_values_and_does_not_invent_missing_ones(self):
        source = "Mass ( 50t ) ORTSTrackGauge ( 4ft 8.5in ) ORTSUnbalancedSuperelevation ( 6in )"
        with tempfile.TemporaryDirectory() as tmp:
            for name, writer in (("passenger.wag", write_wag), ("engine.eng", write_eng)):
                target = Path(tmp) / name
                writer(target, "vehicle", source, "")
                exported = target.read_text()
                self.assertIn("(ORTSTrackGauge 4ft 8.5in)", exported)
                self.assertIn("(ORTSUnbalancedSuperElevation 6in)", exported)
                self.assertEqual(exported.count("ORTSTrackGauge"), 1)
                self.assertEqual(exported.count("ORTSUnbalancedSuperElevation"), 1)
        self.assertEqual(extract_curve_values("Wagon ( x Type ( Engine ) )"), {})
        with self.assertRaisesRegex(ValueError, "Ambiguous"):
            extract_curve_values("ORTSTrackGauge ( 1m ) ORTSTrackGauge ( 2m )")

    def test_service_import_replaces_existing_values_once_and_preserves_other_physics(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            con = root / "examples/chiltern/consists/birmingham_pullman.con"
            con.parent.mkdir(parents=True)
            relative = "trains/RF_Blue_Pullman/passenger.wag"
            con.write_text(f'(Train (Wagon "{relative}"))')
            reduced = root / "examples/chiltern" / relative
            reduced.parent.mkdir(parents=True)
            reduced.write_text('(Wagon (Type "passenger")\n  (Mass 50000)\n  (MaxBrakeForce 44)\n'
                               '  (ORTSTrackGauge 0.5m)\n  (ORTSUnbalancedSuperElevation 0.01m)\n)\n')
            original = root / "Content/TRAINS/TRAINSET/RF_Blue_Pullman/passenger.wag"
            original.parent.mkdir(parents=True)
            original.write_text('Type ( Carriage ) BrakeSystemType ( "EP" )\n'
                                'BrakeCylinderPressureForMaxBrakeBrakeForce ( 90psi )\n'
                                'MaxApplicationRate ( 30psi/s ) MaxReleaseRate ( 10psi/s )\n'
                                'ORTSTrackGauge ( 4ft 8.5in ) ORTSUnbalancedSuperElevation ( 6in )')
            output = root / "extended"
            with patch.object(extended.native, "ROOT", root):
                profiles = extended.prepare_brake_profiles(root / "Content/ROUTES/Chiltern", output)
                first = (output / relative).read_text()
                extended.prepare_brake_profiles(root / "Content/ROUTES/Chiltern", output)
            self.assertEqual((output / relative).read_text(), first)
            self.assertEqual(first.count("ORTSTrackGauge"), 1)
            self.assertEqual(first.count("ORTSUnbalancedSuperElevation"), 1)
            self.assertIn("(Mass 50000)", first)
            self.assertIn("(MaxBrakeForce 44)", first)
            self.assertEqual(profiles[0]["tokens"]["ORTSUnbalancedSuperElevation"], "6in")


class CaptureTests(unittest.TestCase):
    def test_controls_start_on_the_step_that_consumed_them_and_keep_the_horizon(self):
        with tempfile.TemporaryDirectory() as tmp:
            trace, driver = Path(tmp) / "trace.csv", Path(tmp) / "driver.csv"
            trace.write_text("time_s,throttle,brake\n0,0,0\n0.05,0,1\n0.1,0.75,0\n0.15,0.75,0\n0.2,0.75,0\n")
            driver_keyframes(trace, driver, 0.05)
            with driver.open() as stream:
                frames = list(csv.DictReader(stream))
            self.assertEqual([float(row["time_s"]) for row in frames], [0, 0.05, 0.15])
            self.assertEqual(float(frames[0]["brake"]), 1)
            self.assertEqual(float(frames[1]["throttle"]), 0.75)
            validate_replay(driver)

    def test_invalid_replay_cannot_reach_the_original_loader(self):
        for data in ("0,0,1\n0,1,0\n", "0,0,1\n1,nan,0\n", "0,0,1\ninf,1,0\n", "0,0,1\n1,2,0\n"):
            with self.subTest(data=data), tempfile.TemporaryDirectory() as tmp:
                path = Path(tmp) / "bad.csv"
                path.write_text("time_s,throttle,brake\n" + data)
                with self.assertRaises(ValueError):
                    validate_replay(path)

    def test_private_prefix_does_not_share_writable_files_or_user_folders(self):
        with tempfile.TemporaryDirectory() as tmp:
            source, target = Path(tmp) / "source", Path(tmp) / "copy"
            compiler = source / "drive_c/windows/Microsoft.NET/Framework64/v4.0.30319/csc.exe"
            compiler.parent.mkdir(parents=True)
            compiler.write_text("compiler")
            (source / "system.reg").write_text("registry")
            (source / "user.reg").write_text("registry")
            os.link(source / "user.reg", source / "user-copy.reg")
            documents = source / "drive_c/users/player/Documents"
            documents.parent.mkdir(parents=True)
            documents.symlink_to(Path(tmp), target_is_directory=True)
            clone_prefix(source, target)
            (target / "user.reg").write_text("private change")
            self.assertEqual((source / "user.reg").read_text(), "registry")
            self.assertEqual((target / "user-copy.reg").read_text(), "private change")
            self.assertFalse((target / documents.relative_to(source)).is_symlink())
            self.assertEqual(list((target / documents.relative_to(source)).iterdir()), [])
            clone_prefix(source, target)  # Reusing this managed copy is safe.
            unmanaged = Path(tmp) / "unmanaged"
            unmanaged.mkdir()
            with self.assertRaisesRegex(ValueError, "Refusing an existing"):
                clone_prefix(source, unmanaged)

    def test_existing_native_name_cannot_overwrite_reference_through_a_symlink(self):
        with tempfile.TemporaryDirectory() as tmp:
            original, facade_file = Path(tmp) / "original.pat", Path(tmp) / "new.pat"
            original.write_text("original path")
            facade_file.symlink_to(original)
            with self.assertRaises(FileExistsError):
                write_native(facade_file, "replacement")
            self.assertEqual(original.read_text(), "original path")

    def test_frozen_reference_contains_the_whole_native_station_service(self):
        report = verify_capture(BASELINE)
        self.assertEqual(report["stations"], 3)
        self.assertEqual(report["samples"], 15003)
        self.assertGreater(report["distance_m"], 7000)

    def test_corrupted_reference_is_rejected_before_comparison(self):
        with tempfile.TemporaryDirectory() as tmp:
            copy = Path(tmp) / "reference"
            shutil.copytree(BASELINE, copy)
            with (copy / "capture/trace.csv").open("a") as stream:
                stream.write("corrupt\n")
            with self.assertRaisesRegex(ValueError, "Capture changed"):
                verify_capture(copy)

    def test_correct_hashes_do_not_hide_departure_with_open_doors(self):
        with tempfile.TemporaryDirectory() as tmp:
            copy = Path(tmp) / "reference"
            shutil.copytree(BASELINE, copy)
            trace = copy / "capture/trace.csv"
            with trace.open() as stream:
                reader = csv.DictReader(stream)
                fields, rows = reader.fieldnames, list(reader)
            next(row for row in rows if float(row["velocity_mps"]) > 5)["door_state"] = "Open"
            with trace.open("w", newline="") as stream:
                writer = csv.DictWriter(stream, fieldnames=fields, lineterminator="\n")
                writer.writeheader()
                writer.writerows(rows)
            manifest_path = copy / "manifest.json"
            manifest = json.loads(manifest_path.read_text())
            manifest["files"]["capture/trace.csv"] = manifest["trace_sha256"] = hashlib.sha256(trace.read_bytes()).hexdigest()
            manifest_path.write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, "departed with doors open"):
                verify_capture(copy)


if __name__ == "__main__":
    unittest.main()
