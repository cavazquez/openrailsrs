#!/usr/bin/env python3
"""Publish genuine environmental observations separately from runtime inputs."""
from pathlib import Path
import csv,gzip,hashlib,json,shutil,struct
ROOT=Path('/home/cristian/repos/propios/ProyectoOpenRails/openrailsrs')
RAW=ROOT/'tmp/twenty-20261015/chiltern-adhesion-native01'
PUBLIC=ROOT/'docs/fixtures/compatibility/twenty-20261015/chiltern-adhesion161-native01'
INPUT=ROOT/'examples/chiltern_local/physics/native-adhesion-inputs.csv'
sha=lambda p:hashlib.file_digest(p.open('rb'),'sha256').hexdigest()
read=lambda p:json.loads(p.read_bytes())
write=lambda p,d:p.write_text(json.dumps(d,indent=2,ensure_ascii=False,allow_nan=False)+'\n')
assert not PUBLIC.exists(),'Fresh publication destination required'
receipt=read(RAW/'receipt.json');rich=read(RAW/'native/adhesion-observation.json');integrity=read(RAW/'trace-integrity.json')
assert receipt['inputs_unchanged'] and receipt['status']=='native_capture_complete_trace_byte_identical_environment_projection_ready'
assert sha(RAW/'native/trace.csv')==integrity['reference_sha256']=='ad9e690435d784df243c8a49bfdb37137c6e5f98f8941045adfde27e4500a7e2'
assert integrity['rows']==15003 and all(n==0 for n in integrity['per_column_scalar_string_mismatches'].values())
source=RAW/'adhesion-inputs.csv'
if INPUT.exists():assert sha(INPUT)==sha(source)
else:shutil.copyfile(source,INPUT)
old=ROOT/'examples/chiltern_local/physics/adhesion-input.csv'
if old.exists():
 assert sha(old)==sha(INPUT)
 old.unlink() # Our newly created provisional copy; canonical destination agreed with Root/A.
verified=read(RAW/'input-projection-verified.json')
verified.pop('runtime_or_rust_executed')
verified['rust_executed']=False
verified['verification_method']='Python IEEE754 Single bit equality only; original C# execution recorded separately in receipt.json'
verified['output']=INPUT.relative_to(ROOT).as_posix()
write(RAW/'input-projection-verified.json',verified)
PUBLIC.mkdir(parents=True)
files={}
for name in ['receipt.json','compile.log','capture.log','trace-integrity.json','input-extraction.json','input-projection-verified.json','native/initial.json','native/outcome.json','wine-isolation/isolation.json','wine-isolation/release.json']:
 target=PUBLIC/name;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(RAW/name,target)
 files[name]={'sha256':sha(target),'raw_path':name,'raw_sha256':sha(RAW/name),'transformation':'none; literal bytes'}
for source in sorted((RAW/'capture-sources').iterdir()):
 if not source.is_file():continue
 name='capture-sources/'+source.name;target=PUBLIC/name;target.parent.mkdir(exist_ok=True);shutil.copyfile(source,target)
 files[name]={'sha256':sha(target),'raw_path':name,'raw_sha256':sha(source),'transformation':'none; literal capture source or preflight snapshot'}
source=RAW/'native/adhesion-observation.json';target=PUBLIC/'native/adhesion-observation.json.gz'
with source.open('rb') as incoming,target.open('wb') as output:
 with gzip.GzipFile(filename='',fileobj=output,mode='wb',mtime=0,compresslevel=9) as zipped:shutil.copyfileobj(incoming,zipped)
