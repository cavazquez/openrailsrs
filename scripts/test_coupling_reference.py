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
        self.rust = json.loads((ROOT / "oracles/coupling-rust32.json").read_text())

    def test_real_original_transfer_and_three_static_equilibria_pass(self):
        result = validate_comparison(self.reference, self.rust)
        self.assertEqual(result["transferred_original_vehicles"], 8)
        self.assertEqual(result["force_cases"], 3)
        self.assertLess(result["maximum_force_error_n"], 0.1)
        self.assertGreater(result["driving_after_restore_m"], 1)

    def test_reference_binds_pinned_dll_source_and_capture_helpers(self):
        manifest = json.loads((ROOT / "oracles/coupling-reference-provenance.json").read_text())
        self.assertEqual(manifest["reference_commit"], "d16e670da333d26d2edfc97d5631a19dadf49ce5")
        self.assertEqual(manifest["assemblies"]["Orts.Simulation.dll"],
                         "dac17afff95141664ee87f734b64e89649699dc2c634196f38b0125d5df09679")
        for path, expected in manifest["files"].items():
            self.assertEqual(hashlib.sha256((ROOT / path).read_bytes()).hexdigest(), expected, path)
        for path, expected in manifest["runtime_source_files"].items():
            self.assertEqual(hashlib.sha256((ROOT / path).read_bytes()).hexdigest(), expected, path)

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
