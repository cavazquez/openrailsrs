#!/usr/bin/env python3
"""Validate recorded drive/PCM evidence; optionally rehash original assets/PCM.

This checks an existing measurement, not a new native capture or listening test.
Run the Rust example to produce another measurement in a fresh directory.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path


def sha(data):
    return hashlib.sha256(data).hexdigest()


def load(path):
    return json.loads(path.read_bytes())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--originals", action="store_true", help="Rehash the original assets at their recorded absolute paths")
    parser.add_argument("--pcm-directory", type=Path, help="Rehash and inspect generated PCM files from the real run")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    fixture = repo / "docs/fixtures/compatibility/track-sound-drive"
    evidence = load(fixture / "evidence.json")
    for name, expected in evidence["artifacts_sha256"].items():
        assert sha((fixture / name).read_bytes()) == expected, name
    assert evidence["status"] == "PASS"
    native = (fixture / evidence["native_reference"]).read_bytes()
    assert sha(native) == evidence["native_reference_sha256"]
    assert len(json.loads(native)["rows"]) == 100
    receipt = load(fixture / "receipt.json")
    assert receipt["status"] == "PASS"
    assert receipt["source_example_sha256_observed_before_run"] == receipt["source_example_sha256_observed_after_run"]
    assert receipt["tested_executable_sha256"] == evidence["tested_executable_sha256"]
    assert receipt["protocol_sha256"] == sha((fixture / "protocol.json").read_bytes())
    assert receipt["native_reference_sha256"] == evidence["native_reference_sha256"]
    assert receipt["audio_device_opened"] is False and receipt["gpu_or_wine_used"] is False
    assert receipt["phase_completed"] and receipt["save_restore_during_split"]
    assert receipt["paused_zero_delta_checks"] == 20
    assert receipt["pause_pcm"]["silent"] and receipt["audio_selection_passed"]
    assert receipt["vehicle_count"] == 7
    for counts in receipt["native_interval_checks_by_direction_type_per_car"]:
        assert all(counts[f"{d}:{t}"] > 0 for d in ["forward", "reverse"] for t in [0, 3, 7])
    assert len(receipt["native_interval_checks_by_direction_type_per_car"]) == 7
    raw_trace = gzip.decompress((fixture / "drive.json.gz").read_bytes())
    assert sha(raw_trace) == receipt["drive_trace_sha256"] == evidence["drive_uncompressed_sha256"]
    trace = json.loads(raw_trace)
    assert len(trace) == receipt["ticks"] and {r["phase"] for r in trace} == {0, 1, 2, 3}
    assert any(len(set(r["effective_types"])) > 1 and r["speed_mps"] > 0.5 for r in trace)
    for phase, increasing in [(0, True), (2, False)]:
        rows = [r for r in trace if r["phase"] == phase]
        assert (rows[-1]["head_m"] > rows[0]["head_m"]) == increasing
    assert receipt["assets"]["selected_exterior_rolling_missing"] == 0
    assert len(receipt["assets"]["missing"]) == receipt["assets"]["missing_unique_count"]
    for contexts in receipt["assets"]["missing_contexts"].values():
        assert all("category=selected_region_exterior" not in c for c in contexts)
    coverage, regional_voices, stock_rolling = set(), 0, 0
    assert len(receipt["clips"]) == 6
    for clip in receipt["clips"]:
        assert clip["report"]["device"] is False
        assert clip["pcm"]["nonzero_samples"] > 0
        assert len(clip["selection_checks"]) == 7
        for check in clip["selection_checks"]:
            assert check["passed"] and check["resolved_active_track_voices"]
            assert check["active_track_programs"] == [check["expected_program"]]
            regional_voices += check["active_track_region_voice_count"]
            stock_rolling += check["active_stock_rolling_voice_count"]
            for voice in check["resolved_active_track_voices"]:
                assert voice["bank_track_type"] == check["track_type"]
                assert voice["group"] == "rolling" and voice["gain"] > 0.0001
                assert voice["resolved_original_rolling_voice"] and voice["wav"]["missing"] is False
                assert len(voice["wav"]["sha256"]) == 64
                coverage.add((check["direction"], check["track_type"]))
    assert coverage == {(d, t) for d in ["forward", "reverse"] for t in [0, 3, 7]}
    if args.originals:
        for asset in receipt["assets"]["files"]:
            assert sha(Path(asset["path"]).read_bytes()) == asset["sha256"], asset["path"]
    if args.pcm_directory:
        for name, stats in evidence["pcm_files"].items():
            data = (args.pcm_directory / name).read_bytes()
            assert sha(data) == stats["sha256"] and len(data) == stats["bytes"], name
            assert data[:4] == b"RIFF" and data[8:12] == b"WAVE", name
        pause = (args.pcm_directory / "pause-resume.wav").read_bytes()
        assert all(x == 0 for x in pause[44 + int(0.6 * 44100) * 4:44 + int(0.95 * 44100) * 4])
    first = gzip.decompress((fixture / "attempt01/receipt.json.gz").read_bytes())
    summary = load(fixture / "attempt01/summary.json")
    assert sha(first) == summary["original_receipt_sha256"]
    failed = json.loads(first)
    assert failed["status"] == "FAIL" and not failed["audio_selection_passed"]
    assert len(summary["failed_checks"]) == 3
    print(json.dumps({"status": "PASS", "scope": "recorded evidence consistency", "ticks": len(trace),
                      "regional_voice_checks": regional_voices, "stock_rolling_voices_separate": stock_rolling,
                      "original_assets_rehashed": args.originals, "pcm_rehashed": bool(args.pcm_directory)}))


if __name__ == "__main__":
    main()
