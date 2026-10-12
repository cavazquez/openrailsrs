#!/usr/bin/env python3
"""Compare fixed-machine viewer captures; incompatible contracts are rejected.

A runner supplies `contract` (machine/resolution/route_sha256/consist_sha256/
camera/seed/quality), `binary_sha256`, `version`, and the original `capture`.
Startup is reported separately and never used as the steady driving baseline.
"""
import argparse
import json
import math
from pathlib import Path

CONTRACT_FIELDS = ("machine", "resolution", "route_sha256", "consist_sha256", "camera", "seed", "quality")
METRICS = ("gameplay_frame_p50_ms", "gameplay_frame_p95_ms", "gameplay_frame_p99_ms", "gameplay_hitches_over_100_ms", "rss_mib", "peak_rss_mib")


def validate(report):
    if set(CONTRACT_FIELDS) - report.get("contract", {}).keys():
        raise ValueError("Incomplete benchmark contract")
    if not isinstance(report.get("version"), str) or not report["version"]:
        raise ValueError("Missing binary version")
    for name in ("binary_sha256",):
        value = report.get(name, "")
        if len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
            raise ValueError(f"Invalid {name}")
    for name in ("route_sha256", "consist_sha256"):
        value = report["contract"][name]
        if len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
            raise ValueError(f"Invalid {name}")
    capture = report.get("capture", {})
    performance = capture.get("performance", {})
    if performance.get("gameplay_frames", 0) < 300:
        raise ValueError("At least 300 gameplay frames are required")
    for name in METRICS:
        value = performance.get(name)
        if value is not None and (not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0):
            raise ValueError(f"Invalid metric {name}")
    for name in METRICS[:4]:
        if performance.get(name) is None:
            raise ValueError(f"Missing gameplay metric {name}")
    return performance


def compare(before, after, percent=15.0):
    old, new = validate(before), validate(after)
    mismatch = [name for name in CONTRACT_FIELDS if before["contract"][name] != after["contract"][name]]
    if mismatch:
        raise ValueError("Different benchmark conditions: " + ", ".join(mismatch))
    changes, regressions = {}, []
    for name in METRICS:
        a, b = old.get(name), new.get(name)
        if a is None or b is None:
            changes[name] = {"before": a, "after": b, "status": "unavailable"}
            continue
        delta = b - a
        allowance = max(2, a * percent / 100) if "frame" in name else max(3 if "hitches" in name else 96, a * percent / 100)
        status = "regression" if delta > allowance else "within_budget"
        changes[name] = {"before": a, "after": b, "delta": delta, "allowed_delta": allowance, "status": status}
        if status == "regression":
            regressions.append(name)
    # VRAM may be unavailable on this driver. Never turn absent measurements into zero.
    a = before.get("vram_mib")
    b = after.get("vram_mib")
    status = "unavailable" if a is None or b is None else ("regression" if b > a + max(96, a * percent / 100) else "within_budget")
    changes["vram_mib"] = {"before": a, "after": b, "status": status}
    if status == "regression":
        regressions.append("vram_mib")
    return {"schema": 1, "contract": before["contract"], "baseline_binary": before["binary_sha256"], "current_binary": after["binary_sha256"], "baseline_version": before["version"], "current_version": after["version"], "metrics": changes, "regressions": regressions, "passed": not regressions,
            "startup": {"before_longest_ms": old.get("startup_longest_frame_ms"), "after_longest_ms": new.get("startup_longest_frame_ms"), "before_loading_frames": old.get("loading_frames"), "after_loading_frames": new.get("loading_frames")}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--current", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--max-percent", type=float, default=15)
    args = parser.parse_args()
    if not 0 <= args.max_percent <= 100:
        parser.error("max-percent must be in 0..100")
    try:
        result = compare(json.loads(args.baseline.read_text()), json.loads(args.current.read_text()), args.max_percent)
    except (ValueError, KeyError, TypeError) as error:
        parser.error(str(error))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"passed": result["passed"], "regressions": result["regressions"]}))
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
