#!/usr/bin/env python3
"""Capture 7 fixed views, compare actual pixels/structure and optionally inject 4 renderer faults.

OPENRAILSRS_* inherited settings are discarded by the shared capture runner.
Only --record writes accepted fixtures; ordinary checks never update tolerances.
"""
import argparse
import hashlib
import json
from pathlib import Path
from types import SimpleNamespace
import shutil
import tomllib

import numpy as np
from PIL import Image, ImageDraw, ImageFilter
from capture_route_views import station_chainages, station_scenario
from check_viewer_streaming import run_checkpoint

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "docs/fixtures/visual/player_goldens"
VIEWS = {
    "cab-front": dict(follow="driver",look_yaw=0,look_pitch=0),
    "cab-up": dict(follow="driver",look_yaw=0,look_pitch=0.55),
    "cab-left": dict(follow="driver",look_yaw=0.7,look_pitch=0),
    "cab-right": dict(follow="driver",look_yaw=-0.7,look_pitch=0),
    "cab-2d": dict(follow="cab2d",look_yaw=0,look_pitch=0),
    "chase": dict(follow="chase",look_yaw=0,look_pitch=0),
    "orbit": dict(follow="orbit",look_yaw=0,look_pitch=0),
}

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def mask(size, region):
    image=Image.new("L",size)
    draw=ImageDraw.Draw(image)
    if "polygon" in region: draw.polygon([tuple(p) for p in region["polygon"]],fill=255)
    else: draw.rectangle(region["box"],fill=255)
    return np.array(image)>0

def edges(image):
    gray=np.asarray(image.convert("L"),dtype=float)
    dx=np.zeros_like(gray); dy=np.zeros_like(gray)
    dx[:,1:-1]=abs(gray[:,2:]-gray[:,:-2])/2
    dy[1:-1,:]=abs(gray[2:,:]-gray[:-2,:])/2
    return (np.hypot(dx,dy)>18)

def dilate(edge):
    return np.asarray(Image.fromarray((edge*255).astype("uint8")).filter(ImageFilter.MaxFilter(5)))>0

def compare(actual_path, golden_path, regions, cross_engine=False):
    with Image.open(actual_path) as a, Image.open(golden_path) as b:
        a=a.convert("RGB"); b=b.convert("RGB")
        if a.size!=b.size: return {"pass":False,"error":"resolution mismatch"}
        av=np.asarray(a,dtype=float); bv=np.asarray(b,dtype=float)
        ae=edges(a); be=edges(b); ad=dilate(ae); bd=dilate(be)
        result={}
        for name,region in regions.items():
            roi=mask(a.size,region)
            train_iou=None
            train_bbox_iou=None
            if region.get("train_mask"):
                def blue(values):
                    return (values[:,:,2]-values[:,:,0]>14)&(values[:,:,2]-values[:,:,1]>5)&(values[:,:,0]<100)&roi
                train_a=blue(av);train_b=blue(bv)
                train_iou=float((train_a&train_b).sum()/max(1,(train_a|train_b).sum()))
                def bbox(values):
                    yy,xx=np.nonzero(values)
                    return [int(xx.min()),int(yy.min()),int(xx.max())+1,int(yy.max())+1] if xx.size else None
                ab,bb=bbox(train_a),bbox(train_b)
                train_bbox_iou=0.0
                if ab and bb:
                    intersection=max(0,min(ab[2],bb[2])-max(ab[0],bb[0]))*max(0,min(ab[3],bb[3])-max(ab[1],bb[1]))
                    union=(ab[2]-ab[0])*(ab[3]-ab[1])+(bb[2]-bb[0])*(bb[3]-bb[1])-intersection
                    train_bbox_iou=intersection/max(1,union)
                # Compare rendered rolling-stock pixels, keeping landscape lighting separate.
                roi=(train_a|train_b)
                if not roi.any(): return {"pass":False,"error":"no train pixels in structural mask"}
            delta=abs(av-bv)[roi]
            mae=float(delta.mean())
            hot=float((delta.max(axis=1)>region.get("rgb_delta",24)).mean())
            ar=ae&roi;br=be&roi
            # Symmetric edge coverage prevents an occluder or extra geometry from winning recall.
            precision=float((ar&bd).sum()/max(1,ar.sum()))
            recall=float((br&ad).sum()/max(1,br.sum()))
            f1=2*precision*recall/max(1e-9,precision+recall)
            result[name]={"rgb_mae":round(mae,4),"hot_fraction":round(hot,4),"edge_f1":round(f1,4),
                "pass":mae<=region.get("mae_max",5) and hot<=region.get("hot_max",0.03) and f1>=region.get("edge_min",0.85) and (train_iou is None or (train_iou>=region.get("train_iou_min",0.55) and train_bbox_iou>=region.get("train_bbox_min",0.0))),
                "pixel_count":int(roi.sum())}
            if train_iou is not None:
                result[name]["train_pixel_iou"]=round(train_iou,4)
                result[name]["train_pixel_bbox_iou"]=round(train_bbox_iou,4)
                result[name]["train_pixel_bbox_actual"]=ab
                result[name]["train_pixel_bbox_reference"]=bb
        return {"pass":all(r["pass"] for r in result.values()),"cross_engine":cross_engine,"regions":result}

