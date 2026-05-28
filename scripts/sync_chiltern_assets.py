#!/usr/bin/env python3
"""Extract physics-only rolling stock for examples/chiltern (no cab/C# scripts).

With ``--with-shapes``, also copies ``WagonShape`` meshes and textures into
``examples/chiltern/trains/<trainset>/SHAPES`` and ``TEXTURES`` for the 3D viewer.
"""
from __future__ import annotations

import argparse
import re
import shutil
import sys
from pathlib import Path

LBF = 4.448_221_615_260_5


def read_msts(path: Path) -> str:
    raw = path.read_bytes()
    if raw.startswith(b"\xff\xfe"):
        return raw[2:].decode("utf-16-le", errors="replace")
    return raw.decode("latin-1", errors="replace")


def first_quantity(pattern: str, text: str) -> str | None:
    m = re.search(pattern, text, re.I)
    return m.group(1).strip() if m else None


def parse_wagon_shape(text: str) -> str | None:
    return first_quantity(r'WagonShape\s*\(\s*"([^"]+)"\s*\)', text) or first_quantity(
        r'WagonShape\s*\(\s*([^\s)]+)\s*\)', text
    )


def parse_mass(text: str) -> float:
    s = first_quantity(r"Mass\s*\(\s*([^)]+)\)", text) or "68000"
    m = re.match(r"([\d.]+)\s*(t-uk|t|kg)?", s)
    if not m:
        return float(s.split()[0])
    v = float(m.group(1))
    unit = (m.group(2) or "kg").lower()
    if unit.startswith("t"):
        return v * 1000.0
    return v


def parse_length(text: str, default: float = 20.0) -> float:
    for pat in [
        r"ORTSLengthCouplerFace\s*\(\s*([^)]+)\)",
        r"Size\s*\(\s*[\d.]+m\s+[\d.]+m\s+([\d.]+m)\s*\)",
    ]:
        s = first_quantity(pat.replace("([^)]+)", "([^)]+)"), text)
        if s:
            m = re.search(r"([\d.]+)\s*m", s)
            if m:
                return float(m.group(1))
            ft = re.search(r"([\d.]+)\s*ft\s+([\d.]+)\s*in", s)
            if ft:
                return float(ft.group(1)) * 0.3048 + float(ft.group(2)) * 0.0254
    m = re.search(r"Size\s*\(\s*[\d.]+m\s+[\d.]+m\s+([\d.]+m)", text)
    if m:
        return float(m.group(1).replace("m", ""))
    return default


def parse_force_lbf(text: str, keys: list[str], default: float) -> float:
    for key in keys:
        s = first_quantity(rf"{key}\s*\(\s*([^)]+)\)", text)
        if not s:
            continue
        m = re.search(r"([\d.]+)\s*lbf", s, re.I)
        if m:
            return float(m.group(1)) * LBF
        m = re.search(r"([\d.]+)\s*kN", s, re.I)
        if m:
            return float(m.group(1)) * 1000.0
        m = re.search(r"^([\d.]+)", s)
        if m:
            return float(m.group(1))
    return default


def parse_max_power(text: str) -> float:
    s = first_quantity(r"MaxPower\s*\(\s*([^)]+)\)", text)
    if s:
        m = re.search(r"([\d.]+)\s*kW", s, re.I)
        if m:
            return float(m.group(1)) * 1000.0
        m = re.search(r"([\d.]+)\s*hp", s, re.I)
        if m:
            return float(m.group(1)) * 745.699872
        m = re.search(r"^([\d.]+)", s)
        if m:
            return float(m.group(1))
    pairs = re.findall(
        r"DieselPowerTab\s*\((.*?)\)\s*DieselConsumptionTab", text, re.S | re.I
    )
    if not pairs:
        return 745_513.0
    nums = [float(x) for x in re.findall(r"[\d.]+", pairs[0])]
    best = 0.0
    for i in range(1, len(nums), 2):
        best = max(best, nums[i])
    return best or 745_513.0


def parse_max_velocity_mps(text: str) -> float:
    s = first_quantity(r"MaxVelocity\s*\(\s*([^)]+)\)", text)
    if not s:
        return 90.0 / 2.2369362921
    m = re.search(r"([\d.]+)\s*mph", s, re.I)
    if m:
        return float(m.group(1)) * 0.44704
    m = re.search(r"^([\d.]+)", s)
    return float(m.group(1)) / 2.2369362921 if m else 40.0


