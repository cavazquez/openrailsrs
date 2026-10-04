#!/usr/bin/env python3
"""Capture Chiltern after real travel; require scenery coverage and bounded RSS.

Linux/Xvfb; uses the existing viewer build and original route assets. This is a
streaming regression, not an Open Rails pixel-parity or physics acceptance test.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time


def run_checkpoint(args, name, target, pause):
    prefix = args.out_dir / name
    # A successful process exit must not reuse captures from an earlier run.
    for suffix in (".png", ".stream.json"):
        prefix.with_suffix(suffix).unlink(missing_ok=True)
    env = {key: value for key, value in os.environ.items()
           if not key.startswith("OPENRAILSRS_")}
    env.pop("WAYLAND_DISPLAY", None)
    env.update({
        "OPENRAILSRS_FOLLOW": getattr(args, "follow", "driver"),
        "OPENRAILSRS_VIEW_RADIUS_M": str(getattr(args, "view_radius_m", 2000)),
        "OPENRAILSRS_WINDOW_WIDTH": "1280",
        "OPENRAILSRS_WINDOW_HEIGHT": "720",
        "OPENRAILSRS_DISABLE_AUDIO": "1",
        "OPENRAILSRS_AUTODRIVE": str(getattr(args, "autodrive", 0.75)),
        "OPENRAILSRS_SPEED_MUL": str(getattr(args, "speed_mul", 16)),
        "OPENRAILSRS_SCREENSHOT": str(prefix.with_suffix(".png")),
        "OPENRAILSRS_SCREENSHOT_AFTER_READY": "1",
        "OPENRAILSRS_SCREENSHOT_READY_FRAMES": str(getattr(args, "ready_frames", 8)),
        "OPENRAILSRS_SCREENSHOT_MIN_ODOMETER_M": str(target),
        "OPENRAILSRS_SCREENSHOT_PAUSE_AT_TARGET": "1" if pause else "0",
        "OPENRAILSRS_SCREENSHOT_AFTER_SERVICE": "0" if pause else "1",
        "OPENRAILSRS_SCREENSHOT_DELAY_S": str(args.timeout_s - 10),
    })
    if hasattr(args, "camera_yaw"):
        env.update(OPENRAILSRS_CAM_YAW=str(args.camera_yaw),
                   OPENRAILSRS_CAM_PITCH=str(args.camera_pitch),
                   OPENRAILSRS_CAM_DIST=str(args.camera_distance))
    if args.software:
        env.update({
            "VK_ICD_FILENAMES": "/usr/share/vulkan/icd.d/lvp_icd.json",
            "WGPU_BACKEND": "vulkan",
            "WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER": "1",
        })
    read_fd, write_fd = os.pipe()
    viewer = None
    peak_kib = 0
    started = time.monotonic()
    with prefix.with_suffix(".xvfb.log").open("w") as xlog:
        display = subprocess.Popen([
            "Xvfb", "-displayfd", str(write_fd), "-screen", "0",
            "1280x720x24", "-nolisten", "tcp",
        ], pass_fds=(write_fd,), stdout=xlog, stderr=xlog)
        os.close(write_fd)
        try:
            with os.fdopen(read_fd) as number:
                env["DISPLAY"] = ":" + number.readline().strip()
            with prefix.with_suffix(".log").open("w") as log:
                command = [
                    str(args.viewer), str(args.scenario), "--live",
                    "--route-root", str(args.route_root),
                ]
                if hasattr(args, "cab_fov_deg"):
                    command.extend(["--cab-fov", str(args.cab_fov_deg)])
                viewer = subprocess.Popen(command, cwd=args.repo, env=env, stdout=log, stderr=log)
                while viewer.poll() is None:
                    try:
                        status = Path(f"/proc/{viewer.pid}/status").read_text()
                        rss = next(int(row.split()[1]) for row in status.splitlines()
                                   if row.startswith("VmRSS:"))
                        peak_kib = max(peak_kib, rss)
                        if rss > args.max_rss_mib * 1024:
                            raise RuntimeError(f"{name}: RSS limit exceeded ({rss / 1024:.0f} MiB)")
                    except (FileNotFoundError, StopIteration):
                        pass
                    if time.monotonic() - started > args.timeout_s:
                        raise RuntimeError(f"{name}: capture timed out")
                    time.sleep(0.25)
                if viewer.returncode:
                    raise RuntimeError(f"{name}: viewer exited {viewer.returncode}; see {prefix.with_suffix('.log')}")
        finally:
            if viewer is not None and viewer.poll() is None:
                viewer.terminate()
                try:
                    viewer.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    viewer.kill()
                    viewer.wait()
            display.terminate()
            display.wait(timeout=5)
    image = prefix.with_suffix(".png")
    report = json.loads(prefix.with_suffix(".stream.json").read_text())
    if not image.is_file() or report["odometer_m"] < target:
        raise RuntimeError(f"{name}: actual travel checkpoint missing")
    if report["unactivated_near_shapes"] != 0 or not report["gpu_entities"]:
        raise RuntimeError(f"{name}: scenery incomplete: {report}")
    if report.get("shader_pipelines") != {"pending": 0, "failed": 0}:
        raise RuntimeError(f"{name}: scenery shaders incomplete: {report.get('shader_pipelines')}")
    if not pause and not report["service_complete"]:
        raise RuntimeError(f"{name}: service did not complete")
    if getattr(args,"require_hardware",False) and not (report.get("renderer") or {}).get("hardware"):
        raise RuntimeError(f"{name}: hardware benchmark requested, renderer is {report.get('renderer')}")
    report.update(peak_rss_mib=round(peak_kib / 1024, 1),
                  elapsed_s=round(time.monotonic() - started, 1))
    prefix.with_suffix(".stream.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"PASS {name}: {report['odometer_m']:.1f} m, "
          f"{report['gpu_tiles']} GPU tiles, RSS {report['peak_rss_mib']:.0f} MiB", flush=True)
    return report


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--route-root", type=Path, required=True)
    parser.add_argument("--viewer", type=Path, default=repo / "target/debug/openrailsrs-viewer3d")
    parser.add_argument("--out-dir", type=Path, default=repo / "tmp/viewer-streaming")
    parser.add_argument("--scenario", type=Path, default=repo / "examples/chiltern_local/scenario.toml")
    parser.add_argument("--require-hardware",action="store_true",help="fail instead of reporting CPU/software rendering as GPU performance")
    parser.add_argument("--software", action="store_true", help="use Vulkan lavapipe")
    parser.add_argument("--checkpoint", choices=["middle", "terminal"], action="append")
    parser.add_argument("--max-rss-mib", type=int, default=6144)
    parser.add_argument("--timeout-s", type=int, default=270)
    args = parser.parse_args()
    args.repo = repo
    args.scenario = args.scenario.resolve()
    args.viewer = args.viewer.resolve()
    args.route_root = args.route_root.resolve()
    args.out_dir = args.out_dir.resolve()
    if not args.viewer.is_file() or not args.route_root.is_dir():
        parser.error("requires an existing viewer build and original Chiltern route")
    if args.max_rss_mib <= 0 or args.timeout_s <= 10:
        parser.error("RSS limit must be positive; timeout must exceed 10 seconds")
    args.out_dir.mkdir(parents=True, exist_ok=True)
    # Each capture starts at Northolt Park, then actually drives to its target.
    # Runs are sequential so the memory check never counts two renderers.
    selected = set(args.checkpoint or ["middle", "terminal"])
    reports = {}
    for name, target, pause in [("middle", 3700, True), ("terminal", 7054, False)]:
        if name in selected:
            reports[name] = run_checkpoint(args, name, target, pause)
    (args.out_dir / "report.json").write_text(json.dumps(reports, indent=2) + "\n")


if __name__ == "__main__":
    main()
