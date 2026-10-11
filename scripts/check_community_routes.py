#!/usr/bin/env python3
"""Audit and exercise pinned community content without downloading or changing it.

Content stays in the player's directories. A failed native import is reported as
blocked, never retried with its signal scripts disabled. Optional GPU checkpoints
use the real viewer on a private Weston display; they do not certify OR parity.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib

from check_viewer_streaming import run_checkpoint


def child(root, name):
    if not root.is_dir():
        return root / name
    matches = sorted(p for p in root.iterdir() if p.name.lower() == name.lower())
    return next((p for p in matches if p.name == name), matches[0] if matches else root / name)


def original_file(root, name):
    path = child(root, name)
    if not path.is_file():
        raise ValueError("Original resource missing: " + name)
    return path


def find_route(case, roots):
    packages = sorted({p.resolve() for root in roots for p in root.glob(case["package_glob"])
                       if p.is_dir()})
    for package in packages:
        manifest_path = package / "openrailsrs-content.json"
        if manifest_path.is_file():
            manifest = json.loads(manifest_path.read_text())
            if case.get("revision") and (manifest.get("revision") or {}).get("commit") != case["revision"]:
                continue
            if case.get("sha256") and manifest.get("download_sha256") != case["sha256"]:
                continue
        candidates = sorted(p.parent for p in package.rglob("*")
                            if p.is_file() and p.suffix.lower() == ".tdb"
                            and p.parent.name.lower() == case["route_folder"].lower())
        for route in candidates:
            if route.resolve().is_relative_to(package):
                return package, route
    raise ValueError("Install the author's package first: " + case["source"])


def command(command_line, output, timeout):
    with output.open("w") as log:
        try:
            result = subprocess.run([str(v) for v in command_line], stdout=log,
                                    stderr=subprocess.STDOUT, timeout=timeout)
            return result.returncode
        except subprocess.TimeoutExpired:
            log.write("\nQA timeout: the process was stopped.\n")
            return 124


def error_tail(path):
    return path.read_text(errors="replace")[-1800:].strip()


def gpu_summary(report):
    return {key: report.get(key) for key in (
        "odometer_m", "peak_rss_mib", "elapsed_s", "renderer", "gpu_tiles",
        "gpu_entities", "unactivated_near_shapes", "pending_gpu_uploads",
        "pending_terrain_tiles", "shader_pipelines", "native_signal_errors",
        "formation_validation", "frame_timing",
    )}


def exercise(case, args):
    output = args.out_dir / case["id"]
    output.mkdir(parents=True, exist_ok=True)
    record = {"id": case["id"], "name": case["name"], "source": case["source"],
              "status": "blocked", "audit": None, "import": None, "service": None,
              "gpu": {}, "scope": "installed stock, native import, optional player checkpoints"}
    record["cli_sha256"] = args.cli_sha256
    if args.gpu:
        record["viewer_sha256"] = args.viewer_sha256
    try:
        package, route = find_route(case, args.content_root)
        record["route_in_package"] = route.relative_to(package).as_posix()
        record["package"] = package.name
        audit = {}
        stock = child(child(route.parent.parent, "TRAINS"), "CONSISTS")
        if stock.is_dir():
            audit_path = output / "audit.json"
            code = command([args.cli, "audit-consists", stock, "--json"],
                           audit_path, args.timeout_s)
            if code == 0:
                audit = json.loads(audit_path.read_text())
                record["audit"] = {key: audit[key] for key in ("total", "content_valid", "player_ready")}
            else:
                record["audit"] = {"exit_code": code, "error": error_tail(audit_path)}
        else:
            record["audit"] = {"total": 0, "content_valid": 0, "player_ready": 0}
        # Test the route even when it ships no stock or no original activity.
        import_dir = output / "import"
        log = output / "import.log"
        code = command([args.cli, "import-msts", route, "--out-dir", import_dir],
                       log, args.timeout_s)
        record["import"] = {"exit_code": code}
        if code:
            record["import"]["error"] = error_tail(log)
            return record
        record["status"] = "route_imported"
        if not case.get("activity") or not case.get("consist"):
            record["limitation"] = "No selected original activity and complete original formation"
            return record
        activity = original_file(child(route, "ACTIVITIES"), case["activity"])
        consist = original_file(stock, case["consist"])
        log = output / "activity.log"
        code = command([args.cli, "import-msts", route, "--out-dir", import_dir,
                        "--activity", activity, "--consist", consist], log, args.timeout_s)
        record["activity"] = {"name": activity.name, "consist": consist.name, "exit_code": code,
                              "consist_override": bool(case.get("consist_override"))}
        if code:
            record["activity"]["error"] = error_tail(log)
            record["status"] = "activity_blocked"
            return record
        scenario_path = import_dir / "scenario.toml"
        scenario = tomllib.loads(scenario_path.read_text())
        formation = next((form["report"] for form in audit.get("formations", [])
                          if Path(form["report"]["path"]).resolve() == consist.resolve()), {})
        record["status"] = "activity_imported"
        record["activity"]["stops"] = [s["name"] for s in scenario["route"].get("stops", [])]
        if args.service:
            log = output / "service.log"
            outcome = output / "service" / "outcome.json"
            # A failed new process must not inherit a prior run's success.
            outcome.unlink(missing_ok=True)
            code = command([args.cli, "play-service", scenario_path, "--out-dir", output / "service"],
                           log, args.timeout_s)
            record["service"] = {"exit_code": code}
            if outcome.is_file():
                record["service"]["outcome"] = json.loads(outcome.read_text())
            if code:
                record["service"]["error"] = error_tail(log)
        if args.gpu:
            for name, follow, distance in (("cab", "driver", 0), ("exterior", "orbit", args.travel_m)):
                capture_args = argparse.Namespace(
                    repo=args.repo, viewer=args.viewer, scenario=scenario_path,
                    route_root=route, out_dir=output, timeout_s=args.timeout_s,
                    max_rss_mib=args.max_rss_mib, software=False, require_hardware=True,
                    headless_wayland=True, follow=follow, after_service=False,
                    autodrive=0.75, speed_mul=16, ready_frames=8,
                    formation_cars=formation.get("vehicles"), view_radius_m=2000,
                )
                if name == "exterior":
                    capture_args.camera_yaw = 1.1
                    capture_args.camera_pitch = 0.3
                    capture_args.camera_distance = max(25, min(400, 1.4 * formation.get("length_m", 50)))
                try:
                    report = run_checkpoint(capture_args, name, distance, True)
                    record["gpu"][name] = {"passed": True, **gpu_summary(report)}
                except (RuntimeError, ValueError, AssertionError, OSError) as error:
                    record["gpu"][name] = {"passed": False, "error": str(error)}
            record["status"] = ("gpu_checkpoints_passed" if all(r["passed"] for r in record["gpu"].values())
                                else "gpu_checkpoint_failed")
    except (ValueError, OSError, KeyError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        record["error"] = str(error)
    return record


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", type=Path, default=repo / "docs/fixtures/compatibility/community-routes-2026-10-11/cases.json")
    parser.add_argument("--content-root", action="append", type=Path, required=True,
                        help="Directory containing original installed packages; repeat for additional libraries")
    parser.add_argument("--out-dir", type=Path, default=repo / "tmp/community-routes-qa")
    parser.add_argument("--cli", type=Path, default=repo / "target/debug/openrailsrs")
    parser.add_argument("--viewer", type=Path, default=repo / "target/debug/openrailsrs-viewer3d")
    parser.add_argument("--only", nargs="+", help="Case IDs to run; existing results for other cases are retained")
    parser.add_argument("--gpu", action="store_true", help="Render initial cab and exterior after actual travel")
    parser.add_argument("--service", action="store_true", help="Attempt the whole native service with the player's physics")
    parser.add_argument("--travel-m", type=float, default=200)
    parser.add_argument("--timeout-s", type=int, default=180)
    parser.add_argument("--max-rss-mib", type=int, default=6144)
    args = parser.parse_args()
    args.repo = repo
    args.out_dir, args.cli, args.viewer = (p.resolve() for p in (args.out_dir, args.cli, args.viewer))
    args.content_root = [p.resolve() for p in args.content_root]
    if args.travel_m <= 0 or args.timeout_s < 30 or args.max_rss_mib <= 0:
        parser.error("Use a positive travel/RSS limit and at least a 30-second timeout")
    manifest = json.loads(args.cases.read_text())
    if args.only and set(args.only) - {case["id"] for case in manifest["routes"]}:
        parser.error("Unknown case ID")
    args.out_dir.mkdir(parents=True, exist_ok=True)
    args.cli_sha256 = hashlib.sha256(args.cli.read_bytes()).hexdigest()
    args.viewer_sha256 = hashlib.sha256(args.viewer.read_bytes()).hexdigest() if args.gpu else None
    result_path = args.out_dir / "results.json"
    results = json.loads(result_path.read_text()) if result_path.exists() else {}
    for record in results.get("routes", []):
        record.setdefault("cli_sha256", results.get("cli_sha256"))
        if record.get("gpu"):
            record.setdefault("viewer_sha256", results.get("viewer_sha256"))
    results.update(schema=1, date=manifest["date"], selection=manifest["selection"],
                   cli_sha256=args.cli_sha256)
    if args.gpu:
        results["viewer_sha256"] = args.viewer_sha256
    records = {record["id"]: record for record in results.get("routes", [])}
    for case in manifest["routes"]:
        if args.only and case["id"] not in args.only:
            continue
        record = exercise(case, args)
        records[case["id"]] = record
        results["routes"] = [records[c["id"]] for c in manifest["routes"] if c["id"] in records]
        result_path.write_text(json.dumps(results, indent=2, ensure_ascii=False) + "\n")
        print(json.dumps({"id": record["id"], "status": record["status"],
                          "audit": record["audit"], "import": record["import"]}, ensure_ascii=False), flush=True)
    # Expected incompatibilities are data, not a reason to silently omit a route.
    if any(r["status"] in ("blocked", "activity_blocked", "gpu_checkpoint_failed")
           or (r.get("audit") or {}).get("exit_code", 0)
           or (r.get("service") or {}).get("exit_code", 0) for r in records.values()):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