def check_fixtures(directory=FIXTURES):
    spec=json.loads((directory/"manifest.json").read_text())
    for name,view in spec["views"].items():
        if digest(directory/(name+".png"))!=view["sha256"]: raise ValueError(f"Golden checksum: {name}")
        if not view["regions"]: raise ValueError(f"Missing pixel masks: {name}")
    for ref in spec["or_references"]:
        if digest(ROOT/ref["path"])!=ref["sha256"]: raise ValueError("Pinned OR reference changed")
    for ref in spec["semantic_sources"]:
        if digest(ROOT/ref["path"])!=ref["sha256"]: raise ValueError("User semantic reference changed")
    return spec

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--route-root",required=True,type=Path)
    p.add_argument("--viewer",type=Path,default=ROOT/"target/debug/openrailsrs-viewer3d")
    p.add_argument("--out-dir",type=Path,default=ROOT/"tmp/player-goldens")
    p.add_argument("--record",action="store_true",help="explicitly capture candidate goldens for review")
    p.add_argument("--prove-faults",action="store_true")
    p.add_argument("--view",action="append",choices=list(VIEWS),help="optional focused repair; default captures all seven")
    args=p.parse_args(); out=args.out_dir.resolve();out.mkdir(parents=True,exist_ok=True)
    source=ROOT/"examples/chiltern_extended/scenario.toml"
    scenario=tomllib.loads(source.read_text());graph=tomllib.loads((source.parent/scenario["route"]["path"]/"track.toml").read_text())
    first=scenario["route"]["stops"][0]
    chainage=station_chainages(scenario,graph)[first["node"]]+first.get("offset_m",0)
    frozen=station_scenario(source,scenario,chainage,0,out/"scenario")
    spec=json.loads((FIXTURES/"manifest.json").read_text()) if args.record else check_fixtures()
    base=dict(repo=ROOT,route_root=args.route_root.resolve(),viewer=args.viewer.resolve(),scenario=frozen,out_dir=out,
        software=False,renderer="gpu",headless_wayland=True,require_hardware=True,weather="clear",weather_execution="gpu",
        timeout_s=180,max_rss_mib=6144,autodrive=0,speed_mul=1,ready_frames=60,view_radius_m=450,cab_fov_deg=45,
        camera_yaw=1.6,camera_pitch=0.6,camera_distance=160,capture_or_focus=True)
    reports={}; pixels={}
    for name,pose in VIEWS.items():
        if args.view and name not in args.view:continue
        reports[name]=run_checkpoint(SimpleNamespace(**(base|pose)),name,0,True)
        if args.record:
            shutil.copy2(out/(name+".png"),FIXTURES/(name+".png"))
            spec["views"][name]["sha256"]=digest(FIXTURES/(name+".png"))
            spec["views"][name]["camera"]=reports[name]["camera"]
        else: pixels[name]=compare(out/(name+".png"),FIXTURES/(name+".png"),spec["views"][name]["regions"])
    if args.record:
        spec["scenario_sha256"]=digest(source)
        spec["track_sha256"]=digest(source.parent/scenario["route"]["path"]/"track.toml")
        (FIXTURES/"manifest.json").write_text(json.dumps(spec,indent=2)+"\n")
    native={}
    for reference in spec["or_references"]:
        if args.view and reference["view"] not in args.view:continue
        native[reference["view"]]=compare(out/(reference["view"]+".png"),ROOT/reference["path"],reference["regions"],True)
    faults={}
    if args.prove_faults:
        mutations={"mirror":("cab-front",{"visual_fault":"mirror"}),"forward":("cab-front",{"look_yaw":0.7}),
                   "occluder":("cab-front",{"visual_fault":"occluder"}),"train_missing":("orbit",{"visual_fault":"hide_train"})}
        for name,(view,mutation) in mutations.items():
            run_checkpoint(SimpleNamespace(**(base|VIEWS[view]|mutation)),"fault-"+name,0,True)
            faults[name]=compare(out/("fault-"+name+".png"),FIXTURES/(view+".png"),spec["views"][view]["regions"])
            faults[name]["detected"]=not faults[name]["pass"]
    result={"recorded":args.record,"views":pixels,"or_comparison":native,"faults":faults,"capture_metadata":reports}
    result["pass"]=all(v["pass"] for v in pixels.values()) and all(v["pass"] for v in native.values()) and all(v["detected"] for v in faults.values())
    (out/"report.json").write_text(json.dumps(result,indent=2)+"\n")
    print(json.dumps({k:v for k,v in result.items() if k!="capture_metadata"},indent=2))
    if not result["pass"]: raise SystemExit(1)

if __name__=="__main__":main()
