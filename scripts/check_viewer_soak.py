#!/usr/bin/env python3
"""Real elapsed-time endurance test on a private renderer, with bounded telemetry."""
import argparse,json,hashlib,platform,subprocess
from pathlib import Path
import check_player_expansion as expansion
import check_viewer_streaming as streaming
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--viewer',type=Path,default=Path('target/debug/openrailsrs-viewer3d'))
    p.add_argument('--out-dir',type=Path,required=True)
    p.add_argument('--scenario',type=Path,default=Path('examples/chiltern_extended/scenario.toml'))
    p.add_argument('--route-root',type=Path,required=True)
    p.add_argument('--seconds',type=float,default=10800)
    p.add_argument('--smoke',action='store_true',help='Short harness validation; not an endurance receipt')
    p.add_argument('--routes-json',type=Path,help='List of additional {path, route_root} to load every three minutes')
    p.add_argument('--audio-null', action='store_true', help='Exercise the live audio worker on a private ALSA null output')
    p.add_argument('--route-interval-s',type=float,default=180,help='Route cycle period in wall seconds (30–600); defaults to three minutes')
    p.add_argument('--framepace',choices=('off','30','60'),default='30',help='Bound presentation load on the private renderer')
    a=p.parse_args()
    if not a.smoke and a.seconds<7200: p.error('An endurance run requires at least two actual hours')
    if not 30<=a.seconds<=21600:p.error('Duration must be between 30 seconds and six hours')
    a.repo=Path(__file__).resolve().parents[1]
    a.viewer=a.viewer.resolve();a.out_dir=a.out_dir.resolve();a.out_dir.mkdir(parents=True,exist_ok=True)
    run=expansion.options(a,a.scenario.resolve(),a.route_root.resolve())
    run.audio_null=a.audio_null
    run.framepace=a.framepace
    if not 30<=a.route_interval_s<=600:p.error('Route cycle period must be 30–600 seconds')
    run.soak_route_interval_s=a.route_interval_s
    run.dev_inspector=0;run.dev_tools=0
    if a.routes_json:run.soak_routes=json.loads(a.routes_json.read_text())
    run.soak_seconds=a.seconds;run.timeout_s=a.seconds+180;run.weather_profile=None;run.ready_frames=10;run.autodrive=.5;run.speed_mul=4
    run.after_ready=True;run.particle_budget=2048
    expansion.settings(run,weather_profile='random_journey',weather_particle_budget=2048,quality_limits={'enabled':True,'target_fps':30,'min_level':0,'max_level':3})
    result=streaming.run_checkpoint(run,'endurance',0,pause=False)
    rows=[json.loads(line) for line in (a.out_dir/'endurance.soak.jsonl').read_text().splitlines()]
    if not rows or rows[-1]['wall_s']<a.seconds-35:raise RuntimeError('Missing final endurance telemetry')
    if rows[-1]['save_restore_cycles']<int(a.seconds/60)-2:raise RuntimeError('Save/restore cycle failed')
    if not a.smoke and any(r.get('schema') != 2 for r in rows):raise RuntimeError('Endurance evidence requires active entity counts (schema 2)')
    if a.routes_json and rows[-1].get('route_changes',0)<max(1,int(a.seconds/a.route_interval_s)-2):raise RuntimeError('Route-change cycle failed')
    audio=[r['native_audio'] for r in rows if r.get('native_audio')]
    if a.audio_null and (not audio or not all(r.get('device') for r in audio)):raise RuntimeError('Live audio device did not run')
    warm=[r for r in rows if r['wall_s']>=min(600,a.seconds/3)]
    entity_range=max(r['entities'] for r in warm)-min(r['entities'] for r in warm)
    per_scenario={}
    for row in warm:per_scenario.setdefault(row['scenario'],[]).append(row)
    ranges={name:{key:{'min':min(r[key] for r in group),'max':max(r[key] for r in group),'first':group[0][key],'last':group[-1][key]} for key in ('entities','meshes','images','fonts','font_atlases','pending_uploads') if all(key in r for r in group)} for name,group in per_scenario.items()}
    resource_ranges={}
    for name,group in per_scenario.items():
        fields={}
        for section,keys in [('performance',('rss_mib','peak_rss_mib')),('heap_allocator',('arena','uordblks','fordblks','hblkhd')),('graphics_memory',('process_vram_mib','process_gtt_mib','device_used_mib')),('native_audio',('decoded_mib','active_voices','programs','streams','samples')),('render_resources',('views','meshes','images','mesh_slabs','mesh_slab_bytes','indexed_mesh_allocations','material_slabs','material_slab_bytes','bindless_material_allocations','oit_node_capacity','oit_head_capacity','pipelines'))]:
            for key in keys:
                values=[r.get(section,{}).get(key) for r in group if isinstance(r.get(section),dict)]
                values=[v for v in values if isinstance(v,(int,float))]
                if values:fields[f'{section}.{key}']={'min':min(values),'max':max(values),'first':values[0],'last':values[-1]}
        for key in ('allocated_bytes','reserved_bytes','allocations','blocks'):
            values=[r.get('render_resources',{}).get('allocator',{}).get(key) for r in group if isinstance(r.get('render_resources'),dict) and isinstance(r.get('render_resources',{}).get('allocator'),dict)]
            values=[v for v in values if isinstance(v,(int,float))]
            if values:fields[f'render_resources.allocator.{key}']={'min':min(values),'max':max(values),'first':values[0],'last':values[-1]}
        for key in ('cached_keys','source_keys','stale_keys_before','stale_keys_after','retired_keys'):
            values=[r.get('render_resources',{}).get('bin_unpacking_lifetime',{}).get(key) for r in group if isinstance(r.get('render_resources'),dict) and isinstance(r.get('render_resources',{}).get('bin_unpacking_lifetime'),dict)]
            values=[v for v in values if isinstance(v,(int,float))]
            if values:fields[f'render_resources.bin_unpacking_lifetime.{key}']={'min':min(values),'max':max(values),'first':values[0],'last':values[-1]}
        for owner in ('bindings', 'prepared'):
            maps = [r.get('render_resources', {}).get('material_owners', {}).get(owner)
                    for r in group if isinstance(r.get('render_resources'), dict)]
            maps = [m for m in maps if isinstance(m, dict) and m.get('available')]
            if not maps:
                continue
            totals = [m['total'] for m in maps]
            fields[f'render_resources.material_owners.{owner}.total'] = {
                'min': min(totals), 'max': max(totals),
                'first': totals[0], 'last': totals[-1]}
            for material_type in sorted(set().union(*(m['by_type'] for m in maps))):
                # An absent type in an available actual map has zero owners;
                # an unavailable map is omitted instead of zero-filled.
                values = [m['by_type'].get(material_type, 0) for m in maps]
                fields[f'render_resources.material_owners.{owner}.{material_type}'] = {
                    'min': min(values), 'max': max(values),
                    'first': values[0], 'last': values[-1]}
        resource_ranges[name]=fields
    report={'endurance':not a.smoke,'wall_s':rows[-1]['wall_s'],'rows':len(rows),'cycles':rows[-1]['cycles'],'save_restore_cycles':rows[-1]['save_restore_cycles'],'route_changes':rows[-1].get('route_changes',0),'audio_null':a.audio_null,'audio_samples':len(audio),'entity_range_after_warmup':entity_range,'scenario_ranges':ranges,'checkpoint':result,
        'resource_ranges':resource_ranges,'memory_scope':'DRM dedicated VRAM and shared GTT are separate process accounts; neither is inferred from RSS. Legacy receipts without active_device_pci sum the capacity/usage of open DRM devices; newer receipts select the device with greatest process residency. Unavailable readings are omitted, not zero-filled',
        'heap_scope':'Optional Linux GNU mallinfo2 raw bytes describe libc arena capacity/use/free and mmap allocation space; they do not equal total PSS or all driver allocations. No allocator trimming is performed.',
        'material_scope':'material_owners uses actual RenderMaterialBindings and ErasedRenderAssets<PreparedMaterial> maps. Raw upstream bindless_material_allocations may overcount duplicate bindings within a material and is not interpreted as live ownership.',
        'configuration':{'seconds':a.seconds,'scenario':str(a.scenario.resolve()),'scenario_sha256':hashlib.sha256(a.scenario.read_bytes()).hexdigest(),'route_root':str(a.route_root.resolve()),'routes':getattr(run,'soak_routes',[]),'weather_seed':42,'speed_multiplier':4,'autodrive':.5,'particle_budget':2048,'audio':'private ALSA null' if a.audio_null else 'disabled','window':[1280,720],'framepace':a.framepace},
        'versions':{'viewer_sha256':hashlib.sha256(a.viewer.read_bytes()).hexdigest(),'cargo_lock_sha256':hashlib.sha256((a.repo/'Cargo.lock').read_bytes()).hexdigest(),'git_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=a.repo,text=True).strip(),'hardware':platform.uname()._asdict()}}
    (a.out_dir/'endurance-report.json').write_text(json.dumps(report,indent=2,default=str)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k!='checkpoint'},indent=2))
if __name__=='__main__':main()
