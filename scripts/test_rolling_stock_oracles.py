"""Verify real OR 1.6.1 rolling-stock captures, their scope and prior limits.

This suite reads captured outputs. It never generates expected physical values
from a replacement model and never treats missing native evidence as a pass.
"""

import ast
import csv
import gzip
import hashlib
import io
import json
import math
from pathlib import Path
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
CATALOG = ROOT / "oracles/rolling-stock-families.json"
GEARBOX = ROOT / "docs/fixtures/compatibility/gearbox-or161"
REFERENCE_COMMIT = "d16e670da333d26d2edfc97d5631a19dadf49ce5"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def repository_file(name):
    path = ROOT / name
    if Path(name).is_absolute() or ".." in Path(name).parts or not path.resolve().is_relative_to(ROOT):
        raise ValueError(f"Evidence must be a repository file: {name}")
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"Missing regular captured evidence: {name}")
    return path


class RollingStockOracleTests(unittest.TestCase):
    def test_catalog_requires_every_selected_native_family_and_declares_pending_scope(self):
        catalog = json.loads(CATALOG.read_text())
        pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
        self.assertEqual(catalog["schema_version"], 1)
        self.assertEqual(catalog["reference_version"], pin["version"])
        self.assertEqual(catalog["reference_version"], "1.6.1")
        self.assertEqual(catalog["reference_commit"], REFERENCE_COMMIT)
        self.assertEqual(catalog["reference_commit"], pin["commit"])
        entries = catalog["captures"]
        self.assertEqual({entry["id"] for entry in entries},
                         {"diesel_gearbox_class101", "vacuum_independent", "coupler_forces"})
        self.assertEqual(catalog["status"], "complete")
        self.assertEqual(catalog["pending_captures"], [])
        self.assertTrue(catalog["pending_systems"])
        for entry in entries:
            with self.subTest(capture=entry["id"]):
                self.assertEqual(entry["status"], "native_capture_verified")
                self.assertTrue(entry["families"])
                self.assertTrue(entry["phases"])
                self.assertTrue(entry["native_methods"])
                self.assertTrue(entry["source_selection"])
                self.assertTrue(entry["scope_limits"])
                self.assertGreater(entry["native_points"], 0)
                self.assertEqual(entry["tolerance_declaration_phase"], "before_capture_and_rust_comparison")
                self.assertTrue(entry["tolerances"])
        pending = {entry["system"] for entry in catalog["pending_systems"]}
        self.assertIn("complete_steam_thermodynamics", pending)
        self.assertIn("complete_lirr_route_physics", pending)
        self.assertIn("authored_electric_voltage_protection", pending)

    def test_every_capture_artifact_and_original_binary_identity_is_pinned(self):
        catalog = json.loads(CATALOG.read_text())
        pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
        simulation = next(entry["sha256"] for entry in pin["binaries"] if entry["name"] == "Orts.Simulation.dll")
        for entry in catalog["captures"]:
            with self.subTest(capture=entry["id"]):
                for name, expected in entry["artifacts"].items():
                    self.assertEqual(digest(repository_file(name)), expected, name)
                    self.assertNotIn(Path(name).suffix.lower(), {".dll", ".exe", ".eng", ".wag", ".s", ".ace", ".sms", ".cvf"})
                provenance = json.loads(repository_file(entry["manifest"]).read_text())
                self.assertEqual(provenance["reference_version"], "1.6.1")
                self.assertEqual(provenance["reference_commit"], REFERENCE_COMMIT)
                binaries = provenance.get("sha256", provenance.get("binaries", provenance.get("assemblies", {})))
                self.assertEqual(binaries["Orts.Simulation.dll"], simulation)
                self.assertEqual(digest(repository_file(entry["helper"])), provenance["helper_sha256"])
                script_hash = provenance.get("script_sha256", provenance.get("capture_script_sha256"))
                self.assertEqual(digest(repository_file(entry["capture_script"])), script_hash)
                self.assertIn(entry["reference"], entry["artifacts"])
                self.assertIn(entry["manifest"], entry["artifacts"])
                self.assertIn(entry["tolerance_evidence"], entry["artifacts"])
                if "owned_fixture_artifacts" in entry:
                    self.assertEqual(entry["owned_fixture_artifacts"], provenance["fixture_sha256"])
                    for name, expected in entry["owned_fixture_artifacts"].items():
                        self.assertTrue(name.startswith("examples/vacuum_independent/vehicles/"))
                        fixture = repository_file(name)
                        self.assertIn(fixture.suffix, {".eng", ".wag"})
                        self.assertLess(fixture.stat().st_size, 16_384)
                        self.assertEqual(digest(fixture), expected, name)
                if "recapture_script" in entry:
                    self.assertIn(entry["recapture_script"], entry["artifacts"])
                    self.assertNotEqual(entry["capture_script"], entry["recapture_script"])
                if "isolation_evidence" in entry:
                    evidence = entry["isolation_evidence"]
                    isolated = json.loads(repository_file(evidence["isolation"]).read_text())
                    release = json.loads(repository_file(evidence["release"]).read_text())
                    self.assertTrue(isolated["registry_verified"])
                    self.assertFalse(isolated["original_prefix_touched"])
                    self.assertEqual(isolated["graphics_driver"], "null")
                    self.assertEqual(isolated["show_crash_dialog"], 0)
                    self.assertIsNone(isolated["environment"]["DISPLAY"])
                    self.assertIsNone(isolated["environment"]["XAUTHORITY"])
                    self.assertEqual(release["remaining_processes"], [])
                    self.assertEqual(release["forbidden_drivers_seen"], [])
                    self.assertGreater(release["observation_count"], 0)
                    self.assertEqual(digest(repository_file(evidence["release"])),
                                     provenance["wine_isolation_release_sha256"])
                    self.assertEqual(digest(repository_file(evidence["helper"])),
                                     provenance["wine_isolation_helper_sha256"])

    def test_previously_approved_reference_bytes_are_preserved(self):
        catalog = json.loads(CATALOG.read_text())
        pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
        known = {entry["path"]: entry["sha256"] for entry in pin["files"]}
        self.assertGreaterEqual(len(catalog["preserved_approved_baselines"]), 4)
        for name, expected in catalog["preserved_approved_baselines"].items():
            self.assertEqual(known[name], expected, name)
            self.assertEqual(digest(repository_file(name)), expected, name)

    def test_gearbox_inputs_outputs_and_prior_tolerances_match_registered_bytes(self):
        manifest = json.loads((GEARBOX / "manifest.json").read_text())
        registration = json.loads((GEARBOX / "registration.json").read_text())
        for name, expected in manifest["files"].items():
            self.assertEqual(digest(GEARBOX / name), expected, name)
        for key in ("helper_sha256", "script_sha256", "input_sha256", "tolerances_sha256", "drive_config_sha256"):
            self.assertEqual(manifest[key], registration[key])
        self.assertEqual(registration["phase"], "before_wine_capture_and_before_rust_comparison")
        tolerances = json.loads((GEARBOX / "tolerances.json").read_text())
        self.assertTrue(tolerances["declared_before_capture_and_rust_comparison"])
        self.assertEqual(tolerances["shaft_rpm_abs"], .02)
        self.assertEqual(tolerances["force_n_abs"], .1)
        self.assertEqual(tolerances["clutch_pct_abs"], .002)
        self.assertEqual(tolerances["state_and_gear"], "exact")
        inputs = json.loads((GEARBOX / "inputs.json").read_text())
        result = json.loads((GEARBOX / "reference.json").read_text())
        self.assertEqual(inputs["observer_order"], result["getter_order"])
        self.assertEqual({profile["id"] for profile in inputs["profiles"]},
                         {profile["id"] for profile in result["profiles"]})
        actual = {scenario["id"]: scenario for scenario in result["scenarios"]}
        self.assertEqual(len(actual), 5)
        for scenario in inputs["scenarios"]:
            captured = actual[scenario["id"]]
            self.assertEqual(captured["kind"], "prescribed_subsystem")
            self.assertEqual(captured["method"], scenario["method"])
            self.assertEqual(captured["operation"], scenario["operation"])
            self.assertEqual([row["tick"] for row in scenario["inputs"]],
                             [row["tick"] for row in captured["checkpoints"]])
            self.assertTrue(all(math.isfinite(float(row["tractive_force_n"])) for row in captured["checkpoints"]))
        self.assertEqual(sum(len(scenario["checkpoints"]) for scenario in actual.values()), 1293)

    def test_historical_gearbox_script_is_preserved_and_recapture_keeps_its_protocol(self):
        entry = next(entry for entry in json.loads(CATALOG.read_text())["captures"]
                     if entry["id"] == "diesel_gearbox_class101")
        manifest = json.loads((GEARBOX / "manifest.json").read_text())
        historical = repository_file(entry["capture_script"])
        current = repository_file(entry["recapture_script"])
        self.assertEqual(digest(historical), manifest["script_sha256"])
        self.assertEqual(digest(historical), "ff61a44135dee5fc8b9869ce762513237afeb68bcde701e34aff02d9304661c4")
        self.assertNotEqual(digest(current), digest(historical))
        # The transition changes process isolation only. Compare the input,
        # parameter and observation protocol definitions to the executed script,
        # without running Wine or replacing any native physical result.
        def protocol_definitions(path):
            definitions = {}
            for node in ast.parse(path.read_text()).body:
                if isinstance(node, ast.FunctionDef) and node.name in {
                        "vehicle_profile", "replace_field", "prescribed_input", "inputs",
                        "drive_config", "prepare_drive_facade"}:
                    definitions[node.name] = ast.dump(node, include_attributes=False)
                elif isinstance(node, ast.Assign) and any(isinstance(name, ast.Name) and
                        name.id in {"REFERENCES", "TOLERANCES"} for name in node.targets):
                    definitions[node.targets[0].id] = ast.dump(node.value, include_attributes=False)
            return definitions
        self.assertEqual(len(protocol_definitions(historical)), 8)
        self.assertEqual(protocol_definitions(historical), protocol_definitions(current))
        transition = entry["recapture_transition"]
        self.assertEqual(transition["current_execution_status"], "configured_not_executed")
        self.assertFalse(transition["historical_captures_claim_common_wrapper"])
        self.assertEqual(transition["isolation_helper"], "scripts/oracle_wine.py")
        preparation_path = repository_file(transition["preparation_evidence"])
        self.assertIn(transition["preparation_evidence"], entry["artifacts"])
        preparation = json.loads(preparation_path.read_text())
        self.assertEqual(preparation["script_sha256"], digest(current))
        self.assertEqual(preparation["wine_isolation_helper_sha256"],
                         digest(repository_file(transition["isolation_helper"])))
        for key in ("helper_sha256", "input_sha256", "tolerances_sha256",
                    "drive_config_sha256", "source_vehicle", "source_path", "phase"):
            self.assertEqual(preparation[key], manifest[key])
        source = ast.parse(current.read_text())
        self.assertTrue(any(isinstance(node, ast.With) and any(
            isinstance(item.context_expr, ast.Call) and
            isinstance(item.context_expr.func, ast.Name) and
            item.context_expr.func.id == "OracleWine" for item in node.items)
            for node in ast.walk(source)))
        self.assertFalse(any(isinstance(node, ast.Import) and any(
            alias.name == "subprocess" for alias in node.names) for node in ast.walk(source)))

    def test_gearbox_native_phases_are_observed_in_the_original_outputs(self):
        result = json.loads((GEARBOX / "reference.json").read_text())
        scenarios = {scenario["id"]: scenario["checkpoints"] for scenario in result["scenarios"]}
        manual = scenarios["manual_clutch_shift_and_neutral"]
        automatic = scenarios["automatic_up_down_and_neutral"]
        self.assertEqual({row["current_gear_index"] for row in manual}, {-1, 0, 1, 2, 3})
        self.assertEqual({row["current_gear_index"] for row in automatic}, {-1, 0, 1, 2, 3})
        self.assertTrue(any(row["current_gear_index"] != row["next_gear_index"] for row in manual))
        self.assertEqual(automatic[-1]["current_gear_index"], -1)
        grid = scenarios["manual_force_and_overspeed_grid"]
        self.assertTrue(any(row["overspeed_warning"] and not row["overspeed_error"] for row in grid))
        self.assertTrue(any(row["overspeed_error"] for row in grid))
        coast = scenarios["explicit_units_coasting_and_clutch_clamp"]
        self.assertTrue(any(row["tractive_force_n"] < 0 for row in coast))
        self.assertTrue(any(row["clutch_pct"] == -100 for row in coast))
        self.assertTrue(any(row["clutch_pct"] == 100 for row in coast))
        engine = scenarios["native_geared_engine_prescribed_speed"]
        self.assertTrue(any(row["flow_lps"] > .01 for row in engine))
        self.assertGreater(len({row["rpm"] for row in engine}), 100)

    def test_coupler_capture_contains_real_tension_compression_and_impulse_results(self):
        directory = ROOT / "docs/fixtures/compatibility/coupler-forces-or161"
        manifest = json.loads((directory / "manifest.json").read_text())
        reference = json.loads((directory / "reference.json").read_text())
        self.assertEqual(digest(directory / "reference.json"), manifest["output_sha256"])
        self.assertEqual(digest(directory / "input.json"), manifest["input_sha256"])
        self.assertEqual(digest(directory / "protocol.json"), manifest["protocol_sha256"])
        self.assertEqual(len(reference["profiles"]), 6)
        self.assertEqual(len(reference["forces"]), 6)
        self.assertEqual(len(reference["impacts"]), 4)
        forces = {row["id"]: row for row in reference["forces"]}
        self.assertEqual(forces["slack_start"]["tension_positive_force_n"], 0)
        self.assertGreater(forces["traction_tension"]["tension_positive_force_n"], 0)
        self.assertLess(forces["braking_compression"]["tension_positive_force_n"], 0)
        self.assertEqual(forces["wrong_contact_face"]["tension_positive_force_n"], 0)
        impacts = {row["id"]: row for row in reference["impacts"]}
        self.assertEqual(impacts["stationary_contact"]["native_impulse_u_ns"], 0)
        self.assertGreater(abs(impacts["high_energy_compression"]["native_impulse_u_ns"]),
                           abs(impacts["low_speed_compression"]["native_impulse_u_ns"]))
        profiles = {row["id"]: row for row in reference["profiles"]}
        self.assertTrue(profiles["advanced_metric"]["advanced"])
        self.assertTrue(profiles["rigid_advanced"]["rigid"])
        self.assertNotEqual(profiles["two_ends"]["simple_slack_m"],
                            profiles["two_ends_flipped"]["simple_slack_m"])

    def test_vacuum_capture_actually_moves_controllers_and_covers_declared_phases(self):
        config = json.loads((ROOT / "examples/vacuum_independent/reference-input.json").read_text())
        reference = json.loads((ROOT / "oracles/openrails-vacuum-independent.json").read_text())
        provenance = json.loads((ROOT / "oracles/openrails-vacuum-independent-provenance.json").read_text())
        self.assertEqual(digest(ROOT / "oracles/openrails-vacuum-independent.json"), provenance["output_sha256"])
        self.assertEqual(config["tolerances_declared_before_capture"],
                         {"pressure_psi_abs": .002, "shoe_force_n_abs": 50.})
        self.assertEqual({case["name"] for case in config["cases"]},
                         {case["name"] for case in reference["cases"]})
        self.assertEqual(len(reference["cases"]), 3)
        captured = {case["name"]: case for case in reference["cases"]}
        count = 0
        saw_independent_shoe_force = False
        saw_train_shoe_force = False
        for spec in config["cases"]:
            case = captured[spec["name"]]
            points = case["points"]
            train_has_applied = False
            self.assertEqual(len(case["initial"]), len(spec["vehicles"]))
            self.assertTrue(all(math.isfinite(float(vehicle[field]))
                                for vehicle in case["initial"]
                                for field in ("pipe_psi", "independent_pipe_psi", "cylinder_psi",
                                              "reservoir_psi", "brake_cyl_fraction")))
            self.assertEqual(len(points), sum(phase["ticks"] for phase in spec["phases"]))
            self.assertEqual({point["phase"] for point in points},
                             {phase["name"] for phase in spec["phases"]})
            self.assertGreater(max(point["independent_command"] for point in points), .5,
                               "SetValue without native controller.Update must not pass as applied control")
            self.assertEqual(min(point["independent_command"] for point in points), 0)
            for point in points:
                count += 1
                self.assertEqual(len(point["vehicles"]), len(spec["vehicles"]))
                self.assertTrue(all(math.isfinite(float(vehicle[field]))
                                    for vehicle in point["vehicles"]
                                    for field in ("pipe_psi", "independent_pipe_psi", "cylinder_psi",
                                                  "reservoir_psi", "adjusted_reservoir_psi", "shoe_force_n",
                                                  "pipe_input_psi", "brake_cyl_fraction")))
                self.assertTrue(all(0 <= vehicle["brake_cyl_fraction"] <= 1
                                    for vehicle in point["vehicles"]))
                if point["train_command"] == 0 and point["independent_command"] > .5 and not train_has_applied:
                    for vehicle, hardware in zip(point["vehicles"], spec["vehicles"]):
                        if hardware["role"] == "coach":
                            self.assertLessEqual(vehicle["shoe_force_n"], 50,
                                                 "Independent brake must not apply the coach")
                        else:
                            saw_independent_shoe_force |= vehicle["shoe_force_n"] > 50
                if point["train_command"] > .5:
                    saw_train_shoe_force |= any(vehicle["shoe_force_n"] > 50 for vehicle in point["vehicles"])
                train_has_applied |= point["train_command"] > 0
        self.assertEqual(count, 1440)
        self.assertTrue(saw_independent_shoe_force)
        self.assertTrue(saw_train_shoe_force)

    def test_class101_drive_is_a_complete_native_route_trace_with_control_inputs(self):
        initial = json.loads((GEARBOX / "native-drive/initial.json").read_text())
        outcome = json.loads((GEARBOX / "native-drive/outcome.json").read_text())
        config = json.loads((GEARBOX / "drive-config.json").read_text())
        tolerances = json.loads((GEARBOX / "tolerances.json").read_text())["native_drive"]
        with (GEARBOX / "native-drive/trace.csv").open() as stream:
            rows = list(csv.DictReader(stream))
        self.assertEqual(initial["kind"], "native_route_simulation")
        self.assertEqual(initial["method"], "Simulator.Update")
        self.assertEqual(outcome["method"], "Simulator.Update")
        self.assertEqual(outcome["control_mode"], "EXPLORER")
        self.assertEqual(outcome["out_of_control_reason"], "UNDEFINED")
        self.assertEqual(len(rows), config["ticks"] + 1)
        self.assertEqual({int(row["current_gear_index"]) for row in rows}, {-1, 0, 1, 2, 3})
        self.assertGreater(max(float(row["velocity_mps"]) for row in rows), 20)
        self.assertGreater(outcome["odometer_m"] - initial["initial_odometer_m"], 1000)
        self.assertLessEqual(abs(outcome["final_speed_mps"]), .01)
        self.assertLessEqual(abs(initial["initial_fuel_l"] - outcome["fuel_l"] - outcome["integrated_native_flow_l"]),
                             tolerances["fuel_l_abs"])
        previous = -1
        command = 0
        for index, row in enumerate(rows):
            tick = int(row["tick"])
            self.assertEqual(tick, index)
            self.assertAlmostEqual(float(row["time_s"]), tick * config["step_s"], places=7)
            self.assertGreater(float(row["time_s"]), previous)
            previous = float(row["time_s"])
            self.assertTrue(all(math.isfinite(float(value)) for value in row.values()))
            if index == 0:
                continue
            consumed_tick = tick - 1
            while command + 1 < len(config["commands"]) and config["commands"][command + 1]["tick"] <= consumed_tick:
                command += 1
            expected = config["commands"][command]
            self.assertEqual(float(row["throttle"]), expected["throttle"])
            self.assertEqual(float(row["brake"]), expected["brake"])
            self.assertEqual(int(row["command_gear"]), expected["gear"])
        self.assertNotEqual(int(rows[0]["node"]), int(rows[-1]["node"]))

    def test_independent_original_gearbox_captures_are_reproducible(self):
        proof = json.loads((GEARBOX / "reproducibility.json").read_text())
        first, second = [json.loads((GEARBOX / name).read_text()) for name in proof["captures"]]
        for key in ("input_sha256", "tolerances_sha256", "drive_config_sha256", "helper_sha256", "script_sha256"):
            self.assertEqual(first[key], second[key])
        self.assertEqual(first["sha256"], second["sha256"])
        for entry in proof["byte_identical_outputs"]:
            self.assertEqual(first["files"][entry["file"]], second["files"][entry["file"]])
            self.assertEqual(digest(GEARBOX / entry["file"]), entry["sha256"])
        initial = proof["initial_state_comparison"]
        first_state = json.loads((GEARBOX / initial["first"]).read_text())
        second_state = json.loads((GEARBOX / initial["second"]).read_text())
        self.assertEqual(initial["identical_after_omitting"], ["source_vehicle"])
        self.assertNotEqual(first_state.pop("source_vehicle"), second_state.pop("source_vehicle"))
        self.assertEqual(first_state, second_state)

    def test_archived_independent_rust_route_meets_the_prior_native_limits(self):
        directory = GEARBOX / "rust-drive"
        report = json.loads((directory / "comparison.json").read_text())
        receipt = json.loads((directory / "receipt.json").read_text())
        initial = json.loads((directory / "initial.json").read_text())
        limits = json.loads((GEARBOX / "tolerances.json").read_text())["native_drive"]
        self.assertEqual(limits, {"speed_mps_abs": .1, "distance_m_abs": 3,
                                  "fuel_l_abs": .05, "manual_shift_time_s_abs": .1})
        self.assertEqual(report["tolerances"], limits)
        self.assertEqual(report["status"], "passed")
        self.assertTrue(report["original_content_unmodified"])
        self.assertEqual(initial["kind"], "rust_closed_loop_route_simulation")
        self.assertFalse(initial["motion_or_force_prescribed"])
        self.assertTrue(receipt["only_controls_prescribed"])
        self.assertFalse(receipt["native_reference_or_limits_modified"])
        self.assertEqual(receipt["kind"], "rust_measurement_of_this_executable")
        self.assertEqual(receipt["source_snapshot"]["phase"], "after_successful_execution")
        self.assertTrue(receipt["git"]["working_tree_dirty"])
        self.assertTrue(receipt["git"]["source_status_porcelain"])
        self.assertEqual(len(receipt["binary"]["sha256"]), 64)
        self.assertIn("crates/openrailsrs-sim/examples/gearbox_native_drive.rs",
                      receipt["source_snapshot"]["files_sha256"])
        # This is historical execution evidence, not a claim that current source
        # or HEAD bytes equal those of a prior, explicitly dirty checkout.
        for name, expected in receipt["artifacts"].items():
            self.assertEqual(digest(directory / name), expected)
        self.assertEqual(hashlib.sha256(gzip.decompress((directory / "source-diff.patch.gz").read_bytes())).hexdigest(),
                         receipt["git"]["tracked_diff_sha256"])
        raw = gzip.decompress((directory / "trace.csv.gz").read_bytes())
        self.assertEqual(hashlib.sha256(raw).hexdigest(), report["rust_trace_sha256"])
        self.assertEqual(hashlib.sha256(raw).hexdigest(), receipt["trace"]["uncompressed_sha256"])
        self.assertEqual(digest(GEARBOX / "native-drive/trace.csv"), report["reference_trace_sha256"])
        self.assertEqual(digest(GEARBOX / "tolerances.json"), report["tolerances_sha256"])
        rust = list(csv.DictReader(io.StringIO(raw.decode())))
        with (GEARBOX / "native-drive/trace.csv").open() as stream:
            native = list(csv.DictReader(stream))
        self.assertEqual(len(rust), 6001)
        self.assertEqual(len(rust), len(native))
        self.assertEqual(report["rows"], len(rust))
        self.assertEqual(list(rust[0]), list(native[0]))
        offset = float(rust[0]["odometer_m"])
        for actual, expected in zip(rust, native):
            self.assertTrue(all(math.isfinite(float(value)) for value in actual.values()))
            for field in ("tick", "time_s", "throttle", "brake", "command_gear"):
                self.assertEqual(float(actual[field]), float(expected[field]), field)
        speed_error = max(abs(float(a["velocity_mps"]) - float(b["velocity_mps"]))
                          for a, b in zip(rust, native))
        distance_error = max(abs(float(a["odometer_m"]) - offset - float(b["odometer_m"]))
                             for a, b in zip(rust, native))
        rust_fuel = sum(float(row["flow_lps"]) * .05 for row in rust[1:])
        native_fuel = sum(float(row["flow_lps"]) * .05 for row in native[1:])
        self.assertAlmostEqual(speed_error, report["max_speed_error_mps"], places=9)
        self.assertAlmostEqual(distance_error, report["max_distance_error_m"], places=9)
        self.assertAlmostEqual(rust_fuel, report["rust_consumed_l"], places=9)
        self.assertAlmostEqual(native_fuel, report["native_integrated_flow_l"], places=9)
        self.assertLessEqual(speed_error, limits["speed_mps_abs"])
        self.assertLessEqual(distance_error, limits["distance_m_abs"])
        self.assertLessEqual(abs(rust_fuel - native_fuel), limits["fuel_l_abs"])
        def shifts(rows):
            previous = -1
            result = []
            for row in rows:
                gear = int(row["current_gear_index"])
                if gear != previous:
                    result.append((previous, gear, float(row["time_s"])))
                    previous = gear
            return result
        actual, expected = shifts(rust), shifts(native)
        self.assertEqual([(a, b) for a, b, _ in actual], [(a, b) for a, b, _ in expected])
        maximum_shift_error = max(abs(a[2] - b[2]) for a, b in zip(actual, expected))
        self.assertLessEqual(maximum_shift_error, limits["manual_shift_time_s_abs"])
        self.assertEqual(maximum_shift_error, report["max_shift_time_error_s"])


if __name__ == "__main__":
    unittest.main()