with gzip.open(target,'rb') as decoded:assert hashlib.file_digest(decoded,'sha256').hexdigest()==sha(source)
files['native/adhesion-observation.json.gz']={'sha256':sha(target),'raw_path':'native/adhesion-observation.json','raw_sha256':sha(source),'transformation':'lossless gzip level9/mtime0; exact decoded bytes'}
protocol={'schema':1,'status':'declared_before_first_rust_environmental_replay','original_capture_protocol_sha256':sha(RAW/'capture-sources/protocol.json'),'input_schema':['time_s','vehicle','input_factor','base_factor','filter_x0','filter_x1','filter_y0','filter_y1'],'initial':'actual original x/y buffers only at time0; both locomotives have allzero buffers and base0, not Reset(.5)','post_tick':'Read exact Single x0 and original base. Derive realized variation Single(input_factor-base_factor), then combine it with our independently evolved local base determined by our own position/weather. Do not anchor slippery transition to native motion/position.','base_observation_scope':'subtraction and validation only; not a prescribed runtime base transition','post_tick_filter_state':'all four x/y columns are empty; output y0 resides only in isolated filter oracle, never in runtime input file','filter_maximum_absolute_error':1e-6,'prior_motion_limits_unchanged':True,'original_service_protocol':'oracles/chiltern-service.toml','original_service_protocol_sha256':sha(ROOT/'oracles/chiltern-service.toml'),'runtime_input':INPUT.relative_to(ROOT).as_posix(),'runtime_input_sha256':sha(INPUT),'trace_identity':'all15003rows/all20columns rawbyteSHAad9e690435d784df243c8a49bfdb37137c6e5f98f8941045adfde27e4500a7e2','source_scope':'single frozen original environmental input realization; no output forces/speeds/slip/filter state prescribed afterstart'}
write(PUBLIC/'replay-protocol.json',protocol)
files['replay-protocol.json']={'sha256':sha(PUBLIC/'replay-protocol.json'),'transformation':'explicit input-only replay contract; original capture protocol preserved separately'}
initial=[{k:v for k,v in engine.items() if k!='diagnostics_not_inputs'} for engine in rich['initial']]
filter_rows=[[0,engine['vehicle'],engine['output_factor']] for engine in rich['initial']]
filter_rows.extend([sample['tick'],engine['vehicle'],engine['output_factor']] for sample in rich['samples'] for engine in sample['vehicles'])
filter_oracle={'schema':1,'kind':'original_iir_filter_outputs_for_isolated_test_only','status':'captured_not_compared','step_s':.05,'vehicles':[0,7],'rows':30006,'input_csv':INPUT.relative_to(ROOT).as_posix(),'input_sha256':sha(INPUT),'raw_original_observation_sha256':sha(source),'initial_filter_hardware_and_state':initial,'row_columns':['tick','vehicle','expected_output_factor_y0'],'filter_maximum_absolute_error':1e-6,'outputs_may_drive_runtime':False,'expected':filter_rows}
# Compact numeric projection preserves every y0 decimal from genuine JSON.
(PUBLIC/'filter-oracle.json').write_text(json.dumps(filter_oracle,separators=(',',':'),allow_nan=False)+'\n')
files['filter-oracle.json']={'sha256':sha(PUBLIC/'filter-oracle.json'),'raw_sha256':sha(source),'transformation':'all30006 genuine y0 samples selected without force formula, rounding or downsampling; output-only isolated test'}
initial_context={'schema':1,'kind':'original_iir_state_before_first_update','trace_initial_time_s':0.,'vehicle_count':8,'source':'Original Simulator.Start completed; observer runs before first Simulator.Update. No call to Filter/Reset added.','vehicles':initial}
write(PUBLIC/'initial-filter-context.json',initial_context)
files['initial-filter-context.json']={'sha256':sha(PUBLIC/'initial-filter-context.json'),'raw_sha256':sha(source),'transformation':'literal initial filter fields projected; no post-start state'}
shutil.copyfile(__file__,PUBLIC/'publish.py')
files['publish.py']={'sha256':sha(PUBLIC/'publish.py'),'transformation':'own extraction/publication script used here'}
publication={'schema':1,'kind':'additive_original_adhesion_observation_and_input_projection','reference_version':'1.6.1','reference_commit':receipt['reference_commit'],'original_receipt_sha256':sha(RAW/'receipt.json'),'original_trace_rows':15003,'original_trace_columns':20,'original_trace_sha256':sha(RAW/'native/trace.csv'),'original_trace_public_path':'examples/baselines/chiltern_local/capture/trace.csv','all_original_trace_bytes_unchanged':True,'raw_observation_sha256':sha(source),'runtime_input_external_path':INPUT.relative_to(ROOT).as_posix(),'runtime_input_external_sha256':sha(INPUT),'runtime_input_rows':30006,'initial_buffers_observed_zero':True,'filter_oracle_not_runtime_input':True,'prior_filter_limit':1e-6,'prior_motion_limits_unchanged':True,'files':files,'original_resources_bundled':False,'rust_execution':'not_run_by_this_capture','full_workspace_gate_claim':False,'excluded':['original ENG/WAG/CON/ACT/SRV/PAT/TDB/TRK/route/DLL assets','runtime and private Wineprefix','all six human trace contents','duplicate original20column trace (already pinned separately)','duplicate runtime input CSV (external path/hash above)'],'scope':'Original service unchanged, paused historical context. Only observation added. Runtime uses a captured environmental variation, not recorded forces/motion/filter outputs; source x/y state is supplied only at initialization. Full measured original outputs remain diagnostics and isolated-filter evidence.'}
write(PUBLIC/'publication.json',publication)
print(json.dumps({'directory':str(PUBLIC.relative_to(ROOT)),'publication_sha256':sha(PUBLIC/'publication.json'),'runtime_csv':str(INPUT.relative_to(ROOT)),'runtime_csv_sha256':sha(INPUT),'filter_oracle_sha256':sha(PUBLIC/'filter-oracle.json'),'gzip_sha256':sha(target),'files':len(files)+1,'original_receipt_sha256':sha(RAW/'receipt.json')},indent=2))
