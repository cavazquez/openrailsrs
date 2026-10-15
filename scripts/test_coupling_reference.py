"""Guard the real original coupling capture and its bounded state comparison."""

import copy
import hashlib
import json
from pathlib import Path
import unittest

from verify_coupling_reference import validate_comparison, validate_reference

ROOT = Path(__file__).resolve().parents[1]


class CouplingReferenceTests(unittest.TestCase):
    def setUp(self):
        self.reference = json.loads((ROOT / "oracles/coupling-reference.json").read_text())
        self.manifest = json.loads((ROOT / "oracles/coupling-reference-provenance.json").read_text())
        self.replay = self.manifest["replays"][-1]
        self.rust_path = ROOT / self.replay["rust_output_file"]
        self.rust = json.loads(self.rust_path.read_text())

    def test_real_original_transfer_and_three_static_equilibria_pass(self):
        result = validate_comparison(self.reference, self.rust)
        self.assertEqual(result["transferred_original_vehicles"], 8)
        self.assertEqual(result["force_cases"], 3)
        self.assertLess(result["maximum_force_error_n"], 0.1)
        self.assertGreater(result["driving_after_restore_m"], 1)

    def test_reference_binds_pinned_dll_source_and_capture_helpers(self):
        manifest = self.manifest
        self.assertEqual(manifest["reference_commit"], "d16e670da333d26d2edfc97d5631a19dadf49ce5")
        self.assertEqual(manifest["assemblies"]["Orts.Simulation.dll"],
                         "dac17afff95141664ee87f734b64e89649699dc2c634196f38b0125d5df09679")
        for path, expected in manifest["files"].items():
            self.assertEqual(hashlib.sha256((ROOT / path).read_bytes()).hexdigest(), expected, path)
        replay = self.replay
        self.assertEqual(replay["status"], "pass")
        self.assertTrue(replay["source_inputs_unchanged_during_build"])
        self.assertEqual(replay["reference_sha256"],
                         hashlib.sha256((ROOT / "oracles/coupling-reference.json").read_bytes()).hexdigest())
        self.assertEqual(replay["rust_output_sha256"], hashlib.sha256(self.rust_path.read_bytes()).hexdigest())
        historical = (ROOT / "oracles/coupling-rust32.json").read_bytes()
        self.assertEqual(hashlib.sha256(historical).hexdigest(), manifest["rust_capture"]["rust_output_sha256"])
        self.assertEqual(replay["rust_output_byte_identical_to_historical32"],
                         self.rust_path.read_bytes() == historical)
        for path, expected in replay["runtime_source_files"].items():
            self.assertEqual(hashlib.sha256((ROOT / path).read_bytes()).hexdigest(), expected, path)
        for key, path in (("capture_script_sha256", "scripts/capture_coupling_comparison.py"),
                          ("coupling_helper_sha256", "tools/openrails-reference/CouplingCompare.rs"),
                          ("depot_helper_sha256", "tools/openrails-reference/DepotStateCompare.rs")):
            self.assertEqual(hashlib.sha256((ROOT / path).read_bytes()).hexdigest(), replay[key], path)
        for path, expected in replay["evidence_files"].items():
            self.assertEqual(hashlib.sha256((ROOT / path).read_bytes()).hexdigest(), expected, path)

    def test_historical_output_is_preserved_and_still_passes_its_original_acceptance(self):
        historical = json.loads((ROOT / "oracles/coupling-rust32.json").read_text())
        self.assertEqual(validate_comparison(self.reference, historical), self.manifest["comparison"])

    def test_current_replay_receipt_binds_its_actual_outputs_and_libraries(self):
        replay = self.replay
        receipt_path = ROOT / replay["raw_receipt_file"]
        self.assertEqual(hashlib.sha256(receipt_path.read_bytes()).hexdigest(), replay["raw_receipt_sha256"])
        receipt = json.loads(receipt_path.read_text())
        self.assertFalse(receipt["cargo_invoked"])
        self.assertFalse(receipt["runtime_modified"])
        self.assertFalse(receipt["original_assets_versioned"])
        self.assertEqual(receipt["dependencies"], replay["libraries"])
        self.assertEqual(receipt["original_files"], self.manifest["native_vehicle_files"])
        self.assertEqual(receipt["reference_sha256"], replay["reference_sha256"])
        self.assertEqual(receipt["rust_output_sha256"], replay["rust_output_sha256"])
        self.assertEqual(receipt["comparison"], validate_comparison(self.reference, self.rust))
        depot_path = ROOT / replay["depot_output_file"]
        self.assertEqual(hashlib.sha256(depot_path.read_bytes()).hexdigest(), replay["depot_output_sha256"])
        self.assertEqual(receipt["depot_output_sha256"], replay["depot_output_sha256"])
        historical = json.loads((ROOT / "docs/fixtures/compatibility/fifth40-20261013/depot-state-continuity.json").read_text())
        self.assertEqual(replay["depot_observed_identical_to_historical32"],
                         json.loads(depot_path.read_text()) == historical["observed"])

    def test_reference_cannot_reset_pressure_or_keep_a_donor_reservation(self):
        broken = copy.deepcopy(self.reference)
        broken["coupling"]["after"]["vehicles"][4]["cylinder_psi"] = 0
        with self.assertRaisesRegex(ValueError, "preserve cylinder"):
            validate_reference(broken)
        broken = copy.deepcopy(self.reference)
        broken["coupling"]["after"]["track"][0]["donor_occupied"] = True
        with self.assertRaisesRegex(ValueError, "occupation/reservations"):
            validate_reference(broken)

    def test_comparison_rejects_nan_force_and_lost_restored_fuel(self):
        broken = copy.deepcopy(self.rust)
        broken["force_equilibrium"][1]["coupler_force_rust_n"] = float("nan")
        with self.assertRaisesRegex(ValueError, "force outside"):
            validate_comparison(self.reference, broken)
        broken = copy.deepcopy(self.rust)
        broken["restored_before_drive"]["fuel"][0]["fuel_l"] = 0
        with self.assertRaisesRegex(ValueError, "restore lost fuel"):
            validate_comparison(self.reference, broken)

    def test_depot_control_preserves_nondefault_states_and_finite_supply(self):
        receipt = json.loads((ROOT / "docs/fixtures/compatibility/fifth40-20261013/depot-state-continuity.json").read_text())
        data = receipt["observed"]
        self.assertEqual(data["after_recovery"], data["control_after_same_time"])
        self.assertEqual(data["nondefault_initial"]["service"]["accrued_penalty"], 37.5)
        self.assertEqual([car["battery_on"] for car in data["after_recovery"]["formation"]["cars"]], [False, False])
        self.assertEqual(data["used_oil_l"], 50)
        self.assertEqual(data["used_coolant_l"], 70)
        self.assertTrue(data["finite_supply"]["faults_remain"])
        self.assertTrue(data["finite_supply"]["second_cycle_after_restore_creates_no_material"])
        self.assertEqual(hashlib.sha256((ROOT / "tools/openrails-reference/DepotStateCompare.rs").read_bytes()).hexdigest(),
                         receipt["build"]["helper_sha256"])


if __name__ == "__main__":
    unittest.main()
