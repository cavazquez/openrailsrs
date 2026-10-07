#!/usr/bin/env python3
"""Capture Chiltern after real travel; require scenery coverage and bounded RSS.

Linux/Xvfb; uses the existing viewer build and original route assets. This is a
streaming regression, not an Open Rails pixel-parity or physics acceptance test.
"""

import argparse
import json
import math
import os
import re
from pathlib import Path
import subprocess
import tempfile
import time
import tomllib


def drm_client_vram(text):
    """Identify a process's DRM client; do not average unrelated GPUs."""
    fields = dict(line.split(":", 1) for line in text.splitlines() if ":" in line)
    pci = fields.get("drm-pdev", "").strip()
    client = fields.get("drm-client-id", "").strip()
    if not client or not re.fullmatch(r"[0-9a-fA-F]{4}:[0-9a-fA-F]{2}:[0-9a-fA-F]{2}\.[0-7]", pci):
        return None
    value = fields.get("drm-resident-vram", fields.get("drm-memory-vram", ""))
    try:
        number, unit = value.split()
        divisor = {"KiB": 1024, "kB": 1024, "MiB": 1, "B": 1048576, "bytes": 1048576}[unit]
        memory = float(number) / divisor
        if not math.isfinite(memory) or memory < 0:
            return None
    except (ValueError, KeyError):
        return None
    return pci, client, memory


def process_gpu_vram(fdinfo_dir):
    clients, devices = set(), {}
    for path in fdinfo_dir.glob("*"):
        try:
            client = drm_client_vram(path.read_text())
        except OSError:
            continue
        if client is None:
            continue
        pci, client_id, memory = client
        if (pci, client_id) in clients:
            continue
        clients.add((pci, client_id))
        devices[pci] = devices.get(pci, 0) + memory
    return devices


def compare_pullman_cab_foreground(reference, candidate):
    """Check fixed opaque cab panels at the harness's 1280×720 / 60° view.

    These regions exclude the windows, instruments and service-summary overlay.
    This catches a disappearing cab even when Bevy's visibility/asset counters
    still pass. The reference must use the same Pullman cab, seat and daylight.
    It is an internal rendering regression, not Open Rails pixel parity.
    """
    from PIL import Image, ImageChops, ImageStat

    regions = (
        (115, 260, 230, 330),
        (50, 305, 200, 368),
        (300, 400, 404, 482),
        (943, 331, 1015, 392),
        (1050, 360, 1150, 415),
        (520, 564, 708, 616),
    )
    with Image.open(reference) as original, Image.open(candidate) as captured:
        if original.size != (1280, 720) or captured.size != original.size:
            raise ValueError("Pullman cab foreground requires two 1280×720 frames")
        original, captured = original.convert("RGB"), captured.convert("RGB")
        errors = [
            sum(ImageStat.Stat(ImageChops.difference(
                original.crop(region), captured.crop(region)
            )).mean) / 3.0
            for region in regions
        ]
    return {
        "passed": sum(error <= 30.0 for error in errors) >= 4,
        "matching_regions": sum(error <= 30.0 for error in errors),
        "minimum_matching_regions": 4,
        "region_mean_absolute_rgb_errors": errors,
        "maximum_mean_absolute_rgb_error": 30.0,
    }


