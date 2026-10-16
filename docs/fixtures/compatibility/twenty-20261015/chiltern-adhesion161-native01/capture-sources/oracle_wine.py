#!/usr/bin/env python3
"""Wine isolation for physics references, never the player's original prefix.

Wine 10.0 upstream sources checked for this harness:
https://github.com/wine-mirror/wine/blob/wine-10.0/programs/winedbg/winedbg.c
https://github.com/wine-mirror/wine/blob/wine-10.0/programs/winedbg/crashdlg.c
https://github.com/wine-mirror/wine/blob/wine-10.0/dlls/winewayland.drv/wayland.c
https://github.com/wine-mirror/wine/blob/wine-10.0/programs/explorer/desktop.c

ShowCrashDialog defaults to true and is read from HKCU\\Software\\Wine\\WineDbg.
Removing DISPLAY alone is insufficient: Wayland calls wl_display_connect(NULL)
and can use the inherited XDG_RUNTIME_DIR/default display. Disable both graphics
drivers and give the process a private runtime directory without display sockets.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BLOCKED_DRIVERS = ("winex11.drv", "winewayland.drv", "winepulse.drv", "winealsa.drv")


def owned_processes(prefix):
    """Only identities with this exact WINEPREFIX, including start ticks."""
    target = b"WINEPREFIX=" + os.fsencode(prefix)
    rows = []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if target not in (proc / "environ").read_bytes().split(b"\0"):
                continue
            stat = (proc / "stat").read_text().rsplit(")", 1)[1].split()
            maps = (proc / "maps").read_text(errors="replace")
            rows.append({"pid": int(proc.name), "start_ticks": int(stat[19]),
                         "comm": (proc / "comm").read_text().strip(),
                         "forbidden_drivers": [d for d in BLOCKED_DRIVERS if d in maps]})
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
    return rows


class OracleWine:
    def __init__(self, prefix, evidence_dir, wineserver=None):
        self.prefix = Path(prefix).resolve()
        marker = self.prefix / ".openrailsrs-private-prefix.json"
        if not marker.is_file():
            raise ValueError("OracleWine requires a cloned, marked private prefix")
        source = Path(json.loads(marker.read_text())["source"]).resolve()
        if self.prefix == source or self.prefix.is_relative_to(source):
            raise ValueError("Refusing to modify or stop the original Wine prefix")
        if not (self.prefix.is_relative_to(ROOT / "tmp") or
                self.prefix.is_relative_to(Path(tempfile.gettempdir()).resolve())):
            raise ValueError("Reference Wine prefixes must be in the task's temporary storage")
        if owned_processes(self.prefix):
            raise ValueError("Private prefix is already in use; do not share running captures")
        self.evidence_dir = Path(evidence_dir).resolve()
        self.evidence_dir.mkdir(parents=True, exist_ok=False)
        runtime = self.prefix / "oracle-private-runtime"
        runtime.mkdir(mode=0o700, exist_ok=True)
        runtime.chmod(0o700)
        if any(runtime.iterdir()):
            raise ValueError("Private runtime must contain no inherited display sockets")
        server = str(wineserver) if wineserver else (shutil.which("wineserver") or shutil.which("wineserver-stable"))
        if not server:
            server = next((str(p) for p in (Path("/usr/lib/x86_64-linux-gnu/wine/wineserver"),
                                            Path("/usr/lib/wine/wineserver")) if p.is_file()), None)
        if not server:
            raise ValueError("Cannot find wineserver for guaranteed private-prefix cleanup")
        self.server = server
        self.env = dict(os.environ)
        for name in ("DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS",
                     "SESSION_MANAGER", "WINELOADER", "WINEDEBUGGER", "PULSE_SERVER"):
            self.env.pop(name, None)
        self.env.update(WINEPREFIX=str(self.prefix), WINEARCH="win64", WINEDEBUG="-all",
                        XDG_RUNTIME_DIR=str(runtime), WAYLAND_DISPLAY="oracle-no-display",
                        XDG_SESSION_TYPE="tty", PULSE_SERVER="unix:/oracle-no-audio",
                        WINEDLLOVERRIDES="winex11.drv,winewayland.drv,winemenubuilder.exe,winepulse.drv,winealsa.drv=d")
        self.commands = []
        self.forbidden_seen = []
        self.observation_count = 0

    def run(self, args, *, cwd, log, timeout=90, check=True):
        started = time.monotonic()
        with Path(log).open("wb") as output:
            proc = subprocess.Popen(args, cwd=cwd, env=self.env, stdin=subprocess.DEVNULL,
                                    stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                while proc.poll() is None:
                    rows = owned_processes(self.prefix)
                    self.observation_count += 1
                    with (self.evidence_dir / "process-observations.jsonl").open("a") as fp:
                        fp.write(json.dumps({"elapsed_s": time.monotonic()-started, "processes": rows}) + "\n")
                    forbidden = [r for r in rows if r["forbidden_drivers"]]
                    if forbidden:
                        self.forbidden_seen.extend(forbidden)
                        raise RuntimeError("A disabled desktop/audio driver was mapped in a private oracle process")
                    if time.monotonic()-started > timeout:
                        raise subprocess.TimeoutExpired(args, timeout)
                    time.sleep(0.05)
            except BaseException:
                self.stop()
                if proc.poll() is None:
                    proc.kill()  # Popen identity is ours; never a guessed PID.
                proc.wait(timeout=10)
                raise
        row = {"args": list(map(str,args)), "pid": proc.pid, "returncode": proc.returncode,
               "elapsed_s": time.monotonic()-started, "log": str(log)}
        self.commands.append(row)
        if check and proc.returncode:
            raise subprocess.CalledProcessError(proc.returncode, args)
        return subprocess.CompletedProcess(args,proc.returncode)

    def __enter__(self):
        # Drivers are already disabled before the first Wine process is launched.
        try:
            for index, (key, name, kind, value) in enumerate((
                    (r"HKCU\Software\Wine\Drivers", "Graphics", "REG_SZ", "null"),
                    (r"HKCU\Software\Wine\WineDbg", "ShowCrashDialog", "REG_DWORD", "0"))):
                self.run(["wine","reg","add",key,"/v",name,"/t",kind,"/d",value,"/f"],
                         cwd=self.evidence_dir,log=self.evidence_dir/f"registry-set-{index}.log",timeout=45)
            query = self.evidence_dir / "registry-query.log"
            self.run(["wine","reg","query",r"HKCU\Software\Wine\WineDbg","/v","ShowCrashDialog"],
                     cwd=self.evidence_dir,log=query,timeout=30)
            if not re.search(r"ShowCrashDialog\s+REG_DWORD\s+0x0\b",query.read_text(errors="replace")):
                raise RuntimeError("Wine did not confirm ShowCrashDialog=0")
            (self.evidence_dir / "isolation.json").write_text(json.dumps({
                "schema":1,"helper_sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "prefix":str(self.prefix),"original_prefix_touched":False,
                "environment":{k:self.env.get(k) for k in ("DISPLAY","WAYLAND_DISPLAY","XDG_RUNTIME_DIR","XAUTHORITY","DBUS_SESSION_BUS_ADDRESS","WINEDLLOVERRIDES")},
                "show_crash_dialog":0,"graphics_driver":"null","wineserver":self.server,
                "registry_verified":True},indent=2)+"\n")
            return self
        except BaseException:
            self.stop()
            raise

    def stop(self):
        result = subprocess.run([self.server,"-k"],env=self.env,stdin=subprocess.DEVNULL,
                                stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15,check=False)
        deadline = time.monotonic()+3
        while owned_processes(self.prefix) and time.monotonic()<deadline:
            time.sleep(0.1)
        remaining = owned_processes(self.prefix)
        for sig in (signal.SIGTERM,signal.SIGKILL):
            for row in remaining:
                current = next((r for r in owned_processes(self.prefix) if r["pid"]==row["pid"] and r["start_ticks"]==row["start_ticks"]),None)
                if current:
                    try: os.kill(row["pid"],sig)
                    except ProcessLookupError: pass
            if remaining: time.sleep(0.2)
            remaining=owned_processes(self.prefix)
        receipt={"schema":1,"wineserver_stop_returncode":result.returncode,"remaining_processes":remaining,
                 "forbidden_drivers_seen":self.forbidden_seen,"observation_count":self.observation_count,
                 "commands":self.commands,"only_private_prefix":str(self.prefix)}
        (self.evidence_dir / "release.json").write_text(json.dumps(receipt,indent=2)+"\n")
        if remaining:
            raise RuntimeError("Private oracle processes did not terminate")

    def __exit__(self, *_):
        self.stop()


def smoke_main():
    parser=argparse.ArgumentParser(description="Controlled crash in an already cloned private Wine prefix")
    parser.add_argument("--prefix",type=Path,required=True)
    parser.add_argument("--out",type=Path,required=True)
    args=parser.parse_args()
    out=args.out.resolve();out.mkdir(parents=True,exist_ok=False)
    source=out/"ExpectedOracleCrash.cs"
    source.write_text('using System; class ExpectedOracleCrash { static void Main() { throw new InvalidOperationException("EXPECTED_ORACLE_ISOLATION_FAILURE"); } }\n')
    from capture_chiltern_service_or import windows_path
    with OracleWine(args.prefix,out/"wine-isolation") as wine:
        wine.run(["wine",r"C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe","/nologo","/out:ExpectedOracleCrash.exe",windows_path(source)],
                 cwd=out,log=out/"compile.log",timeout=60)
        result=wine.run(["wine","ExpectedOracleCrash.exe"],cwd=out,log=out/"expected-crash.log",timeout=30,check=False)
        if result.returncode==0 or "EXPECTED_ORACLE_ISOLATION_FAILURE" not in (out/"expected-crash.log").read_text(errors="replace"):
            raise RuntimeError("Expected controlled failure did not occur")
    release=json.loads((out/"wine-isolation/release.json").read_text())
    if release["remaining_processes"] or release["forbidden_drivers_seen"] or release["observation_count"]==0:
        raise RuntimeError("Private Wine isolation check failed")
    (out/"result.json").write_text(json.dumps({"status":"PASS_CONTROLLED_CRASH_ISOLATION","failure_returncode":result.returncode,
        "registry_verified":True,"desktop_drivers_disabled_and_not_mapped":True,"remaining_processes":[],
        "scope":"Controlled CLR failure with disconnected desktop environment and native drivers disabled; no user desktop window or renderer was used"},indent=2)+"\n")
    print((out/"result.json").read_text())


if __name__=="__main__":
    smoke_main()
