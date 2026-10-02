#!/usr/bin/env python3
"""Build a small, reproducible service from Chiltern's native PAT/TDB data.

The Rust importer resolves physical lengths through tsection.dat. Native TDB
section anchors are used to snap stops onto the correct track, rather than onto
the straight chord between distant junctions. Bulk scenery stays in Content.
"""

import argparse
import hashlib
import heapq
import json
import math
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PAT = "RS_Maryleb-WRuislip0955.pat"


def msts_text(path):
    data = path.read_bytes()
    return data.decode("utf-16") if data[:2] in (b"\xff\xfe", b"\xfe\xff") else data.decode("cp1252")


def native_polylines(route_root, graph):
    text = msts_text(route_root / "Chiltern.tdb")
    anchors = {}
    pattern = r"TrackNode\s*\(\s*(\d+)\s+TrVectorNode\s*\(\s*TrVectorSections\s*\(\s*([^)]*)\)"
    for match in re.finditer(pattern, text):
        numbers = match[2].split()
        count = int(numbers[0])
        if len(numbers) != 1 + 16 * count:
            raise ValueError(f"Unsupported native vector layout at TDB {match[1]}")
        points = []
        for index in range(count):
            record = list(map(float, numbers[1 + 16 * index:1 + 16 * (index + 1)]))
            points.append((record[8] * 2048 + record[10], -(record[9] * 2048 + record[12])))
        anchors["e" + match[1]] = points
    nodes = {node["id"]: node for node in graph["nodes"]}
    polylines = {}
    for edge in graph["edges"]:
        forward = edge["id"].removesuffix("_r")
        points = list(anchors.get(forward, []))
        if not points:
            continue
        if edge["id"].endswith("_r"):
            points.reverse()
        start, end = nodes[edge["from"]], nodes[edge["to"]]
        # UiD endpoints include the final section, absent from the anchor list.
        points = [(start["x_m"], start["y_m"])] + points + [(end["x_m"], end["y_m"])]
        polylines[edge["id"]] = points
    return polylines


