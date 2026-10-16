"""Reject changed native operational inputs, outputs, helpers or prior limits."""
import hashlib
import gzip
import json
from pathlib import Path
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class OperationalOracles(unittest.TestCase):
    def test_route_audit_keeps_link_inventory_and_uncertified_routes_explicit(self):
        folder = ROOT / "docs/fixtures/compatibility/operational-signals-routes"
        summary = json.loads((folder / "summary.json").read_text())
        for name, expected in summary["artifacts_sha256"].items():
            self.assertEqual(digest(folder / name), expected, name)
        receipt = json.loads(gzip.decompress((folder / "receipt.json.gz").read_bytes()))
        self.assertTrue(receipt["passed"])
        rows = receipt["routes"]
        self.assertEqual(len(rows), 10)
        self.assertEqual(sum(row["heads"] > 0 for row in rows), 9)
        self.assertEqual(sum(row["heads"] for row in rows), 24489)
        self.assertEqual(sum(len(row["links"]) for row in rows), 8995)
        self.assertEqual(sum(row["contexts_evaluated"] for row in rows), 46848)
        empty = [row for row in summary["counts"] if not row["heads"]]
        self.assertEqual(len(empty), 1)
        self.assertEqual(empty[0]["status"], "no_native_heads_imported_not_certified")

    def capture(self, name, helper, script):
        folder = ROOT / "docs/fixtures/compatibility" / name
        manifest = json.loads((folder / "manifest.json").read_text())
        protocol = json.loads((folder / "protocol.json").read_text())
        inputs = json.loads((folder / "input.json").read_text())
        pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
        self.assertEqual(manifest["reference_version"], "1.6.1")
        self.assertEqual(manifest["reference_commit"], pin["commit"])
        for file, key in [("input.json", "input_sha256"), ("reference.json", "output_sha256"), ("protocol.json", "protocol_sha256")]:
            self.assertEqual(digest(folder / file), manifest[key], file)
        self.assertEqual(digest(ROOT / "tools/openrails-reference" / helper), manifest["helper_sha256"])
        self.assertEqual(digest(ROOT / "scripts" / script), manifest["capture_script_sha256"])
        self.assertEqual(digest(ROOT / "scripts/oracle_wine.py"), manifest["wine_isolation_helper_sha256"])
        self.assertEqual(protocol["status"], "DECLARED_BEFORE_EXECUTION")
        self.assertEqual(protocol["input"], inputs)
        self.assertEqual(manifest["tolerances_predeclared"], inputs["tolerances_predeclared"])
        self.assertTrue(manifest["reference_source_and_dlls_unmodified"])
        binaries = {b["name"]: b["sha256"] for b in pin["binaries"]}
        for binary, expected in manifest["assemblies"].items():
            if binary in binaries:
                self.assertEqual(expected, binaries[binary], binary)
        isolation_path = folder / ("wine-isolation/isolation.json" if name == "bogie-dynamics-or161" else "wine-isolation.json")
        release_path = folder / ("wine-isolation/release.json" if name == "bogie-dynamics-or161" else "wine-release.json")
        isolation = json.loads(isolation_path.read_text())
        release = json.loads(release_path.read_text())
        self.assertEqual(digest(release_path), manifest["wine_isolation_release_sha256"])
        self.assertTrue(isolation["registry_verified"])
        self.assertEqual(isolation["show_crash_dialog"], 0)
        self.assertEqual(isolation["graphics_driver"], "null")
        self.assertFalse(isolation["original_prefix_touched"])
        self.assertEqual(release["remaining_processes"], [])
        self.assertEqual(release["forbidden_drivers_seen"], [])
        return json.loads((folder / "reference.json").read_text())

    def test_native_block_states_and_junction_links_have_exact_discrete_limits(self):
        data = self.capture("operational-signals-or161", "OperationalSignalsReference.cs", "capture_operational_signals_reference.py")
        self.assertEqual(len(data["blocks"]), 8)
        self.assertEqual(len(data["links"]), 18)
        self.assertEqual(len(data["bindings"]), 2)
        self.assertEqual({row["result"] for row in data["blocks"]}, {0, 1, 2})

    def test_real_world_sound_regions_include_hold_gaps_and_both_directions(self):
        data = self.capture("track-sound-or161", "TrackSoundReference.cs", "capture_track_sound_reference.py")
        self.assertEqual(len(data["rows"]), 100)
        self.assertEqual({row["reverse"] for row in data["rows"]}, {False, True})
        self.assertGreaterEqual(len(data["types"]), 5)
        self.assertIn(None, {row["result"] for row in data["rows"]})
        self.assertTrue(all(row["path"] for row in data["rows"]))

    def test_native_bogies_really_reach_lateral_risk_and_still_reset(self):
        data = self.capture("bogie-dynamics-or161", "BogieDynamicsReference.cs", "capture_bogie_dynamics_reference.py")
        self.assertEqual(len(data["rows"]), 48)
        # The moving speed cache is a required native input: all-false flags
        # would leave the important branch untested despite finite forces.
        possible = [row for row in data["rows"] if row["possible"]]
        self.assertGreater(len(possible), 0)


if __name__ == "__main__":
    unittest.main()
