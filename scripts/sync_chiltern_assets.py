#!/usr/bin/env python3
"""Extract physics-only rolling stock for examples/chiltern (no cab/C# scripts)."""
from __future__ import annotations

import re
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


def write_eng(path: Path, name: str, text: str) -> None:
    mass = parse_mass(text)
    length = parse_length(text)
    force = parse_force_lbf(text, ["MaxForce", "MaxTractiveEffort"], 12000 * LBF)
    brake = parse_force_lbf(
        text, ["ORTSMaxBrakeShoeForce", "MaxBrakeForce"], 70_000.0
    )
    power = parse_max_power(text) * 0.1  # OR diesel table effective fraction
    vmax = parse_max_velocity_mps(text)
    body = f'''(Engine
  (Name "{name}")
  (Mass {mass:.0f})
  (MaxPower {power:.0f})
  (MaxForce {force:.0f})
  (MaxVelocity {vmax * 2.2369362921:.1f})
  (MaxBrakeForce {brake:.0f})
  (Length {length:.3f})
)
'''
    path.write_text(body, encoding="utf-8")
    print(f"  eng {path.name}: {mass/1000:.0f}t, {power/1000:.0f}kW, {length:.1f}m")


def write_wag(path: Path, name: str, text: str) -> None:
    mass = parse_mass(text)
    length = parse_length(text, 20.71)
    brake = parse_force_lbf(
        text, ["ORTSMaxBrakeShoeForce", "MaxBrakeForce"], 60_000.0
    )
    body = f'''(Wagon
  (Type "{name}")
  (Mass {mass:.0f})
  (MaxBrakeForce {brake:.0f})
  (Length {length:.3f})
)
'''
    path.write_text(body, encoding="utf-8")
    print(f"  wag {path.name}: {mass/1000:.0f}t, {length:.1f}m")


def main() -> int:
    src = Path(
        sys.argv[1]
        if len(sys.argv) > 1
        else Path.home()
        / "Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman"
    )
    dest = Path(__file__).resolve().parents[1] / "examples/chiltern/trains/RF_Blue_Pullman"
    dest.mkdir(parents=True, exist_ok=True)

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
    for fname, writer in mapping.items():
        src_path = src / fname
        if not src_path.exists():
            print(f"  skip missing {fname}", file=sys.stderr)
            continue
        text = read_msts(src_path)
        name = fname.rsplit(".", 1)[0]
        writer(dest / fname, name, text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
