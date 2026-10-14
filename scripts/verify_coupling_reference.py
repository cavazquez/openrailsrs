#!/usr/bin/env python3
"""Verify real OR joining and a captured Rust state/equilibrium comparison.

OR's rigid/simple coupler and Rust's elastic coupler are compared at prescribed
static load balance. Their transient curves and moving impacts are outside this
acceptance. Conserved quantities are checked within each engine's transfer;
this does not disguise differences in their initial operating state as parity.
"""

import argparse
import json
import math
from pathlib import Path


def equal(left, right, message):
    if left != right:
        raise ValueError(message)


def validate_reference(reference):
    equal(reference["reference"], "Open Rails 1.6.1", "Wrong original version")
    coupling = reference["coupling"]
    equal(coupling["split_method"], "Simulator.UncoupleBehind(TrainCar, true)", "Wrong split method")
    equal(coupling["contact_method"], "Simulator.CheckForCoupling(Train, 0.05f)", "Wrong contact method")
    if not -1 <= coupling["signed_overlap_m"] < 0:
        raise ValueError("OR contact does not overlap within its one-metre gate")
    for stage in ("separated", "stationary_scan", "before"):
        equal(len(coupling[stage]["active_trains"]), 2, "OR coupled without its moving contact trigger")
    equal(coupling["stationary_scan"]["player_speed_mps"], 0, "Stationary OR scan was not stationary")
    before = coupling["before"]["vehicles"] + coupling["before"]["donor_vehicles"]
    after = coupling["after"]["vehicles"]
    equal(len(before), 8, "OR input is not two four-car formations")
    equal([row["id"] for row in after], coupling["original_order"], "OR lost vehicle identity/order")
    equal(len({row["id"] for row in after}), 8, "OR active vehicles are duplicated")
    equal(len(coupling["after"]["active_trains"]), 1, "OR donor remains active")
    equal(coupling["after"]["donor_vehicles"], [], "OR donor remains physically active")
    for a, b in zip(before, after, strict=True):
        for key in ("id", "file", "mass_kg", "flipped", "fuel_l", "pipe_psi",
                    "cylinder_psi", "auxiliary_psi", "shoe_force_n", "handbrake"):
            equal(a[key], b[key], "OR failed to preserve " + key)
        equal(b["owner"], coupling["after"]["player_number"], "OR car retained donor ownership")
    donor = coupling["after"]["donor_number"]
    if any(row["donor_occupied"] or row["reserved_by"] == donor for row in coupling["after"]["track"]):
        raise ValueError("OR removed donor retains occupation/reservations")
    equal(coupling["every_original_object_transferred_once"], True, "OR object-transfer assertion absent")
    equal(coupling["removed_donor_has_no_occupation_or_reservations"], True, "OR track assertion absent")
    equal([row["overlap_m"] < 0 for row in reference["overlap"]],
          [True, False, False, False, False], "Original overlap gates changed")
    equal(reference["momentum"][0]["resulting_a_mps"], 0, "Stationary momentum changed")
    equal(reference["momentum"][0]["resulting_b_mps"], 0, "Stationary donor momentum changed")


def validate_comparison(reference, rust):
    validate_reference(reference)
    before = rust["before_player"]["vehicles"] + rust["before_donor"]["vehicles"]
    after = rust["after"]["vehicles"]
    equal(len(after), 8, "Rust did not transfer eight vehicles")
    for a, b in zip(before, after, strict=True):
        # Closing the new joint and leaving its hose/bus disconnected is an
        # explicit preparation step, not a lost reservoir/vehicle state.
        for key in ("name", "length_m", "mass_kg", "powered", "power_on", "battery_on",
                    "mu_connected", "handbrake", "train_supply_switch_on"):
            equal(a[key], b[key], "Rust failed to retain car " + key)
    equal(after[3]["rear_cock_open"], False, "Rust new rear cock was not closed")
    equal(after[4]["front_cock_open"], False, "Rust new front cock was not closed")
    equal(after[4]["hose_connected"], False, "Rust connected the new hose silently")
    cylinders = rust["before_player"]["cylinders"] + rust["before_donor"]["cylinders"]
    for a, b in zip(cylinders, rust["after"]["cylinders"], strict=True):
        for key in ("current_force_n", "independent_force_n", "automatic_pipe_bar",
                    "native_air", "native_ep", "native_vacuum", "legacy_ep"):
            equal(a.get(key), b.get(key), "Rust failed to preserve brake " + key)
    before_fuel = rust["before_player"]["fuel"] + rust["before_donor"]["fuel"]
    for a, b in zip(before_fuel, rust["after"]["fuel"], strict=True):
        for key, value in a.items():
            if key != "vehicle":
                equal(value, b[key], "Rust reset a fuel/equipment state")
    expected_mass = rust["before_player"]["effective_mass_kg"] + rust["before_donor"]["effective_mass_kg"]
    if abs(expected_mass - rust["after"]["effective_mass_kg"]) > 1e-6:
        raise ValueError("Rust did not conserve total effective mass")
    equal(rust["removed_donor"], True, "Rust donor was not removed")
    equal(rust["restored_without_duplicates"], True, "Rust reconstruction assertion absent")
    equal(rust["rejected_unsecured_donor"], True, "Rust safety assertion absent")
    equal(rust["rejected_traction_during_contact"], True, "Rust traction gate assertion absent")
    for key in ("vehicles", "cylinders", "fuel", "effective_mass_kg", "speed_mps", "head_m"):
        equal(rust["after"][key], rust["restored_before_drive"][key], "Rust restore lost " + key)
    if rust["moving_after_restore"]["distance_m"] <= 1 or rust["moving_after_restore"]["vehicles"] != 8:
        raise ValueError("Joined original train did not drive after restoration")
    equal(rust["moving_after_restore"]["time_s"], 20, "Driving horizon changed")
    equal(len(rust["force_equilibrium"]), 3, "Missing representative equilibrium cases")
    for a, b in zip(reference["coupling"]["equilibrium_forces"], rust["force_equilibrium"], strict=True):
        equal(a["id"], b["id"], "Equilibrium cases do not match")
        equal(a["masses_kg"], b["masses_kg"], "Equilibrium masses do not match")
        equal(a["input_force_n"], b["input_force_n"], "Equilibrium loads do not match")
        error = abs(a["coupler_force_or_n"] + b["coupler_force_rust_n"])
        if not math.isfinite(error) or error > 0.1:
            raise ValueError("Coupler equilibrium force outside 0.1 N tolerance")
        for original, actual in zip(a["resulting_acceleration_mps2"], b["acceleration_mps2"], strict=True):
            if not math.isfinite(actual) or abs(original - actual) > 1e-5:
                raise ValueError("Equilibrium acceleration outside 1e-5 m/s² tolerance")
    return {"status": "pass", "reference": "Open Rails 1.6.1", "transferred_original_vehicles": 8,
            "force_cases": 3, "maximum_force_error_n": max(row["maximum_force_error_n"] for row in rust["force_equilibrium"]),
            "driving_after_restore_m": rust["moving_after_restore"]["distance_m"],
            "limits": ["Stationary manual rear-to-front joining only in Rust",
                       "OR automatically requires signed movement; zero-speed scan is retained separately",
                       "Different simple rigid / elastic transient and slack laws are not declared equivalent",
                       "Original stock is driven on owned synthetic track; native route/signal/full-physics parity not certified"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = validate_comparison(json.loads(args.reference.read_text()), json.loads(args.rust.read_text()))
    text = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.write_text(text)
    print(text, end="")


if __name__ == "__main__":
    main()