def run_checkpoint(args, name, target, pause):
    prefix = args.out_dir / name
    after_service = getattr(args, "after_service", not pause)
    # A successful process exit must not reuse captures from an earlier run.
    for suffix in (".png", ".stream.json"):
        prefix.with_suffix(suffix).unlink(missing_ok=True)
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith("OPENRAILSRS_")
    }
    env.pop("WAYLAND_DISPLAY", None)
    env.update(
        {
            "OPENRAILSRS_FOLLOW": getattr(args, "follow", "driver"),
            "OPENRAILSRS_VIEW_RADIUS_M": str(getattr(args, "view_radius_m", 2000)),
            "OPENRAILSRS_WINDOW_WIDTH": "1280",
            "OPENRAILSRS_WINDOW_HEIGHT": "720",
            "OPENRAILSRS_DISABLE_AUDIO": "1",
            "OPENRAILSRS_PLAYER_DIR": str(args.out_dir / ".player-data"),
            "OPENRAILSRS_AUTODRIVE": str(getattr(args, "autodrive", 0.75)),
            "OPENRAILSRS_SPEED_MUL": str(getattr(args, "speed_mul", 16)),
            "OPENRAILSRS_SCREENSHOT": str(prefix.with_suffix(".png")),
            "OPENRAILSRS_SCREENSHOT_AFTER_READY": "1",
            "OPENRAILSRS_SCREENSHOT_READY_FRAMES": str(
                getattr(args, "ready_frames", 8)
            ),
            "OPENRAILSRS_SCREENSHOT_MIN_ODOMETER_M": str(target),
            "OPENRAILSRS_SCREENSHOT_PAUSE_AT_TARGET": "1" if pause else "0",
            "OPENRAILSRS_SCREENSHOT_AFTER_SERVICE": "1" if after_service else "0",
            "OPENRAILSRS_SCREENSHOT_DELAY_S": str(args.timeout_s - 10),
        }
    )
    if hasattr(args, "camera_yaw"):
        env.update(
            OPENRAILSRS_CAM_YAW=str(args.camera_yaw),
            OPENRAILSRS_CAM_PITCH=str(args.camera_pitch),
            OPENRAILSRS_CAM_DIST=str(args.camera_distance),
        )
    for attr, key in (("look_yaw", "OPENRAILSRS_LOOK_YAW"), ("look_pitch", "OPENRAILSRS_LOOK_PITCH")):
        if getattr(args, attr, None) is not None:
            env[key] = str(getattr(args, attr))
    if getattr(args, "visual_fault", None) in ("occluder", "hide_train", "mirror"):
        env["OPENRAILSRS_VISUAL_FAULT"] = args.visual_fault
    if getattr(args,"capture_or_focus",False):
        env["OPENRAILSRS_CAPTURE_OR_FOCUS"]="1"
    if getattr(args, "flip_u", False):
        env["OPENRAILSRS_DEBUG_FLIP_U"] = "1"
    for attr,key in (("fog_quality","OPENRAILSRS_FOG_QUALITY"),("train_motion","OPENRAILSRS_TRAIN_MOTION"),("capture_headlights","OPENRAILSRS_CAPTURE_HEADLIGHTS")):
        if hasattr(args,attr): env[key] = str(getattr(args,attr))
    if hasattr(args, "weather"):
        env["OPENRAILSRS_WEATHER"] = args.weather
    for attr, key in (
        ("weather_profile", "OPENRAILSRS_WEATHER_PROFILE"),
        ("weather_seed", "OPENRAILSRS_WEATHER_SEED"),
        ("weather_phase_s", "OPENRAILSRS_WEATHER_PHASE_S"),
        ("weather_quality", "OPENRAILSRS_WEATHER_QUALITY"),
        ("framepace", "OPENRAILSRS_FRAMEPACE"),
        ("present_mode", "OPENRAILSRS_PRESENT_MODE"),
        ("scenery_profile", "OPENRAILSRS_SCENERY_PROFILE"),
        ("scenery_quality", "OPENRAILSRS_SCENERY_QUALITY"),
        ("camera_journey", "OPENRAILSRS_CAMERA_JOURNEY"),
        ("train_effects_enabled", "OPENRAILSRS_TRAIN_EFFECTS_ENABLED"),
        ("dev_inspector", "OPENRAILSRS_DEV_INSPECTOR"),
        ("dev_tools", "OPENRAILSRS_DEV_TOOLS"),
        ("dev_inspector_select", "OPENRAILSRS_DEV_INSPECTOR_SELECT"),
        ("capture_wiper", "OPENRAILSRS_CAPTURE_WIPER"),
    ):
        if getattr(args, attr, None) is not None:
            env[key] = str(getattr(args, attr))
    if getattr(args, "real_time", False):
        env["OPENRAILSRS_REAL_TIME"] = "1"
    if getattr(args, "real_weather", False):
        env["OPENRAILSRS_REAL_WEATHER"] = "1"
    if getattr(args, "during_lightning", False):
        env["OPENRAILSRS_SCREENSHOT_DURING_LIGHTNING"] = "1"
        env["OPENRAILSRS_SCREENSHOT_PAUSE_AT_TARGET"] = "0"
    env["OPENRAILSRS_WEATHER_EXECUTION"] = getattr(args, "weather_execution", "auto")
    env["OPENRAILSRS_TRAIN_EFFECT_EXECUTION"] = getattr(args, "train_effect_execution", "auto")
    if getattr(args, "texture_cache", "on") == "off":
        env["OPENRAILSRS_TEXTURE_CACHE"] = "off"
    if getattr(args, "texture_upload", "auto") == "rgba":
        env["OPENRAILSRS_TEXTURE_UPLOAD"] = "rgba"
    if getattr(args, "debug_materials", False):
        env["OPENRAILSRS_DEBUG_MATERIALS"] = "1"
    env["OPENRAILSRS_RENDERER"] = "cpu" if args.software else getattr(args, "renderer", "auto")
    if getattr(args, "particle_budget", None) is not None:
        env["OPENRAILSRS_WEATHER_PARTICLE_BUDGET"] = str(args.particle_budget)
    if args.software:
        env.update(
            {
                "VK_ICD_FILENAMES": "/usr/share/vulkan/icd.d/lvp_icd.json",
                "WGPU_BACKEND": "vulkan",
                "WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER": "1",
            }
        )
    viewer = None
    peak_kib = 0
    cpu_start = cpu_end = None
    gpu_busy, gpu_peak_vram = {}, {}
    started = time.monotonic()
    private_runtime = None
    with prefix.with_suffix(".xvfb.log").open("w") as xlog:
        if getattr(args, "headless_wayland", False):
            # Xvfb lacks DRI3 for AMD hardware Vulkan. Weston supplies an
            # isolated accelerated Wayland surface without the user's desktop.
            # Unix socket paths are limited to 108 bytes. A checkout/output
            # directory can be longer; use a short, private runtime in /tmp.
            private_runtime = tempfile.TemporaryDirectory(prefix="openrailsrs-")
            runtime = Path(private_runtime.name)
            env.pop("DISPLAY", None)
            env.update(XDG_RUNTIME_DIR=str(runtime), WAYLAND_DISPLAY="openrailsrs-qa")
            env["__EGL_VENDOR_LIBRARY_FILENAMES"] = (
                "/usr/share/glvnd/egl_vendor.d/50_mesa.json"
            )
            display = subprocess.Popen(
                [
                    "weston",
                    "--backend=headless",
                    "--renderer=gl",
                    "--no-config",
                    "--socket=openrailsrs-qa",
                    "--idle-time=0",
                    "--width=1280",
                    "--height=720",
                ],
                env=env,
                stdout=xlog,
                stderr=xlog,
            )
            display_number = None
        else:
            read_fd, write_fd = os.pipe()
            display = subprocess.Popen(
                [
                    "Xvfb",
                    "-displayfd",
                    str(write_fd),
                    "-screen",
                    "0",
                    "1280x720x24",
                    "-nolisten",
                    "tcp",
                ],
                pass_fds=(write_fd,),
                stdout=xlog,
                stderr=xlog,
            )
            os.close(write_fd)
            display_number = os.fdopen(read_fd)
        try:
            if display_number is not None:
                with display_number as number:
                    env["DISPLAY"] = ":" + number.readline().strip()
            else:
                ready_until = time.monotonic() + 15.0
                while not (runtime / env["WAYLAND_DISPLAY"]).exists():
                    if display.poll() is not None or time.monotonic() > ready_until:
                        raise RuntimeError(
                            f"Private Weston did not start; see {prefix.with_suffix('.xvfb.log')}"
                        )
                    time.sleep(0.05)
            with prefix.with_suffix(".log").open("w") as log:
                command = [
                    str(args.viewer),
                    str(args.scenario),
                    "--live",
                    "--route-root",
                    str(args.route_root),
                ]
                if hasattr(args, "cab_fov_deg"):
                    command.extend(["--cab-fov", str(args.cab_fov_deg)])
                viewer = subprocess.Popen(
                    command, cwd=args.repo, env=env, stdout=log, stderr=log
                )
                while viewer.poll() is None:
                    try:
                        status = Path(f"/proc/{viewer.pid}/status").read_text()
                        rss = next(
                            int(row.split()[1])
                            for row in status.splitlines()
                            if row.startswith("VmRSS:")
                        )
                        peak_kib = max(peak_kib, rss)
                        # Per-process CPU seconds; percent uses one core = 100%.
                        stat = Path(f"/proc/{viewer.pid}/stat").read_text().rsplit(")", 1)[1].split()
                        cpu_end = (int(stat[11]) + int(stat[12])) / os.sysconf("SC_CLK_TCK")
                        if cpu_start is None:
                            cpu_start = cpu_end
                        for pci, memory in process_gpu_vram(Path(f"/proc/{viewer.pid}/fdinfo")).items():
                            gpu_peak_vram[pci] = max(gpu_peak_vram.get(pci, 0), memory)
                            try:
                                busy = float((Path("/sys/bus/pci/devices") / pci / "gpu_busy_percent").read_text())
                                gpu_busy.setdefault(pci, []).append(busy)
                            except (OSError, ValueError):
                                pass
                        if rss > args.max_rss_mib * 1024:
                            raise RuntimeError(
                                f"{name}: RSS limit exceeded ({rss / 1024:.0f} MiB)"
                            )
                    except (FileNotFoundError, StopIteration):
                        pass
                    if time.monotonic() - started > args.timeout_s:
                        raise RuntimeError(f"{name}: capture timed out")
                    # Shader failures cannot satisfy the readiness gate. Report
                    # them immediately instead of waiting several minutes.
                    log.flush()
                    with prefix.with_suffix(".log").open("rb") as diagnostics:
                        diagnostics.seek(max(0, diagnostics.seek(0, 2) - 65536))
                        recent = diagnostics.read().decode("utf-8", errors="replace")
                    if "failed to process shader" in recent or "panicked at" in recent:
                        raise RuntimeError(f"{name}: renderer/shader failure; see {prefix.with_suffix('.log')}")
                    time.sleep(0.25)
                if viewer.returncode:
                    raise RuntimeError(
                        f"{name}: viewer exited {viewer.returncode}; see {prefix.with_suffix('.log')}"
                    )
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
            if private_runtime is not None:
                private_runtime.cleanup()
    log_text = prefix.with_suffix(".log").read_text(errors="replace")
    if "ERROR bevy" in log_text or "panicked at" in log_text:
        raise RuntimeError(f"{name}: renderer errors; see {prefix.with_suffix('.log')}")
    image = prefix.with_suffix(".png")
    report = json.loads(prefix.with_suffix(".stream.json").read_text())
    elapsed = time.monotonic() - started
    selected_gpu = max(gpu_peak_vram, key=gpu_peak_vram.get, default=None)
    busy_samples = gpu_busy.get(selected_gpu, [])
    report["host_usage"] = {
        "cpu_percent_one_core": None if cpu_start is None else (cpu_end - cpu_start) / elapsed * 100,
        "gpu_busy_percent_mean": None if not busy_samples else sum(busy_samples) / len(busy_samples),
        "selected_gpu_pci": selected_gpu,
        "gpu_busy_percent_by_device": {pci: sum(values) / len(values) for pci, values in gpu_busy.items()},
        "scope": "whole launch including loading; selected DRM device has largest observed process VRAM; global busy includes compositor/other processes",
    }
    if getattr(args, "pullman_cab_reference", None):
        report["cab_foreground"] = compare_pullman_cab_foreground(
            args.pullman_cab_reference, image
        )
        prefix.with_suffix(".stream.json").write_text(json.dumps(report, indent=2) + "\n")
        if not report["cab_foreground"]["passed"]:
            raise RuntimeError(f"{name}: opaque Pullman cab panels disappeared or changed; see {image}")
    if not image.is_file() or report["odometer_m"] < target:
        raise RuntimeError(f"{name}: actual travel checkpoint missing")
    if report["unactivated_near_shapes"] != 0 or not report["gpu_entities"]:
        raise RuntimeError(f"{name}: scenery incomplete: {report}")
    if report.get("shader_pipelines") != {"pending": 0, "failed": 0}:
        raise RuntimeError(
            f"{name}: scenery shaders incomplete: {report.get('shader_pipelines')}"
        )
    if report.get("native_signal_errors"):
        raise RuntimeError(f"{name}: native signal script errors: {report['native_signal_errors']}")
    if report.get("pending_gpu_uploads", 0):
        raise RuntimeError(f"{name}: textures or meshes not yet uploaded to the GPU")
    if report.get("pending_terrain_tiles", 0):
        raise RuntimeError(f"{name}: native terrain still being prepared")
    if after_service and not report["service_complete"]:
        raise RuntimeError(f"{name}: service did not complete")
    if after_service and hasattr(args, "expected_station_names"):
        stops = report.get("station_results") or []
        if [stop["name"] for stop in stops] != args.expected_station_names:
            raise RuntimeError(f"{name}: station results missing or out of order")
        if any(
            stop["position_error_m"] > 10.0 or stop["arrival_speed_mps"] > 0.1
            for stop in stops
        ):
            raise RuntimeError(
                f"{name}: a station was not reached within stopping tolerances"
            )
    if getattr(args, "require_hardware", False) and not (
        report.get("renderer") or {}
    ).get("hardware"):
        raise RuntimeError(
            f"{name}: hardware benchmark requested, renderer is {report.get('renderer')}"
        )
    report.update(
        peak_rss_mib=round(peak_kib / 1024, 1),
        elapsed_s=round(time.monotonic() - started, 1),
    )
    prefix.with_suffix(".stream.json").write_text(json.dumps(report, indent=2) + "\n")
    print(
        f"PASS {name}: {report['odometer_m']:.1f} m, "
        f"{report['gpu_tiles']} GPU tiles, RSS {report['peak_rss_mib']:.0f} MiB",
        flush=True,
    )
    return report


