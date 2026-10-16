#!/usr/bin/env python3
"""Compare the same binary/owned scene with each optimization and the combined set."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
from PIL import Image, ImageChops

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,default=Path('target/debug/visibility_oracle'))
    parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--instances-per-group',type=int,default=128)
    args=parser.parse_args()
    args.out.mkdir(parents=True,exist_ok=False)
    reports={}
    variants={'reference':(0,0,0),'gpu':(1,0,0),'occlusion':(0,1,0),'mips':(0,0,1),'combined':(1,1,1),'automatic':(None,1,1)}
    binary=args.binary.resolve()
    for name,(gpu,occlusion,mips) in variants.items():
        directory=args.out/name;directory.mkdir()
        env=os.environ.copy()
        if gpu is None:env.pop('OPENRAILSRS_WORLD_GPU_CULLING',None)
        else:env['OPENRAILSRS_WORLD_GPU_CULLING']=str(gpu)
        env.update(OPENRAILSRS_WORLD_OCCLUSION=str(occlusion),OPENRAILSRS_PROGRESSIVE_MIPS=str(mips),OPENRAILSRS_DISABLE_AUDIO='1')
        with (directory/'viewer.log').open('w') as log:
            subprocess.run([str(binary),str(directory.resolve()),str(args.instances_per_group)],env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=120)
        reports[name]=json.loads((directory/'result.json').read_text())
        assert len(reports[name]['records'])>=4
        print('Captured',name,flush=True)
    comparisons=[]
    for name in variants:
        if name=='reference':continue
        for phase in range(4):
            a=Image.open(args.out/'reference'/f'phase-{phase}.png').convert('RGB')
            b=Image.open(args.out/name/f'phase-{phase}.png').convert('RGB')
            assert a.size==b.size==(512,256)
            hist=ImageChops.difference(a,b).histogram()
            mean=sum((i%256)*count for i,count in enumerate(hist))/(a.width*a.height*3)
            assert mean<=0.75,(name,phase,mean)
            assert len(set(a.getdata()))>8,'Empty/flat baseline cannot certify visibility'
            comparisons.append(dict(variant=name,phase=phase,mean_rgb_error=mean,limit=0.75))
    assert any(r['draws']['compute_dispatches']>0 for r in reports['gpu']['records']), 'No actual GPU compute dispatch'
    for phase in range(4):
        shadows=reports['reference']['records'][phase]['draws']['shadow']
        assert shadows>0, 'The reference did not actually submit directional shadows'
        for name in variants:
            assert reports[name]['records'][phase]['draws']['shadow']==shadows,(name,phase,'lost shadow submissions')
    assert reports['occlusion']['records'][0]['draws']['occluded_groups']>0, 'No proved hidden groups'
    assert reports['occlusion']['records'][0]['draws']['opaque']<reports['reference']['records'][0]['draws']['opaque']
    assert reports['combined']['records'][0]['mips']['staged']>0
    assert reports['combined']['records'][0]['mips']['refined']>0
    assert reports['combined']['max_refinement_asset_bytes_per_frame']<=8*1024*1024
    assert reports['reference']['device']['hardware'] is True, 'Physical GPU required for this receipt'
    assert reports['reference']['device']['backend']=='Vulkan'
    frames={name:[v for r in d['records'] for v in r['frame_ms']] for name,d in reports.items()}
    timing={name:dict(median_ms=statistics.median(values),p95_ms=sorted(values)[int(len(values)*0.95)]) for name,values in frames.items()}
    result=dict(status='pass',binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),device=reports['reference']['device'],instances_per_group=args.instances_per_group,comparisons=comparisons,timing=timing,scope='Owned scene, fixed cameras including near-plane and floating-origin changes. No original-route or all-GPU certification; first-opaque time is affected by sequential driver/pipeline cache warmup.')
    (args.out/'result.json').write_text(json.dumps(result,indent=2))
    print(json.dumps(result,indent=2))
if __name__=='__main__':main()
