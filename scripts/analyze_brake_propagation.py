#!/usr/bin/env python3
"""Analyze brake pipe propagation from openrailsrs run.csv (OR-P4/P6 / A1).

Reads brake_f_head_n, brake_f_train_air_n (first wagon), brake_f_tail_n and checks
that the lead EP loco applies before the first train-air cylinder during service brake.

Pullman consists have EP on both motor cars; propagation is visible on train-air wagons,
not head vs tail.

Usage:
  ./scripts/analyze_brake_propagation.py examples/chiltern/run_brake_coast.csv
  ./scripts/analyze_brake_propagation.py run.csv --apply-start 100 --apply-end 115 \\
      --train-air-position-m 20 --pipe-speed 200

Exit 0 on PASS, 1 on FAIL.
"""

from __future__ import annotations

import argparse
import csv
import sys
from dataclasses import dataclass
from pathlib import Path


@dataclass
class Sample:
    time_s: float
    brake_cmd: float
    head_n: float
    train_air_n: float
    tail_n: float


def load_samples(path: Path) -> list[Sample]:
    with path.open(newline="") as f:
        reader = csv.DictReader(f)
        fields = reader.fieldnames or []
        required = ("time_s", "brake_f_head_n", "brake_f_train_air_n", "brake_f_tail_n")
        missing = [c for c in required if c not in fields]
        if missing:
            raise SystemExit(
                f"{path}: missing columns {missing} — re-run sim with brake cylinder telemetry"
            )
        rows: list[Sample] = []
        for row in reader:
            try:
                rows.append(
                    Sample(
                        time_s=float(row["time_s"]),
                        brake_cmd=float(row.get("brake") or 0.0),
                        head_n=float(row["brake_f_head_n"]),
                        train_air_n=float(row["brake_f_train_air_n"]),
                        tail_n=float(row["brake_f_tail_n"]),
                    )
                )
            except (TypeError, ValueError):
                continue
        if not rows:
            raise SystemExit(f"{path}: no numeric rows")
        return rows


def first_time_force(samples: list[Sample], attr: str, threshold: float, t_min: float, t_max: float) -> float | None:
    for s in samples:
        if not (t_min <= s.time_s <= t_max):
            continue
        force = getattr(s, attr)
        if force >= threshold:
            return s.time_s
    return None


def max_head_minus_train_air(samples: list[Sample], t_min: float, t_max: float) -> tuple[float, float]:
    best = 0.0
    best_t = t_min
    for s in samples:
        if not (t_min <= s.time_s <= t_max):
            continue
        gap = s.head_n - s.train_air_n
        if gap > best:
            best = gap
            best_t = s.time_s
    return best_t, best


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_csv", type=Path, help="openrailsrs run.csv with brake telemetry")
    parser.add_argument(
        "--apply-start",
        type=float,
        default=100.0,
        help="Start of service-brake window (s)",
    )
    parser.add_argument(
        "--apply-end",
        type=float,
        default=115.0,
        help="End of service-brake window (s)",
    )
    parser.add_argument(
        "--force-threshold-n",
        type=float,
        default=5_000.0,
        help="Minimum force (N) to count as 'applying'",
    )
    parser.add_argument(
        "--train-air-position-m",
        type=float,
        default=20.0,
        help="Pipe distance to first train-air cylinder (m); Pullman ~one coach length",
    )
    parser.add_argument(
        "--pipe-speed",
        type=float,
        default=200.0,
        help="Modeled brake-pipe speed (m/s)",
    )
    parser.add_argument(
        "--delay-tolerance-s",
        type=float,
        default=0.35,
        help="Allowed deviation from expected pipe travel time (s)",
    )
    parser.add_argument(
        "--min-head-lead-n",
        type=float,
        default=1_000.0,
        help="Minimum head − train_air force gap (N) during apply",
    )
    args = parser.parse_args()

    samples = load_samples(args.run_csv)
    t0, t1 = args.apply_start, args.apply_end
    expected_delay = args.train_air_position_m / args.pipe_speed

    head_t = first_time_force(samples, "head_n", args.force_threshold_n, t0, t1)
    air_t = first_time_force(samples, "train_air_n", args.force_threshold_n, t0, t1)
    tail_t = first_time_force(samples, "tail_n", args.force_threshold_n, t0, t1)
    gap_t, max_gap = max_head_minus_train_air(samples, t0, t1)

    print(f"=== Brake propagation: {args.run_csv} ===")
    print(f"Apply window: {t0:.0f}–{t1:.0f} s")
    print(f"Expected train-air delay: {expected_delay:.3f} s ({args.train_air_position_m:.0f} m @ {args.pipe_speed:.0f} m/s)")
    print()

    ok = True

    if head_t is None:
        print(f"FAIL: head force never exceeded {args.force_threshold_n:.0f} N in window")
        ok = False
    else:
        print(f"  head EP > threshold:      t = {head_t:.1f} s")

    if air_t is None:
        print(f"FAIL: train-air force never exceeded {args.force_threshold_n:.0f} N in window")
        ok = False
    else:
        print(f"  first train-air wagon:    t = {air_t:.1f} s")

    if tail_t is not None:
        print(f"  tail EP (instant):        t = {tail_t:.1f} s  (both locos are EP on Pullman)")

    if head_t is not None and air_t is not None:
        delay = air_t - head_t
        print(f"  measured head → train-air delay: {delay:.3f} s")
        if delay < -0.05:
            print("FAIL: train-air applied before head (unexpected)")
            ok = False
        elif delay == 0.0:
            print(
                "  note: both crossed threshold same log row (time_step ≥ delay); "
                "use max head−train_air gap below"
            )
        elif abs(delay - expected_delay) > args.delay_tolerance_s:
            print(
                f"WARN: delay outside ±{args.delay_tolerance_s:.2f} s of pipe model "
                f"(got {delay:.3f} s, expected ~{expected_delay:.3f} s)"
            )
            if delay < expected_delay - args.delay_tolerance_s:
                ok = False
        else:
            print("  delay vs pipe model: OK")

    if max_gap < args.min_head_lead_n:
        print(
            f"FAIL: max(head − train_air) = {max_gap:.0f} N at t={gap_t:.1f} s "
            f"(need ≥ {args.min_head_lead_n:.0f} N)"
        )
        ok = False
    else:
        print(f"  max head lead over train-air: {max_gap:.0f} N at t={gap_t:.1f} s")

    print()
    print("overall:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
