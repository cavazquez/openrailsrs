#!/usr/bin/env python3
"""Run the seven libFuzzer targets sequentially, retaining corpus and evidence.

Only the Rust harnesses' synthetic inputs are used. Compilation is outside the
time budget. Completed slices survive interruption; an interrupted slice is run
again on resume. Findings are evidence, not automatically distinct confirmed bugs.
"""
import argparse
from contextlib import contextmanager
import datetime as dt
import fcntl
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
TARGETS = ("stf", "simisa", "textures", "shapes", "terrain", "sigscript", "multiplayer")
MAX_INPUT = 65536


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".new")
    temporary.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")
    temporary.replace(path)


def source_snapshot():
    """Bind the harness and production reader sources, including untracked edits."""
    files = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", Path(__file__).resolve()]
    for directory in (ROOT / "crates", ROOT / "fuzz"):
        for base, directories, names in os.walk(directory):
            directories[:] = [d for d in directories if d not in
                              ("target", "corpus", "artifacts", "findings", "reports", ".git")]
            files.extend(Path(base) / name for name in names
                         if name.endswith((".rs", ".toml", ".lock", ".dict")))
    return {str(p.relative_to(ROOT)): sha(p) for p in sorted(set(files)) if p.is_file()}


def inventory(directory):
    entries = {}
    directory.mkdir(parents=True, exist_ok=True)
    for path in sorted(directory.iterdir()):
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"Corpus entries must be regular files: {path}")
        if path.stat().st_size > MAX_INPUT:
            raise ValueError(f"Corpus input exceeds {MAX_INPUT} bytes: {path}")
        entries[path.name] = {"sha256": sha(path), "bytes": path.stat().st_size}
    return {"files": entries, "count": len(entries),
            "bytes": sum(e["bytes"] for e in entries.values()),
            "unique_inputs": len({e["sha256"] for e in entries.values()})}


def parse_stats(text):
    result = {"executions": 0, "coverage_edges": None, "features": None,
              "reported_seconds": 0, "peak_rss_mb": None}
    for line in text.splitlines():
        if re.match(r"^#\d+\s", line):
            result["executions"] = max(result["executions"], int(re.match(r"#(\d+)", line)[1]))
            for label, key in (("cov", "coverage_edges"), ("ft", "features")):
                value = re.search(rf"\b{label}:\s*(\d+)", line)
                if value:
                    result[key] = int(value[1])
        done = re.search(r"Done (\d+) runs in (\d+) second", line)
        if done:
            result["executions"] = max(result["executions"], int(done[1]))
            result["reported_seconds"] = int(done[2])
        for label, key in (("number_of_executed_units", "executions"),
                           ("peak_rss_mb", "peak_rss_mb")):
            value = re.search(rf"stat::{label}:\s*(\d+)", line)
            if value:
                result[key] = int(value[1])
    return result


@contextmanager
def campaign_lock(corpus):
    corpus.mkdir(parents=True, exist_ok=True)
    with (corpus / ".campaign.lock").open("a+") as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise ValueError("Another campaign is using this corpus") from error
        try:
            yield
        finally:
            fcntl.flock(stream, fcntl.LOCK_UN)


