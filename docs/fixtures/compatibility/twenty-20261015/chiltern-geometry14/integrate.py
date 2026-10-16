from pathlib import Path
import datetime,hashlib,json,re,sys,tomllib
sys.path.insert(0,'scripts')
from prepare_chiltern_service import fields,physics_profiles
root=Path.cwd();base=root/'tmp/twenty-20261015';source=base/'chiltern-native-geometry14/track.toml';graph=tomllib.loads(source.read_text());newedges={e['id']:e for e in graph['edges']};sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
pub=root/'docs/fixtures/compatibility/twenty-20261015/chiltern-geometry14';assert not pub.exists();pub.mkdir()
receipt={'schema':1,'completed_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'origin':'Original TDB/TSECTION and canonical TRK+OpenRails include units, never observed trajectory or force','source_import_sha256':sha(source),'routes':{}};selected=set()
for name in ('chiltern_local','chiltern_extended','chiltern_traffic'):
 path=root/'examples'/name/'track.toml';before=path.read_text();old=tomllib.loads(before);text=before;counts={};skipped=[]
 for edge in old['edges']:
  key=edge['id']
  if abs(edge['length_m']-newedges[key]['length_m'])>1e-8:
   skipped.append({'edge':key,'legacy_length_m':edge['length_m'],'native_length_m':newedges[key]['length_m'],'reason':'Separate legacy600m chainage discrepancy beyond historical three-station corridor; excluded, not fitted'})
   continue
  imported=graph['edge_profiles'].get(key,{});quantities={k:imported[k] for k in ('curves','physical_cant') if imported.get(k)}
  if not quantities:continue
  selected.add(key);counts[key]={k:len(v) for k,v in quantities.items()};profile={**old.get('edge_profiles',{}).get(key,{}),**quantities}
  pattern=r'(?m)^\[edge_profiles\.'+re.escape(key)+r'\]\n[\s\S]*?(?=\n\[|\Z)'
  replacement='[edge_profiles.'+key+']\n'+fields(profile).rstrip()+'\n'
  text,n=re.subn(pattern,lambda m:replacement,text);assert n==1,(name,key,n)
 path.write_text(text);new=tomllib.loads(text)
 def without_geometry(doc):
  d=json.loads(json.dumps(doc))
  for p in d.get('edge_profiles',{}).values():
   p.pop('curves',None);p.pop('physical_cant',None)
  d['edge_profiles']={k:v for k,v in d.get('edge_profiles',{}).items() if v}
  if not d['edge_profiles']:d.pop('edge_profiles')
  return d
 assert without_geometry(new)==without_geometry(old)
 receipt['routes'][name]={'path':str(path.relative_to(root)),'before_sha256':hashlib.sha256(before.encode()).hexdigest(),'after_sha256':sha(path),'edge_profiles_added':counts,'other_parsed_inputs_unchanged':True,'excluded_legacy_edges':skipped}
(pub/'integration.json').write_text(json.dumps(receipt,indent=2)+'\n')
projection={'edge_profiles':{key:{k:v for k,v in graph['edge_profiles'][key].items() if k in ('curves','physical_cant')} for key in selected}}
(pub/'source-profiles.toml').write_text(physics_profiles(projection,selected))
route=Path('/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern');inputs=[route/'Chiltern.tdb',route/'Chiltern.trk',route/'OpenRails/Chiltern.trk',route.parent.parent/'GLOBAL/tsection.dat',route/'tsection.dat',route/'OpenRails/tsection.dat']
provenance={'schema':1,'source_only':True,'reference':'Open Rails1.6.1','reference_commit':'d16e670da333d26d2edfc97d5631a19dadf49ce5','inputs':{str(p.relative_to(route.parent.parent)):sha(p) for p in inputs if p.is_file()},'command':['target/debug/openrailsrs','import-msts','<original-content>/ROUTES/Chiltern','--out-dir','<fresh-output>'],'import_binary_sha256':json.loads((base/'build-13-result.json').read_text())['binaries']['target/debug/openrailsrs'],'full_import_sha256':sha(source),'projection_sha256':sha(pub/'source-profiles.toml'),'projection_edges':len(selected),'supersedes':'../chiltern-geometry13: earlier diagnostic import selected metric defaults before canonical TRK reader fix; preserve its original bytes and scope','scope':'All six directed edges of frozen three-station corridor and other already matching legacy edges; not six-station whole-route parity','original_content_versioned':False}
(pub/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
(pub/'integrate.py').write_bytes(Path(__file__).read_bytes())
pin=root/'oracles/openrails-reference.toml';text=pin.read_text()
for p in sorted(pub.iterdir()):
 rel=str(p.relative_to(root));assert f'path = "{rel}"' not in text;text+=f'\n[[files]]\npath = "{rel}"\nsha256 = "{sha(p)}"\n'
pin.write_text(text)
print(json.dumps({'profile_edges':len(selected),'routes':{name:len(r['edge_profiles_added']) for name,r in receipt['routes'].items()},'published':str(pub.relative_to(root))}),flush=True)
