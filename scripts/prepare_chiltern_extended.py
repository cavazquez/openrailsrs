#!/usr/bin/env python3
"""Six native stations, measured PAT/TDB corridor and the existing live traffic."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import tomllib
import prepare_chiltern_service as native

PAT = native.PAT
STOPS = [
    ("Northolt Park", 1286, 0.0, 20.0),
    ("South Ruislip", 1290, 480.0, 30.0),
    ("West Ruislip", 1080, 840.0, 30.0),
    ("Denham", 1864, 1200.0, 30.0),
    ("Denham Golf Course", 1299, 1410.0, 30.0),
    ("Gerrards Cross", 1303, 1800.0, 30.0),
]


def table(v):
    if isinstance(v, dict):
        return "{ " + ", ".join(f"{k} = {table(x)}" for k, x in v.items()) + " }"
    if isinstance(v, list):
        return "[" + ", ".join(map(table, v)) + "]"
    return json.dumps(v, ensure_ascii=False)


def fields(values):
    return "\n".join(f"{k} = {table(v)}" for k, v in values.items()) + "\n"


def prepare_brake_profiles(route, output):
    """Keep frozen replay fixtures; import stock brake tokens into this service.

    The small physics fixtures retain their trainset folder and shape names so
    Bevy still resolves the original cab, exterior and effects from Content.
    """
    original_stock = route.parent.parent / "TRAINS/TRAINSET/RF_Blue_Pullman"
    con = native.ROOT / "examples/chiltern/consists/birmingham_pullman.con"
    tokens = (
        "BrakeSystemType",
        "BrakeCylinderPressureForMaxBrakeBrakeForce",
        "MaxApplicationRate",
        "MaxReleaseRate",
    )
    profiles = []
    for relative in re.findall(r'"(trains/[^\"]+\.(?:eng|wag))"', con.read_text()):
        source = original_stock / Path(relative).name
        text = native.msts_text(source)
        values = {}
        for token in tokens:
            matches = re.findall(r"\b" + token + r"\s*\(([^)]*)\)", text, re.I)
            if len(matches) != 1:
                raise ValueError(f"Missing or ambiguous {token} in {source}")
            values[token] = matches[0].strip()
        fixture = (native.ROOT / "examples/chiltern" / relative).read_text()
        closing = fixture.rfind(")")
        if closing < 0:
            raise ValueError(f"Invalid physics fixture {relative}")
        fixture = (
            fixture[:closing]
            + "".join(f"  ({token} {value})\n" for token, value in values.items())
            + fixture[closing:]
        )
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(
            "\n".join(line.rstrip() for line in fixture.splitlines()) + "\n"
        )
        profiles.append(
            dict(
                file=source.name,
                sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                tokens=values,
            )
        )
    destination = output / "consists/birmingham_pullman.con"
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(con.read_text())
    return profiles


def prepare(route, imported, output):
    graph = tomllib.loads(imported.read_text())
    script_path = route / "sigscr.dat"
    script_text = native.msts_text(script_path)
    scripts = {m[1].casefold(): m[2] for m in re.finditer(r"^SCRIPT\s+(\w+)\s*\n(.*?)(?=^SCRIPT|\Z)", script_text, re.M | re.S)}
    # Balanced STF blocks preserve UTF-16 content and original signal direction.
    signal_types = {}
    tdb_text = native.msts_text(route / "Chiltern.tdb")
    for body in re.split(r"\n\s*SignalItem\s*\(", tdb_text)[1:]:
        body = body.split("\n\t\t)")[0]
        item = re.search(r"TrItemId\s*\(\s*(\d+)", body)
        signal = re.search(r'TrSignalType\s*\(\s*\w+\s+(\d+)\s+\S+\s+"?([^\s)"]+)', body)
        if item and signal:
            signal_types[int(item[1])] = (int(signal[1]), signal[2])
    cfg = native.msts_text(route / "sigcfg.dat")
    functions = {}
    for name in set(name for _, name in signal_types.values()):
        block = re.search(r'SignalType\s*\(\s*"?' + re.escape(name) + r'"?\s+(.*?)(?=\n\s*SignalType\s*\(|\Z)', cfg, re.S | re.I)
        function = re.search(r"SignalFnType\s*\(\s*(\w+)", block[1]) if block else None
        if function:
            functions[name.casefold()] = function[1]

    polylines = native.native_polylines(route, graph)
    pat = route / "PATHS" / PAT
    pdps = [
        list(map(float, v.split()))
        for v in re.findall(r"TrackPDP\s*\(([^)]+)\)", native.msts_text(pat))
    ]
    points = [(p[0] * 2048 + p[2], -(p[1] * 2048 + p[4])) for p in pdps]
    by_id = {e["id"]: e for e in graph["edges"]}

    def end_edge(a, b):
        return by_id[
            native.nearest(a, graph["edges"], polylines, (b[0] - a[0], b[1] - a[1]))[0]
        ]

    first = end_edge(points[0], points[1])
    last = by_id[
        native.nearest(
            points[-1],
            graph["edges"],
            polylines,
            (points[-1][0] - points[-2][0], points[-1][1] - points[-2][1]),
        )[0]
    ]
    path = (
        [first]
        + native.shortest_path(graph["edges"], first["to"], last["from"])
        + [last]
    )
    approach = next(
        e
        for e in graph["edges"]
        if e["id"].removesuffix("_r") == f"e{native.APPROACH_VECTOR}"
        and e["to"] == first["from"]
    )
    path.insert(0, approach)
    original_markers = native.native_station_markers(route, path, STOPS[:3])
    west = next(
        i for i, e in enumerate(path) if e["id"] == original_markers[-1]["edge_id"]
    )
    path = path[: west + 1]
    # The local PAT ends at West Ruislip. Continue over native connected mainline
    # vectors to the departure ends of the three additional TDB platforms.
    # Exclude the travelled corridor so Dijkstra cannot take an immediate U-turn.
    for edge_id in ("e14606_r", "e14603_r", "e14601_r"):
        blocked = {e["id"].removesuffix("_r") for e in path}
        available = [
            e for e in graph["edges"] if e["id"].removesuffix("_r") not in blocked
        ]
        target = by_id[edge_id]
        path += native.shortest_path(available, path[-1]["to"], target["from"]) + [
            target
        ]
    markers = native.native_station_markers(route, path, STOPS)
    if len({e["id"] for e in path}) != len(path):
        raise ValueError("Repeated route edge")
    offsets = {}
    total = 0.0
    for edge in path:
        offsets[edge["id"]] = total
        total += edge["length_m"]
    start = offsets[markers[0]["edge_id"]] + markers[0]["chainage_m"]
    stops = []
    for i, marker in enumerate(markers):
        edge = by_id[marker["edge_id"]]
        chain = offsets[edge["id"]] + marker["chainage_m"]
        stops.append(
            dict(
                node=edge["to"],
                name=marker["station"],
                offset_m=marker["chainage_m"] - edge["length_m"],
                arrive_s=STOPS[i][2],
                depart_s=STOPS[i][2] + STOPS[i][3],
                dwell_s=STOPS[i][3],
                passengers_on=20 if i < len(markers) - 1 else 0,
                passengers_off=100 if i == len(markers) - 1 else 0,
            )
        )
        print(
            f"{marker['station']}: {chain - start:.1f} m · {edge['id']} · native {marker['item_id']}"
        )
    # Keep the contiguous original traffic corridor and its opposite service.
    traffic = tomllib.loads(
        (native.ROOT / "examples/chiltern_traffic/track.toml").read_text()
    )
    edges = {e["id"]: e for e in traffic["edges"]}
    for edge in path:
        edges[edge["id"]] = {
            **edge,
            "speed_limit_kmh": min(edge["speed_limit_kmh"], 80.0),
        }
        reverse = (
            edge["id"].removesuffix("_r")
            if edge["id"].endswith("_r")
            else edge["id"] + "_r"
        )
        edges[reverse] = {
            **by_id[reverse],
            "speed_limit_kmh": min(by_id[reverse]["speed_limit_kmh"], 80.0),
        }
    node_ids = {e[k] for e in edges.values() for k in ("from", "to")}
    track = '[route]\nid = "chiltern_extended"\n'
    for node in graph["nodes"]:
        if node["id"] in node_ids:
            track += "\n[[nodes]]\n" + fields(
                {k: node[k] for k in ("id", "x_m", "y_m")}
            )
    for edge in edges.values():
        track += "\n[[edges]]\n" + fields(edge)
    for signal in graph["signals"]:
        for edge in edges.values():
            if edge["id"].removesuffix("_r") == signal["edge_id"]:
                item = int(re.search(r"\d+", signal["id"])[0])
                direction, type_name = signal_types[item]
                if edge["id"].endswith("_r") != bool(direction):
                    continue
                source_script = scripts[type_name.casefold()]
                function = functions[type_name.casefold()]
                position = min(signal["position_m"], edge["length_m"])
                if edge["id"].endswith("_r"):
                    position = edge["length_m"] - position
                track += "\n[[signals]]\n" + fields(
                    dict(
                        id=signal["id"] + ("_r" if edge["id"].endswith("_r") else ""),
                        edge_id=edge["id"],
                        position_m=position,
                        aspect="clear",
                    )
                )
                track += '[signals.script]\n'
                track += '[signals.script.native]\n' + fields(dict(name=type_name, function=function, source=source_script))
    for alias in graph["msts_aliases"]:
        if alias["id"] in node_ids or alias["id"] in edges:
            track += "\n[[msts_aliases]]\n" + fields(alias)
    for edge_id, profile in sorted(graph.get("edge_profiles", {}).items()):
        if edge_id in edges:
            track += f"\n[edge_profiles.{edge_id}]\n" + fields(profile)
    source = tomllib.loads(
        (native.ROOT / "examples/chiltern_traffic/scenario.toml").read_text()
    )
    scenario = "[scenario]\n" + fields(
        dict(
            name="Chiltern extendido: Northolt Park → Gerrards Cross",
            description="Seis estaciones originales, pendientes, límites por posición y tráfico vivo.",
            start_time_s=35700,
            season="summer",
        )
    )
    scenario += "\n[route]\n" + fields(
        dict(
            path=".",
            start=path[0]["from"],
            destination=path[-1]["to"],
            start_offset_m=start,
            assume_signals_clear=False,
            waypoints=[path[0]["from"]] + [e["to"] for e in path],
        )
    )
    for stop in stops:
        scenario += "\n[[route.stops]]\n" + fields(stop)
    for group in ["train", "gameplay", "simulation", "output"]:
        values = source[group].copy()
        if group == "simulation":
            values["duration"] = 3600.0
        if group == "train":
            values["consist"] = "consists/birmingham_pullman.con"
        scenario += f"\n[{group}]\n" + fields(values)
    for service in source["extra_trains"]:
        service = {**service, "consist": "consists/birmingham_pullman.con"}
        scenario += "\n[[extra_trains]]\n" + fields(
            {k: v for k, v in service.items() if k != "stops"}
        )
        for stop in service["stops"]:
            scenario += "\n[[extra_trains.stops]]\n" + fields(stop)
    output.mkdir(parents=True, exist_ok=True)
    (output / "track.toml").write_text(track)
    (output / "scenario.toml").write_text(scenario)
    length = offsets[markers[-1]["edge_id"]] + markers[-1]["chainage_m"] - start
    provenance = dict(
        reference="Open Rails 1.6.1",
        path_file=PAT,
        extension="Native TDB connected mainline via Denham Platform 2, Denham Golf Course Platform 2 and Gerrards Cross Platform 1",
        pat_sha256=hashlib.sha256(pat.read_bytes()).hexdigest(),
        tdb_sha256=hashlib.sha256((route / "Chiltern.tdb").read_bytes()).hexdigest(),
        service_length_m=length,
        start_chainage_m=start,
        station_markers=markers,
        physical_profiles=True,
        brake_profiles=prepare_brake_profiles(route, output),
        signal_policy="original directed Chiltern SIGSCR programs; normal/distant blocks on the service path",
        sigscr_sha256=hashlib.sha256(script_path.read_bytes()).hexdigest(),
        sigcfg_sha256=hashlib.sha256((route / "sigcfg.dat").read_bytes()).hexdigest(),
        traffic="existing lead and adjacent opposite services",
    )
    (output / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    return provenance


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--route-root", type=Path, required=True)
    p.add_argument("--imported-track", type=Path, required=True)
    p.add_argument(
        "--out-dir", type=Path, default=native.ROOT / "examples/chiltern_extended"
    )
    a = p.parse_args()
    prepare(a.route_root, a.imported_track, a.out_dir)