def extract_balanced_parens(text: str, start: int) -> str | None:
    depth = 0
    for i in range(start, len(text)):
        ch = text[i]
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0:
                return text[start : i + 1]
    return None


def extract_orts_curves(text: str) -> str | None:
    key = "ORTSMaxTractiveForceCurves"
    idx = text.find(key)
    if idx < 0:
        return None
    open_idx = text.find("(", idx + len(key))
    if open_idx < 0:
        return None
    inner = extract_balanced_parens(text, open_idx)
    if not inner:
        return None
    return f"  (ORTSMaxTractiveForceCurves {inner})\n"


def parse_continuous_force(text: str) -> float:
    return parse_force_lbf(text, ["MaxContinuousForce", "MaxContinuousTractiveForce"], 0.0)


def extract_numeric_tab(text: str, tab_name: str) -> list[tuple[float, float]]:
    idx = text.find(tab_name)
    if idx < 0:
        return []
    open_idx = text.find("(", idx + len(tab_name))
    if open_idx < 0:
        return []
    inner = extract_balanced_parens(text, open_idx)
    if not inner:
        return []
    nums = [float(x) for x in re.findall(r"[\d.]+", inner)]
    pairs: list[tuple[float, float]] = []
    for i in range(0, len(nums) - 1, 2):
        pairs.append((nums[i], nums[i + 1]))
    return pairs


def extract_diesel_physics_lines(text: str) -> str:
    if "ORTSDieselEngines" not in text:
        return ""
    power = extract_numeric_tab(text, "DieselPowerTab")
    throttle = extract_numeric_tab(text, "ThrottleRPMTab")
    if not power or not throttle:
        return ""
    idx = text.find("ORTSDieselEngines")
    subtree = text[idx : idx + 8000]
    scalars: dict[str, str] = {}
    for key in [
        "IdleRPM",
        "MaxRPM",
        "ChangeUpRPMpS",
        "ChangeDownRPMpS",
        "RateOfChangeUpRPMpSS",
        "RateOfChangeDownRPMpSS",
    ]:
        m = re.search(rf"{key}\s*\(\s*([^)]+)\)", subtree, re.I)
        if m:
            v = re.search(r"([\d.]+)", m.group(1))
            if v:
                scalars[key] = v.group(1)
    lines = ["  (ORTSDieselEngines ( 1"]
    for key in [
        "IdleRPM",
        "MaxRPM",
        "ChangeUpRPMpS",
        "ChangeDownRPMpS",
        "RateOfChangeUpRPMpSS",
        "RateOfChangeDownRPMpSS",
    ]:
        if key in scalars:
            lines.append(f"    ( {key} {scalars[key]} )")
    lines.append("    ( DieselPowerTab (")
    for a, b in power:
        lines.append(f"      {a:g} {b:g}")
    lines.append("    ))")
    lines.append("    ( ThrottleRPMTab (")
    for a, b in throttle:
        lines.append(f"      {a:g} {b:g}")
    lines.append("    ))")
    lines.append("  ))")
    return "\n".join(lines) + "\n"


def extract_davis_lines(text: str) -> str:
    out = ""
    for key in ["ORTSDavis_A", "ORTSDavis_B", "ORTSDavis_C"]:
        m = re.search(rf"{key}\s*\(\s*([^)]+)\)", text, re.I)
        if not m:
            continue
        v = re.search(r"([\d.]+)", m.group(1))
        if v:
            out += f"  ( {key} {v.group(1)} )\n"
    return out


def extract_drive_wheel_line(text: str) -> str:
    m = re.search(r"ORTSDriveWheelWeight\s*\(\s*([^)]+)\)", text, re.I)
    if not m:
        return ""
    return f"  ( ORTSDriveWheelWeight ( {m.group(1).strip()} ) )\n"