def terminate_process(process):
    """Kill this owned process group only, including any remaining descendants."""
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def execute(command, log, timeout, heartbeat=None, cwd=ROOT, stream_summary=None):
    started = time.monotonic()
    summary = stream_summary if stream_summary is not None else {}
    summary["stats"] = parse_stats("")
    errors = []
    output = gzip.open(log, "wb", compresslevel=1) if log.suffix == ".gz" else log.open("wb")
    with output:
        process = subprocess.Popen(command, cwd=cwd, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        # Some production readers emit an expected diagnostic for every rejected
        # mutation. Stream-compress all bytes instead of filling the disk with
        # duplicate warnings; keep live numeric metrics separately.
        def pump():
            digest = hashlib.sha256()
            size = 0
            partial = b""
            try:
                while data := process.stdout.read1(65536):
                    output.write(data)
                    digest.update(data)
                    size += len(data)
                    lines = (partial + data).split(b"\n")
                    partial = lines.pop()[-65536:]
                    for line in lines:
                        if line.startswith((b"#", b"Done ", b"stat::")):
                            update = parse_stats(line.decode("ascii", errors="replace"))
                            for key, value in update.items():
                                if value is not None and (value != 0 or summary["stats"][key] is None):
                                    summary["stats"][key] = value
                summary.update(uncompressed_log_sha256=digest.hexdigest(), uncompressed_log_bytes=size)
            except Exception as error:
                errors.append(error)
        reader = threading.Thread(target=pump, daemon=True)
        reader.start()
        try:
            while True:
                try:
                    code = process.wait(timeout=min(10, max(.05, timeout - (time.monotonic() - started))))
                    reader.join(timeout=5)
                    if reader.is_alive() or errors:
                        raise OSError(f"Log capture failed: {errors}")
                    return code, time.monotonic() - started, False
                except subprocess.TimeoutExpired:
                    elapsed = time.monotonic() - started
                    if heartbeat:
                        heartbeat(elapsed)
                    if errors:
                        raise OSError(f"Log capture failed: {errors}")
                    if elapsed >= timeout:
                        terminate_process(process)
                        reader.join(timeout=5)
                        return process.returncode, time.monotonic() - started, True
        except BaseException:
            terminate_process(process)
            reader.join(timeout=5)
            raise
        finally:
            process.stdout.close()


def save_findings(target, artifact_dir, findings_dir, occurrence, binary, limits):
    results = []
    for path in sorted(artifact_dir.iterdir()):
        if not path.is_file() or path.is_symlink():
            raise ValueError(f"Unexpected artifact entry: {path}")
        digest = sha(path)
        kind = next((k for k in ("slow-unit", "timeout", "oom", "crash", "leak")
                     if path.name.startswith(k + "-")), "artifact")
        destination = findings_dir / target / (kind + "-" + digest)
        destination.mkdir(parents=True, exist_ok=True)
        saved = destination / "input.bin"
        if saved.exists() and sha(saved) != digest:
            raise ValueError(f"Stored finding hash mismatch: {saved}")
        if not saved.exists():
            saved.write_bytes(path.read_bytes())
        metadata_path = destination / "finding.json"
        metadata = json.loads(metadata_path.read_text()) if metadata_path.exists() else {
            "target": target, "kind": kind, "sha256": digest,
            "bytes": path.stat().st_size, "confirmed_bug": False, "occurrences": []}
        item = dict(occurrence, original_artifact=str(path.resolve()))
        if item not in metadata["occurrences"]:
            metadata["occurrences"].append(item)
        common = [f"-timeout={limits['timeout']}", f"-rss_limit_mb={limits['rss_limit_mb']}"]
        metadata["replay_command"] = [str(binary), str(saved.resolve()), "-runs=1", *common]
        if kind in ("crash", "leak"):
            metadata["minimize_command"] = [str(binary), str(saved.resolve()),
                "-minimize_crash=1", "-max_total_time=60",
                f"-exact_artifact_path={destination.resolve() / 'minimized.bin'}", *common]
        write_json(metadata_path, metadata)
        results.append({"kind": kind, "sha256": digest, "evidence": str(metadata_path.resolve())})
    return results


def campaign(config, binaries, sources):
    out, corpus, findings_dir = (Path(config[k]) for k in ("out", "corpus", "findings"))
    configuration = {k: v for k, v in config.items() if k not in ("out", "resume", "max_slices")}
    binary_hashes = {target: sha(binaries[target]) for target in config["targets"]}
    state_file = out / "progress.json"
    if config["resume"]:
        state = json.loads(state_file.read_text())
        if (state["configuration"] != configuration or state["source_inputs"] != sources or
                state["binary_sha256"] != binary_hashes):
            raise ValueError("Configuration, sources or binaries changed; use a fresh --out (corpus is retained)")
        if state["status"] in ("complete", "complete_with_findings"):
            return state
    else:
        out.mkdir(parents=True, exist_ok=False)
        state = {"schema": 1, "status": "running", "started_utc": now(),
                 "configuration": configuration, "source_inputs": sources,
                 "binary_sha256": binary_hashes, "sequential_workers": 1,
                 "scope": "Coverage-guided synthetic Rust inputs; not an absence-of-bugs guarantee",
                 "targets": {t: {"status": "pending", "completed_budget_seconds": 0,
                                 "attempts": []} for t in config["targets"]}}

    def persist():
        state["updated_utc"] = now()
        write_json(state_file, state)

    def event(kind, **values):
        with (out / "events.jsonl").open("a") as stream:
            stream.write(json.dumps(dict(utc=now(), event=kind, **values)) + "\n")

    state["status"] = "running"
    persist()
    current = None
    attempt = None
    slices_this_process = 0
    try:
        for target in config["targets"]:
            current = state["targets"][target]
            if current["status"] in ("pass", "finding", "infrastructure_failure"):
                continue
            target_corpus = corpus / target
            if "corpus_before" not in current:
                current["corpus_before"] = inventory(target_corpus)
            if not current["corpus_before"]["count"]:
                raise ValueError(f"No owned seeds for {target}; export them before the campaign")
            # An interrupted or killed slice never counts as a completed time budget.
            for old in current["attempts"]:
                if old["status"] == "running":
                    old["status"] = "interrupted"
            current["status"] = "running"
            while current["completed_budget_seconds"] < config["seconds_per_target"]:
                seconds = min(config["slice_seconds"], config["seconds_per_target"] -
                              current["completed_budget_seconds"])
                index = len(current["attempts"]) + 1
                directory = out / target / f"attempt-{index:03}"
                artifact_dir = directory / "artifacts"
                artifact_dir.mkdir(parents=True)
                seed = (config["seed"] + TARGETS.index(target) * 100003 + index * 7919) % (2 ** 32 - 1) or 1
                command = [str(binaries[target]), str(target_corpus.resolve()),
                           f"-artifact_prefix={artifact_dir.resolve()}/", f"-seed={seed}",
                           f"-max_total_time={seconds}", f"-timeout={config['timeout']}",
                           f"-rss_limit_mb={config['rss_limit_mb']}", f"-max_len={MAX_INPUT}",
                           "-fork=0", "-jobs=0", "-workers=1", "-print_final_stats=1", "-report_slow_units=1"]
                log = directory / "fuzz.log.gz"
                attempt = {"status": "running", "started_utc": now(), "seconds_requested": seconds,
                           "seed": seed, "command": command, "log": str(log.resolve())}
                current["attempts"].append(attempt)
                persist()
                event("start", target=target, attempt=index, seconds=seconds)
                print(f"{now()} {target}: {current['completed_budget_seconds']}/{config['seconds_per_target']} s; slice {index}", flush=True)

                def heartbeat(elapsed):
                    attempt["elapsed_seconds"] = round(elapsed, 3)
                    attempt["live_stats"] = dict(stream_summary.get("stats", {}))
                    persist()

                stream_summary = {}
                code, elapsed, watchdog = execute(command, log, seconds + config["timeout"] + 30,
                                                   heartbeat, stream_summary=stream_summary)
                stats = stream_summary["stats"]
                occurrence = {"campaign": str(out.resolve()), "attempt": index,
                              "completed_utc": now(), "log_sha256": sha(log),
                              "binary_sha256": binary_hashes[target], "seed": seed}
                found = save_findings(target, artifact_dir, findings_dir, occurrence, binaries[target], config)
                errors = [f for f in found if f["kind"] != "slow-unit"]
                passed = (code == 0 and not watchdog and not errors and
                          stats["executions"] > 0 and stats["reported_seconds"] >= seconds)
                attempt.update(exit_code=code, elapsed_seconds=round(elapsed, 3),
                               watchdog_timeout=watchdog, stats=stats, log_sha256=sha(log),
                               uncompressed_log_sha256=stream_summary["uncompressed_log_sha256"],
                               uncompressed_log_bytes=stream_summary["uncompressed_log_bytes"],
                               completed_utc=now(), findings=found,
                               status="pass" if passed else "finding" if errors else "infrastructure_failure")
                if passed:
                    current["completed_budget_seconds"] += seconds
                else:
                    current["status"] = attempt["status"]
                persist()
                event("finished", target=target, attempt=index, status=attempt["status"], stats=stats)
                print(f"{now()} {target}: {attempt['status']}; {stats['executions']} executions; cov={stats['coverage_edges']}", flush=True)
                slices_this_process += 1
                if config.get("max_slices", 0) and slices_this_process >= config["max_slices"]:
                    if current["completed_budget_seconds"] >= config["seconds_per_target"]:
                        current["status"] = "pass"
                    elif passed:
                        current["status"] = "checkpoint"
                    current["corpus_after"] = inventory(target_corpus)
                    if any(t["status"] not in ("pass", "finding", "infrastructure_failure")
                           for t in state["targets"].values()):
                        state["source_inputs_unchanged"] = source_snapshot() == sources
                        state["binaries_unchanged"] = all(sha(binaries[t]) == h for t, h in binary_hashes.items())
                        state["status"] = ("checkpoint" if state["source_inputs_unchanged"] and
                                           state["binaries_unchanged"] else "incomplete")
                        persist()
                        event("checkpoint", target=target, completed_budget_seconds=current["completed_budget_seconds"])
                        return state
                if not passed:
                    break
            if current["completed_budget_seconds"] >= config["seconds_per_target"]:
                current["status"] = "pass"
            current["corpus_after"] = inventory(target_corpus)
            persist()
        unchanged = source_snapshot() == sources
        state["source_inputs_unchanged"] = unchanged
        state["binaries_unchanged"] = all(sha(binaries[t]) == h for t, h in binary_hashes.items())
        statuses = {t["status"] for t in state["targets"].values()}
        state["status"] = ("incomplete" if not unchanged or not state["binaries_unchanged"] or
                           "infrastructure_failure" in statuses else
                           "complete_with_findings" if "finding" in statuses else "complete")
        state["completed_utc"] = now()
        persist()
        return state
    except BaseException:
        state["status"] = "interrupted"
        if attempt and attempt["status"] == "running":
            attempt["status"] = "interrupted"
        if current and current["status"] == "running":
            current["status"] = "interrupted"
        persist()
        event("interrupted")
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True, help="Fresh evidence directory (reuse only with --resume)")
    parser.add_argument("--corpus", type=Path, default=ROOT / "fuzz/corpus")
    parser.add_argument("--findings", type=Path, default=ROOT / "fuzz/findings")
    parser.add_argument("--target-dir", type=Path, default=ROOT / "tmp/fuzz-build")
    parser.add_argument("--toolchain", default="nightly")
    parser.add_argument("--targets", nargs="+", choices=TARGETS, default=list(TARGETS))
    parser.add_argument("--seconds-per-target", type=int, default=1800)
    parser.add_argument("--slice-seconds", type=int, default=300)
    parser.add_argument("--timeout", type=int, default=5)
    parser.add_argument("--rss-limit-mb", type=int, default=1024)
    parser.add_argument("--seed", type=int, default=1612026)
    parser.add_argument("--max-slices", type=int, default=0,
                        help="Exit cleanly after N slices; 0 runs the whole campaign. Resume retains the full budget.")
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    if (not 1 <= args.seconds_per_target <= 86400 or not 1 <= args.slice_seconds <= 1800 or
            not 1 <= args.timeout <= 300 or not 128 <= args.rss_limit_mb <= 16384 or
            not 0 < args.seed < 2 ** 32 or not 0 <= args.max_slices <= 10000 or
            len(set(args.targets)) != len(args.targets)):
        parser.error("Invalid budget, seed or repeated target")
    for key in ("out", "corpus", "findings", "target_dir"):
        setattr(args, key, getattr(args, key).resolve())
    if not args.resume and args.out.exists():
        parser.error("--out already exists; use --resume or choose a fresh directory")
    if args.resume and not (args.out / "progress.json").is_file():
        parser.error("--resume requires a previous progress.json")
    build_log = args.out.parent / (args.out.name + "-build.log")
    build_log.parent.mkdir(parents=True, exist_ok=True)
    # Never silently overwrite an earlier build log, including interrupted builds.
    if build_log.exists():
        build_log = build_log.with_name(build_log.name + "." + str(time.time_ns()))
    sources = source_snapshot()
    config = {key: str(value) if isinstance(value, Path) else value for key, value in vars(args).items()}
    with campaign_lock(args.corpus):
        build = ["cargo", f"+{args.toolchain}", "fuzz", "build", "--sanitizer", "address",
                 "--target-dir", str(args.target_dir), "--codegen-units", "16"]
        print(f"{now()} Building seven ASAN harnesses (outside fuzzing budget)", flush=True)
        code, _, watchdog = execute(build, build_log, 1800)
        if code or watchdog:
            raise ValueError(f"Harness build failed; see {build_log}")
        if source_snapshot() != sources:
            raise ValueError("Sources changed while building; no campaign was started")
        rustc = subprocess.check_output(["rustc", f"+{args.toolchain}", "-vV"], text=True)
        host = re.search(r"^host: (.+)$", rustc, re.M)[1]
        binaries = {t: args.target_dir / host / "release" / t for t in args.targets}
        write_json(build_log.with_suffix(build_log.suffix + ".json"), {
            "rustc": rustc, "cargo_fuzz": subprocess.check_output(["cargo", "fuzz", "--version"], text=True).strip(),
            "build_command": build, "build_log": str(build_log), "build_log_sha256": sha(build_log),
            "sanitizer": "address", "source_inputs": sources})
        result = campaign(config, binaries, sources)
        print(f"{now()} {result['status']}: {args.out / 'progress.json'}", flush=True)
        return 0 if result["status"] in ("complete", "checkpoint") else 1


def interrupted(_signum, _frame):
    raise KeyboardInterrupt


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, interrupted)
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("Interrupted; corpus and completed slices are preserved. Resume with the same options and --resume.", file=sys.stderr)
        sys.exit(130)
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"Campaign error: {error}", file=sys.stderr)
        sys.exit(2)
