#!/usr/bin/env python3
"""Capture a frozen station pose using the pinned Open Rails 1.6.1 assemblies.
One private Xvfb display and a 6 GiB RSS guard; the adapter leaves native binaries intact.
Requires an existing Wine prefix/runtime and native activity; never installs Open Rails.
"""
import json
import hashlib
import os
from pathlib import Path
import signal
import subprocess
import time

import argparse
from run_oracles import verify
import tomllib
root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--prefix", type=Path, required=True)
parser.add_argument("--runtime", type=Path, required=True)
parser.add_argument("--activity", type=Path, required=True)
parser.add_argument("--out-dir", type=Path, default=root / "tmp/native-station-views")
parser.add_argument("--label", required=True)
parser.add_argument("--view", choices=["exterior","cab"], default="exterior")
parser.add_argument("--advance-m", type=float, default=0)
parser.add_argument("--platform-id", type=int)
args_config = parser.parse_args()
import math
if not math.isfinite(args_config.advance_m) or args_config.advance_m < 0:
    parser.error("advance must be finite and nonnegative")
if not args_config.label or Path(args_config.label).name != args_config.label:
    parser.error("label must be a filename")
out = args_config.out_dir.resolve()
out.mkdir(parents=True, exist_ok=True)
label, target, view, advance = args_config.label, "", args_config.view, str(args_config.advance_m)
metadata = out / (label + '-metadata.json')
metadata.unlink(missing_ok=True)
(out / (label+'.png')).unlink(missing_ok=True)
prefix = args_config.prefix.resolve()
runtime = args_config.runtime.resolve()
activity = args_config.activity.resolve()
verify(tomllib.loads((root / "oracles/openrails-reference.toml").read_text()), None, runtime)
if not activity.is_file():
    parser.error("native activity is missing")
read_fd, write_fd = os.pipe()
viewer = None
with (out / (label + '-xvfb.log')).open('w') as xlog:
    display = subprocess.Popen(['Xvfb', '-displayfd', str(write_fd), '-screen', '0', '1280x720x24', '-ac', '-nolisten', 'tcp'], pass_fds=(write_fd,), stdout=xlog, stderr=xlog)
    os.close(write_fd)
    try:
        number = os.read(read_fd, 32).decode().strip()
        if not number:
            raise RuntimeError('Xvfb failed')
        env = {k:v for k,v in os.environ.items() if not k.startswith('OPENRAILS_REFERENCE_')}
        if target == '-': target = ''
        env.pop('WAYLAND_DISPLAY', None)
        env.update(OPENRAILS_REFERENCE_TARGET=target, OPENRAILS_REFERENCE_VIEW=view, OPENRAILS_REFERENCE_ADVANCE_M=advance, OPENRAILS_REFERENCE_METADATA='Z:' + str(metadata).replace('/', '\\'), DISPLAY=':' + number, WINEPREFIX=str(prefix), WINEARCH='win64', WINEDEBUG='-all', WINEDLLOVERRIDES='d3d11,dxgi=b', LIBGL_ALWAYS_SOFTWARE='1', GALLIUM_DRIVER='llvmpipe', __GLX_VENDOR_LIBRARY_NAME='mesa')
        if args_config.platform_id is not None:
            env["OPENRAILS_REFERENCE_PLATFORM"] = str(args_config.platform_id)
        compiler = ['wine', 'C:\\windows\\Microsoft.NET\\Framework64\\v4.0.30319\\csc.exe', '/nologo', '/out:SceneryLowCapture.exe'] + ['/r:' + name for name in ['RunActivity.exe', 'Orts.Settings.dll', 'Orts.Simulation.dll', 'Orts.Common.dll', 'Orts.Formats.Msts.dll', 'MonoGame.Framework.dll', 'Newtonsoft.Json.dll', 'System.Windows.Forms.dll']] + ['Z:' + str(root / 'tools/openrails-reference/ExtendedSceneryCapture.cs').replace('/', '\\')]
        subprocess.run(compiler, cwd=runtime, env=env, check=True, timeout=60)
        args = ['wine', str(runtime / 'SceneryLowCapture.exe'), '-start', 'Z:' + str(activity).replace('/', '\\'), '-skip-user-settings', '-FullScreen=true', '-WindowSize=1280x720', '-ViewingDistance=450', '-DistantMountains=false', '-AntiAliasing=1', '-SoundVolumePercent=0', '-PauseOnFocusLost=false']
        (out / (label + '-command.json')).write_text(json.dumps(args, indent=2) + '\n')
        with (out / (label + '-adapter.log')).open('w') as log:
            viewer = subprocess.Popen(args, cwd=runtime, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            start = time.monotonic()
            peak_kib = 0
            ready_at = None
            while viewer.poll() is None and time.monotonic() - start < 180:
                if 'Unhandled Exception:' in (out / (label + '-adapter.log')).read_text(errors='replace'):
                    raise RuntimeError('Native adapter failed; see its log')
                process_list = subprocess.check_output(['ps', '-eo', 'pgid=,rss='], text=True)
                rss = sum(int(row.split()[1]) for row in process_list.splitlines() if row.split()[0] == str(viewer.pid))
                peak_kib = max(peak_kib, rss)
                if rss > 6 * 1024 * 1024:
                    raise RuntimeError('Native renderer exceeded 6 GiB RAM')
                capture_metadata = metadata
                if capture_metadata.is_file():
                    ready_at = ready_at or time.monotonic()
                if ready_at and time.monotonic() - ready_at > 8:
                    break
                time.sleep(0.5)
            if ready_at is None:
                raise RuntimeError('Native capture never became ready')
            if viewer.poll() is not None:
                raise RuntimeError('Native viewer exited before the screenshot')
            subprocess.run(['ffmpeg', '-y', '-v', 'error', '-f', 'x11grab', '-video_size', '1280x720', '-i', ':' + number, '-frames:v', '1', str(out / (label + '.png'))], check=True, env=env, timeout=15)
            native = json.loads(metadata.read_text())
            native.update(
                png_sha256=hashlib.sha256((out / (label + '.png')).read_bytes()).hexdigest(),
                adapter_sha256=hashlib.sha256((root / 'tools/openrails-reference/ExtendedSceneryCapture.cs').read_bytes()).hexdigest(),
                peak_rss_mib=round(peak_kib / 1024),
            )
            metadata.write_text(json.dumps(native, indent=2) + '\n')
            print(json.dumps({'exit': viewer.poll(), 'peak_mib': round(peak_kib / 1024), 'elapsed_s': round(time.monotonic() - start)}), flush=True)
    finally:
        os.close(read_fd)
        if viewer is not None:
            try:
                os.killpg(viewer.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                viewer.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(viewer.pid, signal.SIGKILL)
                viewer.wait()
        display.terminate()
        display.wait(timeout=10)

verify(tomllib.loads((root / "oracles/openrails-reference.toml").read_text()), None, runtime)
