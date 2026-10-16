#!/usr/bin/env python3
"""Add read-only original adhesion observations; preserve service trace bytes."""
import csv
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import sys
import time

ROOT=Path('/home/cristian/repos/propios/ProyectoOpenRails/openrailsrs')
sys.path.insert(0,str(ROOT/'scripts'))
from oracle_wine import OracleWine,owned_processes
from capture_chiltern_service_or import facade,windows_path
from prepare_chiltern_service import PAT

PREP=Path(__file__).resolve().parent
OUT=ROOT/'tmp/twenty-20261015/chiltern-adhesion-native02'
PREFIX=ROOT/'tmp/twenty-20261015/lirr-wine-prefix'
BASELINE=ROOT/'examples/baselines/chiltern_local'
ORIGINAL_ROUTE=Path('/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern')
CONTENT=ORIGINAL_ROUTE.parent.parent
INSTALL=Path('/home/cristian/wine64-OpenRails/drive_c/Program Files/Open Rails')
REFERENCES=['RunActivity.exe','Orts.Simulation.dll','Orts.Common.dll','Orts.Settings.dll','Orts.Formats.Msts.dll','Orts.Parsers.Msts.dll','MonoGame.Framework.dll','Newtonsoft.Json.dll']
EXPECTED='ad9e690435d784df243c8a49bfdb37137c6e5f98f8941045adfde27e4500a7e2'


