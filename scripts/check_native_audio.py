#!/usr/bin/env python3
"""Render original consist sounds and check signal, clipping and missing WAVs.

WAVs and reports belong in a local output directory, not in the content package.
This checks decoding/mixing; it does not certify acoustic identity with OpenAL.
"""
import argparse
import array
import json
import math
from pathlib import Path
import re
import subprocess
import sys
import wave

ROOT = Path(__file__).resolve().parents[1]


def wav_metrics(path):
    with wave.open(str(path), "rb") as source:
        if source.getsampwidth() != 2:
            raise ValueError("Expected 16-bit PCM")
        duration = source.getnframes() / source.getframerate()
        samples = array.array("h", source.readframes(source.getnframes()))
    if sys.byteorder != "little":
        samples.byteswap()
    peak = max((abs(s) / 32768 for s in samples), default=0)
    return {
        "duration_s": duration,
        "rms": math.sqrt(sum((s / 32768) ** 2 for s in samples) / max(1, len(samples))),
        "peak_dbfs": 20 * math.log10(peak) if peak else None,
        "clipped_percent": 100 * sum(abs(s) >= 32767 for s in samples) / max(1, len(samples)),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--route-root", type=Path, required=True)
    parser.add_argument("--consist", type=Path, action="append", required=True)
    parser.add_argument("--oracle", type=Path, default=ROOT / "target/debug/examples/native_oracle")
    parser.add_argument("--allow-missing-wav", action="store_true",
                        help="Record missing WAVs without failing; all other warnings still fail")
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    oracle = args.oracle.resolve(strict=True)
    route = args.route_root.resolve(strict=True)
    consists = [c.resolve(strict=True) for c in args.consist]
    out = args.out_dir.resolve()
    content = route.parent.parent
    protected = [content, oracle, *(c.parent.parent for c in consists)]
    if any(out.is_relative_to(p) for p in protected):
        raise ValueError("Reports must stay outside the original content and executable")
    out.mkdir(parents=True, exist_ok=False)
    rows = []
    failed = False
    for i, consist in enumerate(consists):
        slug = re.sub(r"[^a-z0-9]+", "-", consist.stem.lower()).strip("-") or "formation"
        for view in ("exterior", "cab"):
            wav = out / f"{i + 1:02d}-{slug}-{view}.wav"
            report = json.loads(subprocess.check_output(
                [str(oracle), str(consist), str(route), str(wav), view], cwd=ROOT, text=True, timeout=120))
            report.update(wav_metrics(wav))
            missing = [w for w in report["warnings"] if w.startswith("Missing WAV:")]
            unsupported = [w for w in report["warnings"] if not w.startswith("Missing WAV:")]
            valid = bool(report["programs"] and report["samples"] and report["rms"] > 1e-5
                         and report["clipped_percent"] == 0 and report["duration_s"] >= 11.9
                         and not unsupported and (not missing or args.allow_missing_wav))
            failed |= not valid
            row = {"formation": consist.name, "view": view, "wav": str(wav),
                   "status": "pass_with_missing_wav" if valid and missing else "pass" if valid else "fail",
                   "missing_wav": missing, "report": report}
            rows.append(row)
            wav.with_suffix(".audio.json").write_text(json.dumps(row, indent=2) + "\n")
            (out / "report.json").write_text(json.dumps(rows, indent=2) + "\n")
            print(f"{consist.name} {view}: {row['status']}; RMS={report['rms']:.4f}, "
                  f"peak={report['peak_dbfs']}, missing WAV={len(missing)}", flush=True)
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())
