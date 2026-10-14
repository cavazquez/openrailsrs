#!/usr/bin/env python3
"""Real elapsed-time endurance test on a private renderer, with bounded telemetry."""
import argparse,json,hashlib,math,platform,re,subprocess,time
from pathlib import Path
import check_player_expansion as expansion
import check_viewer_streaming as streaming

def require_count(value, label):
    if type(value) is not int or value < 0:
        raise RuntimeError(f'{label} must be an available non-negative integer')
    return value

def require_finite(value, label):
    if type(value) not in (int, float) or not math.isfinite(value):
        raise RuntimeError(f'{label} must be an available finite number')
    return value

def validate_trace(rows, seconds, *, smoke=False, route_interval_s=180, require_routes=False):
    """Accept current schema3 evidence only; partial/legacy runs are diagnostics."""
    require_finite(seconds, 'requested wall seconds')
    require_finite(route_interval_s, 'route interval')
    if seconds < 30 or route_interval_s <= 0 or not smoke and seconds < 7200:
        raise RuntimeError('A final endurance requires at least 7200 actual seconds')
    if not rows:
        raise RuntimeError('Missing endurance telemetry')
    previous = None
    counter_names = ('cycles', 'save_restore_cycles', 'memory_restore_failures', 'disk_save_attempts',
                     'disk_save_restore_cycles', 'disk_save_failures', 'route_changes')
    for row in rows:
        if row.get('schema') != 3:
            raise RuntimeError('Current endurance evidence requires schema 3 with real SavedGame disk restores')
        wall = require_finite(row.get('wall_s'), 'wall_s')
        if wall < 0 or previous is not None and wall <= previous['wall_s']:
            raise RuntimeError('Wall samples must increase strictly')
        counters = {k:require_count(row.get(k), k) for k in counter_names}
        if previous is not None and any(counters[k] < previous[k] for k in counter_names):
            raise RuntimeError('Endurance counters cannot move backwards')
        if counters['cycles'] > int(wall // 60):
            raise RuntimeError('Save cycles exceed the actual 60-second cadence')
        if counters['disk_save_failures']:
            raise RuntimeError('A SavedGame disk restore failed')
        if counters['memory_restore_failures']:
            raise RuntimeError('A JSON snapshot restore failed')
        if 'memory_restore_last_error' not in row or row['memory_restore_last_error'] is not None:
            raise RuntimeError('A JSON snapshot error is unavailable or remains')
        if any(k not in row for k in ('disk_save_last_error','disk_save_last_bytes','disk_save_last_sha256')):
            raise RuntimeError('SavedGame disk receipt fields are missing')
        if row['disk_save_last_error'] is not None:
            raise RuntimeError('A SavedGame disk error remains')
        if any(counters[k] != counters['cycles'] for k in (
                'save_restore_cycles', 'disk_save_attempts', 'disk_save_restore_cycles')):
            raise RuntimeError('Every executed cycle must complete both JSON and actual disk save/read/restore')
        if counters['cycles']:
            if require_count(row.get('disk_save_last_bytes'), 'disk_save_last_bytes') == 0:
                raise RuntimeError('SavedGame bytes are missing')
            if not isinstance(row.get('disk_save_last_sha256'), str) or not re.fullmatch(
                    r'[0-9a-f]{64}', row['disk_save_last_sha256']):
                raise RuntimeError('SavedGame SHA256 is missing')
        require_count(row.get('pending_uploads'), 'pending_uploads')
        previous = dict(counters, wall_s=wall)
    final = rows[-1]
    if final['wall_s'] < seconds:
        raise RuntimeError('Missing the full requested real wall duration')
    expected_cycles = int(seconds // 60)
    if final['cycles'] < expected_cycles:
        raise RuntimeError(f'Missing real SavedGame cycles: need {expected_cycles}, got {final["cycles"]}')
    expected_routes = max(0, int(seconds // route_interval_s) - 1) if require_routes else 0
    if final['route_changes'] < expected_routes:
        raise RuntimeError(f'Missing route changes: need {expected_routes}')
    if final['pending_uploads']:
        raise RuntimeError('The final telemetry checkpoint still has pending uploads')
    return {'schema':3, 'actual_wall_s':final['wall_s'], 'requested_wall_s':seconds,
            'required_cycles':expected_cycles, 'json_restores':final['save_restore_cycles'],
            'json_failures':final['memory_restore_failures'],
            'disk_attempts':final['disk_save_attempts'], 'disk_restores':final['disk_save_restore_cycles'],
            'disk_failures':final['disk_save_failures'], 'required_route_changes':expected_routes,
            'route_changes':final['route_changes'], 'endurance':not smoke,
            'disk_scope':'Not exercised: the smoke duration is below 60 seconds' if expected_cycles == 0
                         else 'Actual SavedGame save, filesystem read and restore in every executed 60-second cycle'}

def validate_capture(report, *, returncode, image, log, started_ns, seconds):
    """The screenshot observer must have written a new file before a clean exit."""
    if type(returncode) is not int or returncode != 0:
        raise RuntimeError('The actual viewer process did not exit successfully')
    if require_finite(report.get('elapsed_s'), 'process elapsed_s') < seconds:
        raise RuntimeError('The process did not run for the requested real duration')
    if not image.is_file() or image.stat().st_mtime_ns < started_ns or image.stat().st_size <= 8:
        raise RuntimeError('A new nonempty screenshot is required')
    if image.read_bytes()[:8] != b'\x89PNG\r\n\x1a\n':
        raise RuntimeError('The screenshot is not a PNG')
    if f'screenshot saved → {image}' not in log:
        raise RuntimeError('The ScreenshotCaptured write observer did not confirm this image')
    pipelines = report.get('shader_pipelines')
    if not isinstance(pipelines,dict) or any(
            require_count(pipelines.get(k),f'shader_pipelines.{k}') for k in ('pending','failed')):
        raise RuntimeError('Final shaders are not ready')
    for key in ('pending_gpu_uploads','pending_terrain_tiles','unactivated_near_shapes'):
        if require_count(report.get(key),key):
            raise RuntimeError(f'Final {key} is not zero')
    if report.get('native_signal_errors') is None or report['native_signal_errors']:
        raise RuntimeError('Native signal errors are unavailable or nonempty')

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
    started_ns=time.time_ns()
    result=streaming.run_checkpoint(run,'endurance',0,pause=False)
    rows=[json.loads(line) for line in (a.out_dir/'endurance.soak.jsonl').read_text().splitlines()]
    trace_validation=validate_trace(rows,a.seconds,smoke=a.smoke,
        route_interval_s=a.route_interval_s,require_routes=bool(a.routes_json))
    # run_checkpoint returns only after the actual subprocess returned0; it
    # removes preceding image/report files before launching the viewer.
    validate_capture(result,returncode=0,image=a.out_dir/'endurance.png',
        log=(a.out_dir/'endurance.log').read_text(errors='replace'),
        started_ns=started_ns,seconds=a.seconds)
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
    report={'endurance':not a.smoke,'wall_s':rows[-1]['wall_s'],'rows':len(rows),'cycles':rows[-1]['cycles'],'save_restore_cycles':rows[-1]['save_restore_cycles'],'route_changes':rows[-1].get('route_changes',0),'trace_validation':trace_validation,'audio_null':a.audio_null,'audio_samples':len(audio),'entity_range_after_warmup':entity_range,'scenario_ranges':ranges,'checkpoint':result,
        'resource_ranges':resource_ranges,'memory_scope':'DRM dedicated VRAM and shared GTT are separate process accounts; neither is inferred from RSS. Legacy receipts without active_device_pci sum the capacity/usage of open DRM devices; newer receipts select the device with greatest process residency. Unavailable readings are omitted, not zero-filled',
        'heap_scope':'Optional Linux GNU mallinfo2 raw bytes describe libc arena capacity/use/free and mmap allocation space; they do not equal total PSS or all driver allocations. No allocator trimming is performed.',
        'material_scope':'material_owners uses actual RenderMaterialBindings and ErasedRenderAssets<PreparedMaterial> maps. Raw upstream bindless_material_allocations may overcount duplicate bindings within a material and is not interpreted as live ownership.',
        'configuration':{'seconds':a.seconds,'scenario':str(a.scenario.resolve()),'scenario_sha256':hashlib.sha256(a.scenario.read_bytes()).hexdigest(),'route_root':str(a.route_root.resolve()),'routes':getattr(run,'soak_routes',[]),'weather_seed':42,'speed_multiplier':4,'autodrive':.5,'particle_budget':2048,'audio':'private ALSA null' if a.audio_null else 'disabled','window':[1280,720],'framepace':a.framepace},
        'versions':{'viewer_sha256':hashlib.sha256(a.viewer.read_bytes()).hexdigest(),'cargo_lock_sha256':hashlib.sha256((a.repo/'Cargo.lock').read_bytes()).hexdigest(),'git_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=a.repo,text=True).strip(),'hardware':platform.uname()._asdict()}}
    (a.out_dir/'endurance-report.json').write_text(json.dumps(report,indent=2,default=str)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k!='checkpoint'},indent=2))
if __name__=='__main__':main()