def sha(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()


def read(path):return json.loads(path.read_bytes())


def write(path,data):path.write_text(json.dumps(data,indent=2,ensure_ascii=False,allow_nan=False)+'\n')


def main():
    if OUT.exists():raise ValueError('Preserve existing attempt; a fresh output is required')
    OUT.mkdir(parents=True)
    receipt={'schema':1,'kind':'original_chiltern_readonly_all_car_force_geometry_observation','status':'preflight_started','commands':[],
             'reference_version':'1.6.1','reference_commit':'d16e670da333d26d2edfc97d5631a19dadf49ce5',
             'native_rust_input_scope':'none from added car diagnostics; retained environmental x0/base projection only, never force/geometry/speed/pressure outputs'}
    before={}
    try:
        manifest=read(BASELINE/'manifest.json')
        expected_files={CONTENT/name:expected for name,expected in manifest['native_files'].items()}
        expected_files[ORIGINAL_ROUTE/'Chiltern.tdb']=manifest['inputs']['tdb']
        expected_files[ORIGINAL_ROUTE/'PATHS'/PAT]=manifest['inputs']['original_pat']
        expected_files[CONTENT/'TRAINS/CONSISTS/Birmingham Pullman.con']=manifest['inputs']['consist']
        expected_files.update({INSTALL/name:expected for name,expected in manifest['runtime_assemblies'].items()})
        expected_files.update({INSTALL/name:expected for name,expected in manifest['binaries'].items()})
        expected_files[BASELINE/'HeadlessCapture.cs']=manifest['helper_sha256']
        expected_files[BASELINE/'config.json']=manifest['config_sha256']
        expected_files[BASELINE/'capture/trace.csv']=EXPECTED
        for path,expected in expected_files.items():
            actual=sha(path)
            if actual!=expected:raise ValueError('Historical source identity differs: '+str(path)+' '+actual+' vs '+expected)
        if owned_processes(PREFIX):raise ValueError('Private Wine prefix is occupied')
        for name in ['ChilternForceDiagnostics.cs','HeadlessCapture-original.cs','protocol.json','capture.py']:
            target=OUT/'capture-sources'/name;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(PREP/name,target)
        shutil.copyfile(ROOT/'scripts/oracle_wine.py',OUT/'capture-sources/oracle_wine.py')
        shutil.copyfile(ROOT/'oracles/openrails-reference.toml',OUT/'capture-sources/reference-pin-at-capture.toml')
        sources=list(expected_files)+[BASELINE/'manifest.json',BASELINE/'preparation.json',BASELINE/'reproducibility.json',ROOT/'oracles/chiltern-service.toml',ROOT/'scripts/oracle_wine.py',
                ROOT.parent/'openrails/Source/Orts.Common/Filter.cs',ROOT.parent/'openrails/Source/Orts.Simulation/Simulation/RollingStocks/MSTSLocomotive.cs',ROOT.parent/'openrails/Source/Orts.Simulation/Simulation/RollingStocks/TrainCar.cs',ROOT.parent/'openrails/Source/Orts.Simulation/Simulation/RollingStocks/SubSystems/Brakes/MSTS/AirSinglePipe.cs',PREP.parent/'chiltern-adhesion-preparation01/ChilternAdhesionReference.cs']
        sources.extend(p for p in (OUT/'capture-sources').iterdir() if p.is_file())
        # Own generated ACT/SRV/PAT retain the published baseline bytes. Bulk
        # Content is linked read-only; no native definitions are rewritten.
        route=facade(ORIGINAL_ROUTE,OUT/'content/Chiltern')
        for folder,suffix in [('ACTIVITIES','.act'),('SERVICES','.srv'),('PATHS','.pat')]:
            source=BASELINE/'content/Chiltern/ROUTES/Chiltern'/folder/('openrailsrs_chiltern_local'+suffix)
            target=route/folder/source.name
            if target.is_symlink():target.unlink() # Only remove our facade link.
            if target.exists():raise ValueError('Generated target already exists')
            shutil.copyfile(source,target)
            assert sha(target)==sha(source)
            sources.extend([source,target])
        shutil.copyfile(BASELINE/'config.json',OUT/'config.json');sources.append(OUT/'config.json')
        before={str(path.resolve()):sha(path) for path in sources}
        # Generated files are real copies; symlinked originals resolve to their
        # existing immutable identity. The map deliberately includes no Rust.
        receipt.update(status='declared_before_wine',input_sha256_before=before,
                       protocol_sha256=sha(PREP/'protocol.json'),helper_sha256=sha(PREP/'ChilternForceDiagnostics.cs'),
                       historical_trace_sha256=EXPECTED,historical_manifest_sha256=sha(BASELINE/'manifest.json'),
                       original_assembly_sha256={name:sha(INSTALL/name) for name in REFERENCES},
                       protected_human_traces_sha256=read(ROOT/'tmp/twenty-20261015/protected-traces-before.json'))
        write(OUT/'receipt.json',receipt)
        runtime=OUT/'runtime';runtime.mkdir()
        for file in INSTALL.iterdir():
            if file.is_file() and (file.suffix.lower()=='.dll' or file.name in ['RunActivity.exe','RunActivity.exe.config']):
                shutil.copy2(file,runtime/file.name)
        assert all(sha(runtime/name)==receipt['original_assembly_sha256'][name] for name in REFERENCES)
        print('Historical Content/DLL hashes verified; private OracleWine preflight follows',flush=True)
        with OracleWine(PREFIX,OUT/'wine-isolation') as wine:
            command=['wine',r'C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe','/nologo','/target:exe','/main:HeadlessCapture','/out:ChilternForceDiagnostics.exe']+['/r:'+name for name in REFERENCES]+[windows_path(OUT/'capture-sources/ChilternForceDiagnostics.cs')]
            wine.run(command,cwd=runtime,log=OUT/'compile.log',timeout=90)
            receipt['runtime_executable_sha256']=sha(runtime/'ChilternForceDiagnostics.exe')
            print('Observer compiled; running unchanged original service logic',flush=True)
            wine.run(['wine','ChilternForceDiagnostics.exe','service',windows_path(route/'ACTIVITIES/openrailsrs_chiltern_local.act'),windows_path(OUT/'native'),windows_path(OUT/'config.json')],cwd=runtime,log=OUT/'capture.log',timeout=900)
            receipt['commands']=wine.commands
        trace=OUT/'native/trace.csv';observed=sha(trace)
        rows=list(csv.DictReader(trace.open(newline='')))
        previous=list(csv.DictReader((BASELINE/'capture/trace.csv').open(newline='')))
        mismatches={key:sum(a[key]!=b[key] for a,b in zip(rows,previous)) for key in previous[0]}
        integrity={'schema':1,'rows':len(rows),'previous_rows':len(previous),'columns':len(rows[0]),'sha256':observed,'reference_sha256':EXPECTED,
                   'all_previous_columns_byte_identical':observed==EXPECTED,'per_column_scalar_string_mismatches':mismatches}
        write(OUT/'trace-integrity.json',integrity)
        if observed!=EXPECTED or len(rows)!=15003:raise ValueError('Original service trace is not byte-identical; do not use environmental inputs')
        if sha(OUT/'native/adhesion-observation.json')!='3392f303463a7d54b48cda1bfd8bd260855ffcc952fb77747e9bf3eea71a1103':
            raise ValueError('Read-only IIR diagnostics changed relative to01; preserve attempt and investigate')
        with (OUT/'native/car-diagnostics.csv').open(newline='') as stream:
            count=0;missing=set()
            for count,car in enumerate(csv.DictReader(stream),1):
                assert int(car['tick'])==(count-1)//8 and int(car['vehicle'])==(count-1)%8
                assert abs(float(car['time_s'])-int(car['tick'])*.05)<1e-9
                for key in ['CurveForceN','CurveForceNFiltered','WindForceN','TunnelForceN','GravityForceN','FrictionForceN','BrakeForceN','MotiveForceN','TotalForceN','_SpeedMpS','AbsSpeedMpS','MassKG','CurrentCurveRadiusM','SuperElevationM','SuperElevationAngleRad','TrackGaugeM','RigidWheelBaseM','WagonCoefficientFriction']:
                    assert __import__('math').isfinite(float(car[key])), key
                missing.update(key for key,value in car.items() if value=='')
            assert count==15003*8
        write(OUT/'diagnostics-integrity.json',{'schema':1,'all_car_rows':count,'car_diagnostics_sha256':sha(OUT/'native/car-diagnostics.csv'),'same_old_trace_sha256':EXPECTED,'same_adhesion_observation_sha256':sha(OUT/'native/adhesion-observation.json'),'optional_empty_fields':sorted(missing),'diagnostics_may_drive_rust':False})
        rich=read(OUT/'native/adhesion-observation.json')
        assert rich['rows_including_initial']==15003 and len(rich['samples'])==15002
        assert rich['initial_tick']==0 and rich['vehicle_count']==8
        assert [v['vehicle'] for v in rich['initial']]==[0,7]
        for vehicle in rich['initial']:
            assert len(vehicle['x'])==len(vehicle['y'])==2 and vehicle['order']==1
        for tick,sample in enumerate(rich['samples'],1):
            assert sample['tick']==tick and abs(sample['time_s']-tick*.05)<1e-9
            assert [v['vehicle'] for v in sample['vehicles']]==[0,7]
            for vehicle in sample['vehicles']:
                assert vehicle['input_factor']==vehicle['x'][0] and vehicle['output_factor']==vehicle['y'][0]
        # Decimal output round-trips the actual Single; post-step y is omitted.
        def single_text(value):
            value=struct.unpack('f',struct.pack('f',value))[0]
            return format(value,'.9g')
        with (OUT/'adhesion-inputs.csv').open('w',newline='') as stream:
            writer=csv.writer(stream,lineterminator='\n')
            writer.writerow(['time_s','vehicle','input_factor','base_factor','filter_x0','filter_x1','filter_y0','filter_y1'])
            for vehicle in rich['initial']:
                writer.writerow(['0',vehicle['vehicle'],single_text(vehicle['input_factor']),single_text(vehicle['base_factor']),*[single_text(v) for v in vehicle['x']],*[single_text(v) for v in vehicle['y']]])
            for sample in rich['samples']:
                for vehicle in sample['vehicles']:
                    writer.writerow([format(sample['time_s'],'.12g'),vehicle['vehicle'],single_text(vehicle['input_factor']),single_text(vehicle['base_factor']),'','','',''])
        write(OUT/'input-extraction.json',{'schema':1,'kind':'environment_inputs_only_projection','source_sha256':sha(OUT/'native/adhesion-observation.json'),'output_sha256':sha(OUT/'adhesion-inputs.csv'),
             'native_trace_identity_required':EXPECTED,'rows':15003*2,'vehicles':[0,7],'step_s':.05,
             'initial':'only tick0 carries actual x[0..1]/y[0..1]; no Reset(.5) assumption','post_update':'only x0 exact Single and BaseFrictionCoefficientFactor; all post-step filter state and force/speed diagnostics excluded',
             'numeric_conversion':'parse exact native JSON Single, format9 significant decimal digits; output recovers identical IEEE754 Single bits',
             'rust_execution':'not_run'})
        receipt['status']='native_capture_complete_trace_and_iir_byte_identical_additive_all_car_diagnostics'
        receipt['initial_filter_observations']=rich['initial']
        receipt['native_rows']=15003;receipt['environment_rows']=30006;receipt['car_diagnostic_rows']=120024;receipt['car_diagnostics_sha256']=sha(OUT/'native/car-diagnostics.csv')
        receipt['native_trace_sha256']=observed
        receipt['rich_observation_sha256']=sha(OUT/'native/adhesion-observation.json')
        receipt['input_only_csv_sha256']=sha(OUT/'adhesion-inputs.csv')
        print(json.dumps({'status':receipt['status'],'trace_sha256':observed,'initial':rich['initial'],'input_csv_sha256':receipt['input_only_csv_sha256']},indent=2),flush=True)
    except BaseException as error:
        receipt['status']='failed_preserved'
        receipt['error']=repr(error)
        raise
    finally:
        if before:
            after={path:sha(Path(path)) for path in before}
            receipt['input_sha256_after']=after;receipt['inputs_unchanged']=before==after
        receipt['ended_unix_s']=time.time()
        if (OUT/'wine-isolation/release.json').exists():
            release=read(OUT/'wine-isolation/release.json')
            receipt['wine_private_release_sha256']=sha(OUT/'wine-isolation/release.json')
            receipt['private_processes_remaining']=release['remaining_processes']
            receipt['forbidden_drivers_seen']=release['forbidden_drivers_seen']
        receipt['files_sha256']={str(p.relative_to(OUT)):sha(p) for p in sorted(OUT.rglob('*')) if p.is_file() and not p.is_symlink() and not any(part in p.relative_to(OUT).parts for part in ['runtime','content']) and p.name!='receipt.json'}
        write(OUT/'receipt.json',receipt)
        if before and not receipt['inputs_unchanged']:raise ValueError('Source identities changed during capture')


if __name__=='__main__':main()
