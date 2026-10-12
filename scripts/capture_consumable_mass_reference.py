#!/usr/bin/env python3
"""Capture fuel-mass points by invoking the pinned, unmodified OR 1.6.1 DLL."""
import argparse,hashlib,json,os,shutil,subprocess,tomllib
from pathlib import Path
from capture_chiltern_service_or import windows_path,clone_prefix
from run_oracles import verify
ROOT=Path(__file__).resolve().parents[1]
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out-dir',type=Path,required=True);args=p.parse_args()
 install=Path.home()/'wine64-OpenRails/drive_c/Program Files/Open Rails';prefix=ROOT/'tmp/or161-wine-prefix';out=args.out_dir.resolve()
 if out.exists() or out.is_relative_to(install.resolve()) or out.is_relative_to((ROOT.parent/'openrails').resolve()):raise ValueError('Choose a fresh output outside originals')
 pin=tomllib.loads((ROOT/'oracles/openrails-reference.toml').read_text());verify(pin,ROOT.parent/'openrails',install);clone_prefix(Path.home()/'wine64-OpenRails',prefix)
 out.mkdir(parents=True);runtime=out/'runtime';shutil.copytree(install,runtime)
 env=dict(os.environ,WINEPREFIX=str(prefix),WINEARCH='win64',WINEDEBUG='-all');env.pop('DISPLAY',None);env.pop('WAYLAND_DISPLAY',None)
 files=['RunActivity.exe','Orts.Simulation.dll','Orts.Settings.dll','Orts.Common.dll','Orts.Formats.Msts.dll','Orts.Parsers.Msts.dll','MonoGame.Framework.dll','Newtonsoft.Json.dll'];helper=ROOT/'tools/openrails-reference/ConsumableMassReference.cs'
 with (out/'compile.log').open('w') as log:subprocess.run(['wine',r'C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe','/nologo','/out:ConsumableMassReference.exe']+['/r:'+f for f in files]+[windows_path(helper)],cwd=runtime,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=60,check=True)
 reference=out/'reference.json'
 with (out/'capture.log').open('w') as log:subprocess.run(['wine','ConsumableMassReference.exe',windows_path(reference)],cwd=runtime,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=60,check=True)
 provenance={'reference_version':pin['version'],'reference_commit':pin['commit'],'sha256':{f:hashlib.sha256((runtime/f).read_bytes()).hexdigest()for f in files},'helper_sha256':hashlib.sha256(helper.read_bytes()).hexdigest(),'output_sha256':hashlib.sha256(reference.read_bytes()).hexdigest()}
 (out/'manifest.json').write_text(json.dumps(provenance,indent=2)+'\n');print(json.dumps(provenance))
if __name__=='__main__':main()