def scenario_service_distance(source, scenario):
    """Measured chainage of the last station on the authored directed path."""
    route = scenario["route"]
    graph = tomllib.loads((source.parent / route["path"] / "track.toml").read_text())
    waypoints = route.get("waypoints")
    if not waypoints or not route.get("stops"):
        raise ValueError("Streaming journey needs directed waypoints and station stops")
    before = 0.0
    stops = route["stops"]
    terminal = stops[-1]
    for start, end in zip(waypoints, waypoints[1:]):
        edges = [e for e in graph["edges"] if e["from"] == start and e["to"] == end]
        if len(edges) > 1:
            node = next((node for node in graph["nodes"] if node["id"] == start), None)
            switch = (node or {}).get("kind", {}).get("switch")
            if switch:
                position = next((item["position"] for item in route.get("switches", [])
                                 if item["node"] == start), switch.get("default_position", "straight"))
                selected = switch["diverging_edge" if position == "diverging" else "stem_edge"]
                edges = [edge for edge in edges if edge["id"] == selected]
        if len(edges) != 1:
            raise ValueError(f"Ambiguous authored path {start} → {end}")
        before += edges[0]["length_m"]
        if end == terminal["node"]:
            return (
                before
                + terminal.get("offset_m", 0.0)
                - route.get("start_offset_m", 0.0)
            )
    raise ValueError("Terminal station is absent from authored waypoints")


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--route-root", type=Path, required=True)
    parser.add_argument(
        "--viewer", type=Path, default=repo / "target/debug/openrailsrs-viewer3d"
    )
    parser.add_argument("--out-dir", type=Path, default=repo / "tmp/viewer-streaming")
    parser.add_argument(
        "--scenario", type=Path, default=repo / "examples/chiltern_local/scenario.toml"
    )
    parser.add_argument(
        "--require-hardware",
        action="store_true",
        help="fail instead of reporting CPU/software rendering as GPU performance",
    )
    parser.add_argument("--software", action="store_true", help="use Vulkan lavapipe")
    parser.add_argument(
        "--headless-wayland",
        action="store_true",
        help="use private Weston with hardware presentation instead of Xvfb",
    )
    parser.add_argument("--checkpoint", choices=["middle", "terminal"], action="append")
    parser.add_argument("--weather", choices=["clear", "rain", "fog", "snow", "overcast", "storm"], default="clear")
    parser.add_argument("--weather-execution", choices=["auto", "gpu", "cpu", "hybrid"], default="auto")
    parser.add_argument("--train-effect-execution", choices=["auto", "gpu", "cpu", "hybrid"], default="auto")
    parser.add_argument("--renderer", choices=["auto", "gpu", "cpu"], default="auto")
    parser.add_argument("--particle-budget", type=int)
    parser.add_argument("--pullman-cab-reference", type=Path,
                        help="same-seat daylight Pullman 3D cab PNG, 1280×720 and 60° FOV")
    parser.add_argument("--view-radius-m", type=int, default=2000)
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
    scenario = tomllib.loads(args.scenario.read_text())
    args.expected_station_names = [stop["name"] for stop in scenario["route"]["stops"]]
    terminal_distance = scenario_service_distance(args.scenario, scenario)
    middle_distance = terminal_distance * 0.525
    for name, target, pause in [
        ("middle", middle_distance, True),
        ("terminal", terminal_distance - 10.0, False),
    ]:
        if name in selected:
            reports[name] = run_checkpoint(args, name, target, pause)
    (args.out_dir / "report.json").write_text(json.dumps(reports, indent=2) + "\n")


if __name__ == "__main__":
    main()
