#!/usr/bin/env python3
"""Prepare a three-station pilot from installed native PAT/TDB/CON content.

Intended for Belgrano CC, also testable with any installed route. No stock,
station names, scenery or signals are substituted. Original content is read
only; generated service and provenance go to a new project directory.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
import tomllib

import prepare_chiltern_service as native
from prepare_chiltern_extended import fields

ROOT = Path(__file__).resolve().parents[1]

def inspect_installation(route):
    """Report original-file locations even before an incomplete route can be imported."""
    route=Path(route).absolute()
    required=[]
    def files(folder,extension):
        try:directory=resolve(folder)
        except (ValueError,OSError):return []
        return [str(p.resolve()) for p in sorted(directory.iterdir()) if p.is_file() and p.suffix.casefold()==extension]
    tdbs=files(route,'.tdb');paths=files(route/'PATHS','.pat')
    for name in ('sigcfg.dat','sigscr.dat'):
        try:resolve(route/name)
        except (ValueError,OSError):required.append(str(route/name))
    if not tdbs:required.append(str(route/'<nombre-original>.tdb'))
    if not paths:required.append(str(route/'PATHS/<recorrido-original>.pat'))
    trains=route.parent.parent/'TRAINS'
    consists=files(trains/'CONSISTS','.con')
    if not consists:required.append(str(trains/'CONSISTS/<formación-original>.con'))
    try:trainset=resolve(trains/'TRAINSET');has_trainset=trainset.is_dir()
    except (ValueError,OSError):has_trainset=False
    if not has_trainset:required.append(str(trains/'TRAINSET'))
    return dict(route_root=str(route),train_root=str(trains),paths=paths,consists=consists,
        missing_locations=required,ready_to_select=not required,
        guidance='Conservá los nombres y carpetas originales del autor. Esta inspección no certifica la formación ni sustituye la auditoría de sus recursos.')


def resolve(path):
    """Windows-authored file casing, without following ambiguous matches."""
    path = Path(path)
    if path.exists():
        return path.resolve()
    parent = resolve(path.parent)
    matches = [p for p in parent.iterdir() if p.name.casefold() == path.name.casefold()]
    if len(matches) != 1:
        raise ValueError(f"Missing or ambiguous native file: {path}")
    return matches[0].resolve()


def field(block, name):
    values = list(native.msts_blocks(block, name))
    if len(values) != 1:
        raise ValueError(f"Missing/ambiguous {name}")
    return values[0].strip().strip('"')


def main_path_points(text):
    pdps = [list(map(float, b.split())) for b in native.msts_blocks(text, "TrackPDP")]
    nodes = [b.split() for b in native.msts_blocks(text, "TrPathNode")]
    if not nodes or not pdps:
        raise ValueError("Pilot requires native TrackPDP and linked TrPathNode records")
    points, visited, index = [], set(), 0
    while index not in (4294967295, -1):
        if index in visited or not 0 <= index < len(nodes):
            raise ValueError("Broken/cyclic native PAT main path")
        visited.add(index)
        flags, next_main, _siding, pdp = nodes[index]
        if int(flags, 16) & 1:
            raise ValueError("Pilot path contains a reversal; choose a continuous passenger path")
        pdp = int(pdp)
        if not 0 <= pdp < len(pdps) or len(pdps[pdp]) != 7 or int(pdps[pdp][6]) == 9:
            raise ValueError("Broken native PAT point")
        p = pdps[pdp]
        point = (p[0] * 2048 + p[2], -(p[1] * 2048 + p[4]))
        if not all(math.isfinite(v) for v in point):
            raise ValueError("Nonfinite PAT coordinate")
        if not points or math.dist(points[-1], point) > 0.01:
            points.append(point)
        index = int(next_main)
    if len(points) < 2:
        raise ValueError("Native path has fewer than two distinct points")
    return points


def directed_path(graph, polylines, points):
    by_id = {e["id"]: e for e in graph["edges"]}
    first = by_id[native.nearest(points[0], graph["edges"], polylines,
        (points[1][0]-points[0][0], points[1][1]-points[0][1]))[0]]
    last = by_id[native.nearest(points[-1], graph["edges"], polylines,
        (points[-1][0]-points[-2][0], points[-1][1]-points[-2][1]))[0]]
    path = [first] if first["id"] == last["id"] else [first] + native.shortest_path(
        [e for e in graph["edges"] if e["id"].removesuffix("_r") not in
         {first["id"].removesuffix("_r"), last["id"].removesuffix("_r")}],
        first["to"], last["from"]) + [last]
    if len({e["id"].removesuffix("_r") for e in path}) != len(path):
        raise ValueError("Pilot route revisits track")
    # Endpoints alone can pick the wrong branch. Require every linked native
    # main-path PDP to project onto this corridor in increasing chainage.
    offsets, before = {}, 0.0
    for e in path:
        offsets[e["id"]] = before
        before += e["length_m"]
    previous = -math.inf
    for p in points:
        distance, chainage = min((native.project(p, polylines[e["id"]])[0],
            offsets[e["id"]] + native.project(p, polylines[e["id"]])[1]*e["length_m"])
            for e in path)
        if distance > 12 or chainage + 5 < previous:
            raise ValueError("Native PAT branch/order differs from the candidate corridor")
        previous = chainage
    return path


def station_markers(text, path):
    hosts, platforms = {}, {}
    for body in native.msts_blocks(text, "TrackNode"):
        node_id = int(body.split()[0])
        for item in re.findall(r"TrItemRef\s*\(\s*(\d+)\s*\)", body):
            if int(item) in hosts:
                raise ValueError(f"Ambiguous platform host {item}")
            hosts[int(item)] = node_id
    for body in native.msts_blocks(text, "PlatformItem"):
        item = int(field(body, "TrItemId"))
        platforms[item] = dict(item_id=item, station=field(body, "Station"),
            platform=field(body, "PlatformName"), distance_m=float(field(body, "TrItemSData").split()[0]),
            pair=int(field(body, "PlatformTrItemData").split()[1]))
    candidates, offset = [], 0.0
    for e in path:
        for p in platforms.values():
            if hosts.get(p["item_id"]) != int(e["id"].removesuffix("_r")[1:]):
                continue
            q = platforms.get(p["pair"])
            if not q or q["pair"] != p["item_id"] or q["station"] != p["station"] or q["platform"] != p["platform"] or hosts.get(q["item_id"]) != hosts[p["item_id"]]:
                raise ValueError(f"Invalid native platform pair {p['item_id']}")
            for v in (p["distance_m"], q["distance_m"]):
                if not math.isfinite(v) or not 0 <= v <= e["length_m"]:
                    raise ValueError("Platform chainage lies outside its native vector")
            chain = e["length_m"]-p["distance_m"] if e["id"].endswith("_r") else p["distance_m"]
            other = e["length_m"]-q["distance_m"] if e["id"].endswith("_r") else q["distance_m"]
            if chain > other and p["station"]:
                candidates.append(dict(**p, edge_id=e["id"], chainage_m=chain,
                    route_m=offset+chain, platform_length_m=chain-other))
        offset += e["length_m"]
    result, names = [], set()
    for p in sorted(candidates, key=lambda p: p["route_m"]):
        if p["station"].casefold() not in names:
            names.add(p["station"].casefold())
            result.append(p)
    return result


def native_signals(route, tdb, graph, edges):
    selected = {e["id"]: e for e in edges}
    originals = {e["id"]: e for e in graph["edges"]}
    cfg_path = resolve(route / "sigcfg.dat")
    cfg = native.msts_text(cfg_path)
    functions = {}
    for body in native.msts_blocks(cfg, "SignalType"):
        name = re.match(r'\s*("[^"]+"|\S+)', body)[1].strip('"')
        functions[name.casefold()] = field(body, "SignalFnType")
    script_files = list(native.msts_blocks(cfg, "SignalScriptFile"))
    # Native sigcfg names scripts with ScriptFile, usually sigscr.dat.
    script_files += list(native.msts_blocks(cfg, "ScriptFile"))
    sources = [resolve(route / value.strip().strip('"').replace("\\", "/")) for value in script_files]
    if not sources:
        sources = [resolve(route / "sigscr.dat")]
    scripts = {}
    for source in dict.fromkeys(sources):
        scripts.update({m[1].casefold(): m[2] for m in re.finditer(
            r"^\s*SCRIPT\s+(\w+)\s*\n(.*?)(?=^\s*SCRIPT|\Z)", native.msts_text(source), re.M | re.S)})
    types = {}
    for body in native.msts_blocks(tdb, "SignalItem"):
        item = int(field(body, "TrItemId"))
        values = field(body, "TrSignalType").split()
        types[item] = (bool(int(values[1])), values[3].strip('"'))
    result = []
    for signal in graph.get("signals", []):
        item = int(re.search(r"\d+", signal["id"])[0])
        reverse, name = types[item]
        original = originals[signal["edge_id"]]
        edge_id = original["id"].removesuffix("_r") + ("_r" if reverse else "")
        if edge_id not in selected:
            continue
        if name.casefold() not in scripts or name.casefold() not in functions:
            raise ValueError(f"Missing original SIGSCR/function for {name}; no simplified replacement")
        e = selected[edge_id]
        position = signal["position_m"]
        if not 0 <= position <= e["length_m"]:
            raise ValueError("Native signal chainage outside track")
        result.append(dict(id=signal["id"]+("_r" if reverse else ""), edge_id=edge_id,
            position_m=e["length_m"]-position if reverse else position, aspect="clear",
            script=dict(native=dict(name=name, function=functions[name.casefold()], source=scripts[name.casefold()]))))
    return result, [cfg_path, *sources]


def prepare(route, imported, pat, consist, output, cli, origin=None, traffic_consist=None):
    if output.exists() and any(output.iterdir()):
        raise ValueError("Output must be an empty/new directory; original content is read-only")
    tdbs = [p for p in route.iterdir() if p.suffix.casefold() == ".tdb"]
    if len(tdbs) != 1:
        raise ValueError("Expected exactly one native TDB")
    tdb_path = tdbs[0]
    graph = tomllib.loads(imported.read_text())
    path = directed_path(graph, native.native_polylines(route, graph, tdb_path),
                         main_path_points(native.msts_text(pat)))
    markers = station_markers(native.msts_text(tdb_path), path)
    if origin:
        first = next((i for i, m in enumerate(markers) if m["station"].casefold() == origin.casefold()), None)
        if first is None:
            raise ValueError(f"Origin {origin} not on the native path")
        markers = markers[first:]
    markers = markers[:3]
    if len(markers) != 3:
        raise ValueError("Native passenger path needs three distinct stations")
    audits = []
    for con in dict.fromkeys([consist, *([traffic_consist] if traffic_consist else [])]):
        report = json.loads(subprocess.check_output([str(cli), "audit-consists", str(con), "--json"], text=True))
        if not report["formations"][0]["player_ready"]:
            raise ValueError(f"Incomplete/unpowered original formation {con}: {report}")
        audits.append(report["formations"][0]["report"])
    length = audits[0]["length_m"]
    if markers[0]["route_m"] < length:
        raise ValueError("PAT approach is shorter than the consist; choose an earlier-starting path")
    path = path[:next(i for i, e in enumerate(path) if e["id"] == markers[-1]["edge_id"])+1]
    signals, signal_files = native_signals(route, native.msts_text(tdb_path), graph, path)
    node_ids = {e[k] for e in path for k in ("from", "to")}
    edge_ids = {e["id"] for e in path}
    text = '[route]\nid = "native_pilot"\n'
    for section, records in (("nodes", [n for n in graph["nodes"] if n["id"] in node_ids]),
        ("edges", path), ("signals", signals), ("msts_aliases", [a for a in graph.get("msts_aliases", []) if a["id"] in node_ids | edge_ids])):
        for record in records:
            text += f"\n[[{section}]]\n" + fields(record)
    for edge, profile in graph.get("edge_profiles", {}).items():
        if edge in edge_ids:
            text += f"\n[edge_profiles.{edge}]\n" + fields(profile)
    start = markers[0]["route_m"]
    scenario = "[scenario]\n" + fields(dict(name=f"{route.name}: {markers[0]['station']} → {markers[-1]['station']}",
        description="Tres estaciones originales y formación nativa auditada.", start_time_s=43200, season="summer"))
    scenario += "\n[route]\n" + fields(dict(path=".", start=path[0]["from"], destination=path[-1]["to"],
        start_offset_m=start, assume_signals_clear=False, waypoints=[path[0]["from"]]+[e["to"] for e in path]))
    by_id = {e["id"]: e for e in path}
    for index, m in enumerate(markers):
        arrival = (m["route_m"]-start)/10 + index*30
        scenario += "\n[[route.stops]]\n" + fields(dict(node=by_id[m["edge_id"]]["to"], name=m["station"],
            offset_m=m["chainage_m"]-by_id[m["edge_id"]]["length_m"], arrive_s=arrival, depart_s=arrival+30,
            dwell_s=30, passengers_on=20 if index < 2 else 0, passengers_off=40 if index == 2 else 0))
    scenario += "\n[train]\n" + fields(dict(consist=str(consist)))
    scenario += '\n[gameplay]\nobjective = "arrive_on_time"\ndifficulty = "normal"\n'
    scenario += "\n[simulation]\n" + fields(dict(duration=7200, time_step=0.05, seed=42, multi_body=True, legacy_power_cap=False))
    scenario += '\n[output]\ncsv = "run.csv"\nmetadata = "run.json"\n'
    if traffic_consist:
        traffic_length = audits[-1]["length_m"]
        traffic_start = markers[1]["route_m"] + traffic_length + 50
        if traffic_start >= markers[-1]["route_m"] - traffic_length - 50:
            raise ValueError("Not enough native corridor for a safe leading service")
        scenario += "\n[[extra_trains]]\n" + fields(dict(id="Servicio adelantado", consist=str(traffic_consist),
            start=path[0]["from"], destination=path[-1]["to"], waypoints=[path[0]["from"]]+[e["to"] for e in path],
            start_offset_m=traffic_start, start_time_s=0, output_csv="run_traffic.csv"))
        terminal = markers[-1]
        scenario += "\n[[extra_trains.stops]]\n" + fields(dict(node=path[-1]["to"], name=terminal["station"],
            offset_m=terminal["chainage_m"]-path[-1]["length_m"],
            arrive_s=(terminal["route_m"]-traffic_start)/10,
            depart_s=(terminal["route_m"]-traffic_start)/10+30, dwell_s=30))
    sources = [tdb_path, pat, consist, imported, *signal_files, *([traffic_consist] if traffic_consist else [])]
    provenance = dict(reference=tomllib.loads((ROOT/"oracles/openrails-reference.toml").read_text())["version"],
        route_root=str(route), stations=markers, native_consists=audits,
        service_length_m=markers[-1]["route_m"]-start, native_signal_programs=len(signals),
        files={str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in dict.fromkeys(sources)},
        traffic="native leading service on the same directed corridor" if traffic_consist else "none",
        visual_comparison="pending native OR 1.6.1 captures; no parity claim")
    output.mkdir(parents=True, exist_ok=True)
    (output/"track.toml").write_text(text)
    (output/"scenario.toml").write_text(scenario)
    (output/"provenance.json").write_text(json.dumps(provenance, indent=2, ensure_ascii=False)+"\n")
    (output/"native-content.json").write_text(json.dumps(dict(route_root=str(route)), indent=2)+"\n")
    return provenance


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--route-root", type=Path, required=True)
    p.add_argument("--list", action="store_true", help="list installed paths/services; no import or writes")
    p.add_argument("--inspect",action="store_true",help="show incomplete installation paths without importing")
    p.add_argument("--imported-track", type=Path)
    p.add_argument("--path", type=Path)
    p.add_argument("--consist", type=Path)
    p.add_argument("--traffic-consist", type=Path)
    p.add_argument("--origin", help="exact original station name, e.g. Retiro")
    p.add_argument("--cli", type=Path, default=ROOT/"target/debug/openrailsrs")
    p.add_argument("--out-dir", type=Path, default=ROOT/"examples/belgrano_cc")
    a = p.parse_args()
    if a.inspect:
        print(json.dumps(inspect_installation(a.route_root),indent=2,ensure_ascii=False))
        return
    route = resolve(a.route_root)
    if a.list:
        for folder, extension in (("PATHS", ".pat"), ("SERVICES", ".srv")):
            directory = resolve(route/folder)
            for file in sorted(directory.iterdir()):
                if file.suffix.casefold() == extension:
                    print(file)
        return
    if not a.path or not a.consist or not a.imported_track:
        p.error("use --list, then supply --path, --consist and --imported-track")
    report = prepare(route, resolve(a.imported_track), resolve(a.path), resolve(a.consist),
        a.out_dir.resolve(), resolve(a.cli), a.origin,
        resolve(a.traffic_consist) if a.traffic_consist else None)
    print(json.dumps(report, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