def write_eng(path: Path, name: str, text: str, shape_line: str) -> None:
    mass = parse_mass(text)
    length = parse_length(text)
    force = parse_force_lbf(text, ["MaxForce", "MaxTractiveEffort"], 12000 * LBF)
    continuous = parse_continuous_force(text)
    brake = parse_force_lbf(
        text, ["ORTSMaxBrakeShoeForce", "MaxBrakeForce"], 70_000.0
    )
    power = parse_max_power(text)
    vmax = parse_max_velocity_mps(text)
    orts = extract_orts_curves(text) or ""
    diesel = extract_diesel_physics_lines(text)
    davis = extract_davis_lines(text)
    drive_wheel = extract_drive_wheel_line(text)
    extra = ""
    if continuous > 0.0 and not orts:
        extra += f"  (MaxContinuousForce {continuous:.0f})\n"
    body = f'''(Engine
  (Name "{name}")
{shape_line}  (Mass {mass:.0f})
  (MaxPower {power:.0f})
  (MaxForce {force:.0f})
  (MaxVelocity {vmax * 2.2369362921:.1f})
  (MaxBrakeForce {brake:.0f})
  (Length {length:.3f})
{extra}{davis}{drive_wheel}{diesel}{orts})
'''
    path.write_text(body, encoding="utf-8")
    print(f"  eng {path.name}: {mass/1000:.0f}t, {power/1000:.0f}kW, {length:.1f}m")


def write_wag(path: Path, name: str, text: str, shape_line: str) -> None:
    mass = parse_mass(text)
    length = parse_length(text, 20.71)
    brake = parse_force_lbf(
        text, ["ORTSMaxBrakeShoeForce", "MaxBrakeForce"], 60_000.0
    )
    davis = extract_davis_lines(text)
    body = f'''(Wagon
  (Type "{name}")
{shape_line}  (Mass {mass:.0f})
  (MaxBrakeForce {brake:.0f})
  (Length {length:.3f})
{davis})
'''
    path.write_text(body, encoding="utf-8")
    print(f"  wag {path.name}: {mass/1000:.0f}t, {length:.1f}m")


def find_shape_file(src_roots: list[Path], shape_name: str) -> Path | None:
    """Resolve shape path (MSTS often stores .s in trainset root, not SHAPES/)."""
    want = shape_name.lower()

    def match_in_dir(directory: Path) -> Path | None:
        if not directory.is_dir():
            return None
        exact = directory / shape_name
        if exact.is_file():
            return exact
        for entry in directory.iterdir():
            if entry.is_file() and entry.name.lower() == want:
                return entry
        return None

    for root in src_roots:
        for sub in (root / "SHAPES", root / "shapes", root):
            found = match_in_dir(sub)
            if found is not None:
                return found
    return None


def find_texture_file(src_roots: list[Path], tex_name: str) -> Path | None:
    want = tex_name.lower()
    for root in src_roots:
        for sub in (root / "TEXTURES", root / "textures", root):
            if not sub.is_dir():
                continue
            exact = sub / tex_name
            if exact.is_file():
                return exact
            for entry in sub.iterdir():
                if entry.is_file() and entry.name.lower() == want:
                    return entry
    return None


def copy_texture_file(
    tex: Path,
    dest_textures: Path,
    copied_textures: set[str],
) -> None:
    dest_textures.mkdir(parents=True, exist_ok=True)
    out = dest_textures / tex.name
    if tex.name not in copied_textures or not out.exists():
        shutil.copy2(tex, out)
        copied_textures.add(tex.name)


def textures_referenced_in_shape(shape_path: Path) -> set[str]:
    """Best-effort: scan shape bytes for ``*.ace`` / ``*.dds`` names."""
    try:
        raw = shape_path.read_bytes()
    except OSError:
        return set()
    text = raw.decode("latin-1", errors="ignore")
    found: set[str] = set()
    for m in re.finditer(r"([\w.-]+\.(?:ace|dds))\b", text, re.I):
        found.add(m.group(1))
    return found