def project(point, points):
    lengths = [math.dist(a, b) for a, b in zip(points, points[1:])]
    total = sum(lengths)
    before = 0.0
    best = (math.inf, 0.0, (0.0, 0.0))
    for a, b, length in zip(points, points[1:], lengths):
        if length < 1e-6:
            continue
        dx, dy = b[0] - a[0], b[1] - a[1]
        fraction = max(0.0, min(1.0, ((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / length**2))
        distance = math.dist(point, (a[0] + fraction * dx, a[1] + fraction * dy))
        if distance < best[0]:
            best = (distance, (before + fraction * length) / total, (dx / length, dy / length))
        before += length
    return best


def nearest(point, edges, polylines, direction=None):
    candidates = []
    for edge in edges:
        if edge["id"] not in polylines:
            continue
        distance, fraction, tangent = project(point, polylines[edge["id"]])
        if direction and tangent[0] * direction[0] + tangent[1] * direction[1] <= 0:
            continue
        candidates.append((distance, edge["id"], fraction))
    distance, edge_id, fraction = min(candidates)
    if distance > 10.0:
        raise ValueError(f"PAT point is {distance:.1f} m off its nearest directed track")
    return edge_id, fraction, distance


def shortest_path(edges, start, end):
    outgoing = {}
    for edge in edges:
        outgoing.setdefault(edge["from"], []).append(edge)
    distances, parent, queue = {start: 0.0}, {}, [(0.0, start)]
    while queue:
        distance, node = heapq.heappop(queue)
        if distance != distances[node]:
            continue
        if node == end:
            result = []
            while node != start:
                edge = parent[node]
                result.append(edge)
                node = edge["from"]
            return result[::-1]
        for edge in outgoing.get(node, []):
            new = distance + edge["length_m"]
            if new < distances.get(edge["to"], math.inf):
                distances[edge["to"]] = new
                parent[edge["to"]] = edge
                heapq.heappush(queue, (new, edge["to"]))
    raise ValueError("No path between the native PAT endpoints")


def fields(values):
    return "\n".join(f"{key} = {json.dumps(value, ensure_ascii=False)}" for key, value in values.items()) + "\n"


def prepare(route_root, imported_track, output):
    graph = tomllib.loads(imported_track.read_text())
    polylines = native_polylines(route_root, graph)
    pat_path = route_root / "PATHS" / PAT
    pdps = [list(map(float, value.split())) for value in re.findall(r"TrackPDP\s*\(([^)]+)\)", msts_text(pat_path))]
    points = [(p[0] * 2048 + p[2], -(p[1] * 2048 + p[4])) for p in pdps]
    direction = (points[1][0] - points[0][0], points[1][1] - points[0][1])
    by_id = {edge["id"]: edge for edge in graph["edges"]}
    first_id, first_fraction, _ = nearest(points[0], graph["edges"], polylines, direction)
    last_direction = (points[-1][0] - points[-2][0], points[-1][1] - points[-2][1])
    last_id, _, _ = nearest(points[-1], graph["edges"], polylines, last_direction)
    first, last = by_id[first_id], by_id[last_id]
    path = [first] + shortest_path(graph["edges"], first["to"], last["from"]) + [last]
    if len({edge["id"] for edge in path}) != len(path):
        raise ValueError("Service route contains a repeated edge")
    total = sum(edge["length_m"] for edge in path)
    start_offset = first_fraction * first["length_m"]
    if not 3000 < total - start_offset < 8000:
        raise ValueError(f"Unexpected corridor length: {total - start_offset:.1f} m")
    chainages, before = {}, 0.0
    for edge in path:
        chainages[edge["id"]] = before
        before += edge["length_m"]
    stops = []
    for name, point, arrival, dwell in [("Northolt Park", points[0], 0.0, 20.0),
            ("South Ruislip", points[3], 360.0, 30.0), ("West Ruislip", points[6], 720.0, 30.0)]:
        edge_id, fraction, error = nearest(point, path, polylines)
        edge = by_id[edge_id]
        # PAT points are used as head stop markers for this explicitly authored
        # local service. Its departure point is not an OR activity rear spawn.
        target = chainages[edge_id] + fraction * edge["length_m"]
        stops.append(dict(node=edge["to"], name=name, offset_m=-(1.0-fraction)*edge["length_m"],
            arrive_s=arrival, depart_s=arrival+dwell, dwell_s=dwell, passengers_on=20 if len(stops)<2 else 0,
            passengers_off=40 if len(stops)==2 else 0))
        print(f"{name}: {target - start_offset:.1f} m · snap error {error:.3f} m · {edge_id}")
    # Terminal node remains the incoming native edge endpoint; the stopping
    # offset ends the service at the station before the final PAT exit point.
    terminal = stops[-1]["node"]
    end_index = next(i for i, edge in enumerate(path) if edge["to"] == terminal)
    path = path[:end_index+1]
    node_ids = {edge["from"] for edge in path} | {edge["to"] for edge in path}
    output.mkdir(parents=True, exist_ok=True)
    track = "# Generated from native Chiltern PAT/TDB + tsection.dat.\n[route]\nid = \"chiltern_local\"\n"
    for node in graph["nodes"]:
        if node["id"] in node_ids:
            track += "\n[[nodes]]\n" + fields({key: node[key] for key in ("id", "x_m", "y_m")})
    for edge in path:
        track += "\n[[edges]]\n" + fields({**edge, "speed_limit_kmh": min(edge["speed_limit_kmh"], 65.0)})
    for signal in graph["signals"]:
        original = by_id[signal["edge_id"]]
        selected = next((edge for edge in path if edge["id"].removesuffix("_r") == original["id"]), None)
        if selected:
            position = min(signal["position_m"], selected["length_m"])
            if selected["id"].endswith("_r"):
                position = selected["length_m"] - position
            track += "\n[[signals]]\n" + fields(dict(id=signal["id"], edge_id=selected["id"], position_m=position, aspect="clear"))
            track += "[signals.script]\non_block_ahead = \"stop\"\non_second_block_ahead = \"caution\"\ndefault = \"clear\"\n"
    for alias in graph["msts_aliases"]:
        if alias["id"] in node_ids or alias["id"] in {edge["id"].removesuffix("_r") for edge in path}:
            track += "\n[[msts_aliases]]\n" + fields(alias)
    (output / "track.toml").write_text(track)
    scenario = "[scenario]\n" + fields(dict(name="Chiltern local: Northolt Park → West Ruislip", description="Servicio local con Pullman: detenerse, abrir puertas, cumplir la parada y cerrar puertas.", start_time_s=35700))
    scenario += "\n[route]\n" + fields(dict(path=".", start=path[0]["from"], destination=terminal, start_offset_m=start_offset, assume_signals_clear=False,
        waypoints=[path[0]["from"]] + [edge["to"] for edge in path]))
    for stop in stops:
        scenario += "\n[[route.stops]]\n" + fields(stop)
    scenario += "\n[train]\nconsist = \"../chiltern/consists/birmingham_pullman.con\"\n"
    scenario += "\n[gameplay]\nobjective = \"arrive_on_time\"\ndifficulty = \"normal\"\npenalty_per_second_late = 2.0\n"
    scenario += "\n[simulation]\nduration = 1800.0\ntime_step = 0.05\nseed = 42\nmulti_body = true\ncoupler_kind = \"pullman\"\nlegacy_power_cap = false\ntrain_air_lap_hold = true\ntrain_air_full_release_s = 3.0\nbrake_shoe_speed_factor = true\nbrake_skid_limit = true\nbrake_cylinder_full_scale_psi = 35.0\n"
    scenario += "\n[output]\ncsv = \"run.csv\"\nmetadata = \"run.json\"\n"
    (output / "scenario.toml").write_text(scenario)
    provenance = dict(path_file=PAT, pat_sha256=hashlib.sha256(pat_path.read_bytes()).hexdigest(),
        tdb_sha256=hashlib.sha256((route_root/"Chiltern.tdb").read_bytes()).hexdigest(),
        physical_path_length_m=sum(edge["length_m"] for edge in path), start_chainage_m=start_offset,
        station_points=[0, 3, 6], signal_policy="single train, three aspect occupancy script; source SIGSCR not translated")
    provenance["tsection_sha256"] = {
        str(path.relative_to(route_root.parent.parent)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in [route_root.parent.parent/"GLOBAL/tsection.dat", route_root/"tsection.dat", route_root/"OpenRails/tsection.dat"]
        if path.is_file()
    }
    (output / "provenance.json").write_text(json.dumps(provenance, indent=2)+"\n")
    print(output / "scenario.toml")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--route-root", type=Path, default=Path.home()/"Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern")
    parser.add_argument("--out-dir", type=Path, default=ROOT/"examples/chiltern_local")
    parser.add_argument("--imported-track", type=Path)
    parser.add_argument("--binary", type=Path, default=ROOT/"target/debug/openrailsrs")
    args = parser.parse_args()
    if args.imported_track:
        prepare(args.route_root, args.imported_track, args.out_dir)
    else:
        with tempfile.TemporaryDirectory(prefix="openrailsrs-service-") as temp:
            subprocess.run([str(args.binary), "import-msts", str(args.route_root), "--out-dir", temp], check=True)
            prepare(args.route_root, Path(temp)/"track.toml", args.out_dir)


if __name__ == "__main__":
    main()