def copy_shape_assets(
    shape_name: str,
    src_roots: list[Path],
    dest_shapes: Path,
    dest_textures: Path,
    copied_shapes: set[str],
    copied_textures: set[str],
) -> str | None:
    """Copy shape (+ .sd) and referenced textures. Returns canonical filename."""
    src = find_shape_file(src_roots, shape_name)
    if src is None:
        print(f"  shape missing: {shape_name}", file=sys.stderr)
        return None
    canonical = src.name
    dest_shapes.mkdir(parents=True, exist_ok=True)
    dest = dest_shapes / canonical
    if canonical not in copied_shapes or not dest.exists():
        shutil.copy2(src, dest)
        copied_shapes.add(canonical)
    sd = src.with_suffix(".sd")
    if sd.is_file():
        shutil.copy2(sd, dest_shapes / sd.name)
    stem = canonical.rsplit(".", 1)[0]
    for ext in (".ace", ".ACE", ".dds", ".DDS"):
        tex = find_texture_file(src_roots, f"{stem}{ext}")
        if tex is not None:
            copy_texture_file(tex, dest_textures, copied_textures)
    for ref in textures_referenced_in_shape(src):
        tex = find_texture_file(src_roots, ref)
        if tex is not None:
            copy_texture_file(tex, dest_textures, copied_textures)
    return canonical


def copy_trainset_textures(
    src_roots: list[Path],
    dest_textures: Path,
    copied_textures: set[str],
) -> int:
    """Copy all ACE/DDS from trainset root and TEXTURES/ (MSTS often uses root)."""
    n = 0
    for root in src_roots:
        for sub in (root / "TEXTURES", root / "textures", root):
            if not sub.is_dir():
                continue
            for pattern in ("*.ace", "*.ACE", "*.dds", "*.DDS"):
                for tex in sub.glob(pattern):
                    if not tex.is_file():
                        continue
                    copy_texture_file(tex, dest_textures, copied_textures)
                    n += 1
    return n


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "trainset",
        nargs="?",
        type=Path,
        default=Path.home()
        / "Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman",
    )
    parser.add_argument(
        "--route-content",
        type=Path,
        default=None,
        help="Optional MSTS route dir (ROUTES/Chiltern) for route SHAPES/",
    )
    parser.add_argument(
        "--with-shapes",
        action="store_true",
        help="Copy WagonShape .s/.sd and textures into examples/chiltern/trains/RF_Blue_Pullman/",
    )
    args = parser.parse_args()
    src: Path = args.trainset
    repo = Path(__file__).resolve().parents[1]
    dest = repo / "examples/chiltern/trains/RF_Blue_Pullman"
    dest.mkdir(parents=True, exist_ok=True)

    src_roots = [src]
    if args.route_content:
        src_roots.append(args.route_content)

    dest_shapes = dest / "SHAPES"
    dest_textures = dest / "TEXTURES"
    copied_shapes: set[str] = set()
    copied_textures: set[str] = set()

    mapping = {
        "RF_WP_DMBSA.eng": write_eng,
        "RF_WP_DMBSH.eng": write_eng,
        "RF_WP_PSB.wag": write_wag,
        "RF_WP_KFC.wag": write_wag,
        "RF_WP_PCFD.wag": write_wag,
        "RF_WP_PCFE.wag": write_wag,
        "RF_WP_KFF.wag": write_wag,
        "RF_WP_PSG.wag": write_wag,
    }
    print(f"Sync physics assets {src} -> {dest}")
    if args.with_shapes:
        print(f"  shapes -> {dest_shapes}")
        print(f"  textures -> {dest_textures}")

    for fname, writer in mapping.items():
        src_path = src / fname
        if not src_path.exists():
            print(f"  skip missing {fname}", file=sys.stderr)
            continue
        text = read_msts(src_path)
        name = fname.rsplit(".", 1)[0]
        shape_name = parse_wagon_shape(text)
        shape_line = ""
        if shape_name:
            canonical = shape_name
            resolved_path = find_shape_file(src_roots, shape_name)
            if resolved_path is not None:
                canonical = resolved_path.name
            if args.with_shapes:
                copied = copy_shape_assets(
                    shape_name,
                    src_roots,
                    dest_shapes,
                    dest_textures,
                    copied_shapes,
                    copied_textures,
                )
                if copied is not None:
                    canonical = copied
            shape_line = f'  (WagonShape "{canonical}")\n'
        writer(dest / fname, name, text, shape_line)

    if args.with_shapes:
        # Only bulk-copy ACE/DDS from the trainset dir — not route_content (huge).
        copy_trainset_textures([src], dest_textures, copied_textures)
        if copied_shapes or copied_textures:
            print(
                f"  copied {len(copied_shapes)} shape(s), {len(copied_textures)} texture(s)"
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
