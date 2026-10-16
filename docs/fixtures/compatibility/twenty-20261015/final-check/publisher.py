#!/usr/bin/env python3
"""Publish a derived check receipt; never run tests, modify pins, or close issues."""
import argparse,copy,datetime,gzip,hashlib,json,re,subprocess,tempfile
from pathlib import Path

def sha(data): return hashlib.sha256(data).hexdigest()
def load(path): return json.loads(path.read_text())
def encoded(value): return (json.dumps(value,indent=2,ensure_ascii=False)+'\n').encode()
def number(path):
 m=re.fullmatch(r'check-final(\d+)-result\.json',path.name)
 if not m: raise ValueError('Expected an actual check-finalNN-result.json receipt')
 return int(m[1])
def clean(text,root):
 for source,target in [(str(root),'<REPOSITORY>'),(str(Path.home()),'<USER_HOME>'),('/root','<BUILD_HOME>')]:
  text=text.replace(source,target).replace(source.replace('/','\\'),target)
 return re.sub(r'/home/[^/\s"\\]+/', '<USER_HOME>/',text)
def git_commit_binding(root,commit,expected):
 if not commit:return {'verified':False,'reason':'No commit supplied; root closure remains pending.'}
 if not re.fullmatch(r'[0-9a-fA-F]{40}',commit):raise ValueError('Commit must be the full40-digit hash')
 subprocess.run(['git','cat-file','-e',commit+'^{commit}'],cwd=root,check=True,capture_output=True)
 paths=sorted(expected)
 if any(Path(p).is_absolute() or '..' in Path(p).parts for p in paths):raise ValueError('Invalid source snapshot path')
 names=subprocess.run(['git','ls-tree','-r','--name-only','-z',commit,'--',*paths],cwd=root,check=True,capture_output=True).stdout.decode().split('\0')
 missing=sorted(set(paths)-set(names))
 current=[p for p,h in expected.items() if not (root/p).is_file() or sha((root/p).read_bytes())!=h]
 diff=subprocess.run(['git','diff','--quiet',commit,'--',*paths],cwd=root,capture_output=True)
 return {'commit':commit,'verified':not missing and not current and diff.returncode==0,
  'missing_from_commit':missing,'changed_since_check_snapshot':current,'worktree_matches_commit':diff.returncode==0,
  'scope':'Exact checked runtime/fixture paths only; does not claim the complete working tree is clean.'}
def main():
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument('--check-result',type=Path,required=True)
 parser.add_argument('--pin-used',type=Path,required=True,help='Exact archived pin supplied by Root for this attempt; never the mutable current pin')
 parser.add_argument('--matrix',type=Path,default=Path('tmp/twenty-20261015/closure-evidence-proposal20-build11.json'))
 parser.add_argument('--runtime-before',type=Path)
 parser.add_argument('--fixtures-before',type=Path,default=Path('tmp/twenty-20261015/compile-fixtures-before-build11.json'))
 parser.add_argument('--commit',help='Optional actual final commit; verified read-only against checked sources')
 parser.add_argument('--out',type=Path,default=Path('docs/fixtures/compatibility/twenty-20261015/final-check'))
 args=parser.parse_args();root=Path.cwd().resolve();base=args.check_result.parent;n=number(args.check_result)
 if args.out.exists():raise ValueError('Output already exists; preserve it and choose a fresh directory')
 final=load(args.check_result); pin=args.pin_used.read_bytes(); matrix=load(args.matrix)
 if b'final-check/result.json' in pin:raise ValueError('Used pin must not include this final receipt (self-reference)')
 if len(matrix['issues'])!=20 or sum(len(i['criteria']) for i in matrix['issues'])!=60:raise ValueError('Expected the complete20-by3 matrix')
 history=[];payloads={};all_passed_tests=set();latest_log=None
 for i in range(1,n+1):
  receipt=base/f'check-final{i:02}-result.json';log=base/f'check-final{i:02}.log'
  result=load(receipt);raw=log.read_bytes();text=raw.decode('utf-8',errors='replace')
  if sha(raw)!=result['log_sha256']:raise ValueError(f'Raw log hash mismatch: {log}')
  if (result['status']=='pass')!=(result['exit_code']==0):raise ValueError('Result status/exit code disagree')
  counts=[tuple(map(int,m)) for m in re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',text)]
  totals=[sum(x[j] for x in counts) for j in range(3)]
  if totals!=[result['rust_passed'],result['rust_failed'],result['rust_ignored']]:raise ValueError('Rust totals differ from the authentic log')
  safe=clean(text,root).encode();safe_receipt=clean(receipt.read_text(),root).encode()
  prefix=f'history/check-final{i:02}'
  payloads[prefix+'-result.json']=safe_receipt
  payloads[prefix+'.log.gz']=gzip.compress(safe,mtime=0)
  exact_pin=base/f'reference-pin-used-by-finalcheck{i:02}.toml'
  pin_info=None
  if i==1:
   pin_receipt=base/'reference-pin-used-by-final-check.json'
   if pin_receipt.exists():
    pr=load(pin_receipt);exact_pin=root/pr['path']
    if sha(exact_pin.read_bytes())!=pr['sha256']:raise ValueError('Historical pin01 hash changed')
  if i==n:exact_pin=args.pin_used
  if exact_pin.exists():
   pin_path=prefix+'-pin-used.toml';data=exact_pin.read_bytes();payloads[pin_path]=data
   pin_info={'path':pin_path,'sha256':sha(data),'literal_copy':True,'binding':'Explicit archived snapshot supplied by Root; not inferred from today\u2019s pin'}
  failures=[line for line in text.splitlines() if re.match(r'(?:test .* \.\.\. FAILED$|FAIL\b|FAIL:|error: test failed|error: \d+ targets failed)',line)]
  history.append({'attempt':i,'status':result['status'],'exit_code':result['exit_code'],'completed_utc':result['completed_utc'],
   'rust_passed':result['rust_passed'],'rust_failed':result['rust_failed'],'rust_ignored':result['rust_ignored'],
   'raw_result_sha256':sha(receipt.read_bytes()),'raw_log_sha256':sha(raw),
   'public_result_sha256':sha(safe_receipt),'public_log_gzip_sha256':sha(payloads[prefix+'.log.gz']),
   'public_log_decoded_sha256':sha(safe),'log_export_redacted':safe!=raw,
   'failure_lines':clean('\n'.join(failures),root).splitlines(),'pin_used':pin_info,
   'unknown_pin_policy':'Null means that attempt has no archived exact snapshot; no later pin is assigned retrospectively.'})
  if i==n:
   latest_log=text;payloads['log.gz']=payloads[prefix+'.log.gz'];payloads['source-result.json']=safe_receipt
   all_passed_tests=set(re.findall(r'^test (.*?) \.\.\. ok$',text,re.M))
 # All integrity facts are genuine fields of the selected result, never inferred
 # from passing test counts or the four short OR suites alone.
 runtime=args.runtime_before or base/f'runtime-before-check-final{n:02}.json'
 if n==1 and not runtime.exists():runtime=base/'runtime-before-build11.json'
 expected=load(runtime);fixtures=load(args.fixtures_before);combined={**expected,**fixtures}
 protected=load(base/'protected-traces-before.json')
 humans=final.get('human_traces_unchanged')
 stable=(final.get('runtime_sources_changed')==[] and final.get('new_acceptance_fixtures_changed')==[]
  and isinstance(humans,dict) and set(humans)==set(protected) and len(humans)==6
  and all(value is True for value in humans.values()) and final.get('new_acceptance_fixture_count')==len(fixtures))
 during=final.get('runtime_sources_unchanged_since_final_check_freeze',final.get('runtime_sources_unchanged_since_build11',False)) is True
 eof_complete=latest_log.rstrip().endswith('OK: check.sh completó sin errores.')
 gate=final['status']=='pass' and final['exit_code']==0 and stable and during and eof_complete
 binding=git_commit_binding(root,args.commit,combined)
 payloads['checked-runtime-inputs.json']=encoded(expected);payloads['checked-fixture-inputs.json']=encoded(fixtures)
 payloads['reference-pin-used.toml']=pin
 current_regressions=['original_single_double_tunnels_match_previous_tick_context_with_prior_limit','front_foreign_pickup_inventory_uses_original_ids_through_json_restore_and_delivery','own_permuted_section_delivery_keeps_original_vehicle_numbers_after_restore','native_manual_fireman_cannot_be_changed_to_an_unsupported_automatic_state','unloading_event_reports_the_minimum_across_both_supports']
 def seen(name):return any(actual==name or actual.endswith('::'+name) for actual in all_passed_tests)
 regression_results={name:seen(name) for name in current_regressions}
 mapped_names={e['test'] for i in matrix['issues'] for c in i['criteria'] for e in c['evidence'] if 'test' in e}
 gaps=[f'Final log does not contain actual PASS for {name}' for name in sorted(mapped_names|set(current_regressions)) if not seen(name)]
 if final['status']=='pass' and not eof_complete:gaps.append('Successful result lacks authentic check.sh completion marker at raw log EOF')
 eligible=gate and binding['verified'] and not gaps
 matrix['generated_at_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
 matrix['status']='derived_final_gate_matrix_not_a_GitHub_closure'
 matrix['all_close_now']=False;matrix['github_closure_authority']='Root only; this publisher never sends GitHub close commands.'
 matrix['global_gate']={'status':final['status'],'exit_code':final['exit_code'],'attempt':n,'integrity_accepted':gate,'commit_binding':binding,'eligible_for_root_closure':eligible}
 matrix['explicit_open_issues']=[] if eligible else [i['issue'] for i in matrix['issues']]
 matrix['final_gate_current_regressions']=regression_results
 matrix['final_gate_concrete_gaps']=gaps
 staged={'tunnel_ci_pending':('tunnel_ci_final',['original_single_double_tunnels_match_previous_tick_context_with_prior_limit']),
  'final_review_regressions_pending':('final_review_regressions_final',current_regressions[1:])}
 for old,(new,tests) in staged.items():
  if old in matrix['evidence']:
   value=matrix['evidence'].pop(old);value['scope_at_draft']=value.pop('scope')
   value['scope']='Same declared subsystem/regression scope; current execution is bound to the actual final-check log below.'
   value['proposal_execution_status']=value.get('execution_status')
   value['execution_status']='pass' if all(seen(t) for t in tests) else 'missing_actual_pass'
   value['final_execution']={'attempt':n,'tests':{t:seen(t) for t in tests},'raw_log_sha256':final['log_sha256']}
   matrix['evidence'][new]=value
 for i in matrix['issues']:
  for c in i['criteria']:
   for e in c['evidence']:
    if e['evidence_id'] in staged:e['evidence_id']=staged[e['evidence_id']][0]
 for issue in matrix['issues']:
  issue['close_now']=False
  issue['eligible_for_root_closure']=eligible
  issue['proposed_status']='scope_evidence_and_gate_verified_pending_root_closure' if eligible else 'awaiting_successful_final_gate_or_commit'
  issue['pending']=[] if eligible else (['Root successful full check and exact checked-source commit binding.']+gaps)
  for criterion in issue['criteria']:
   criterion['status']='verified_in_declared_scope_pending_root_closure' if eligible else ('scope_and_global_gate_passed_pending_commit' if gate else 'scope_evidence_available_global_gate_incomplete')
   criterion['final_gate_status']='pass' if gate else final['status']
   criterion['final_gate_evidence']={'receipt':'result.json','history_attempt':n,'raw_log_sha256':final['log_sha256']}
   criterion['named_test_passes_in_final_log']={e['test']:seen(e['test']) for e in criterion['evidence'] if 'test' in e}
   criterion['close_now']=False
 # Preserve the original private draft as a separate sanitized source artifact.
 payloads['proposal-source.json']=clean(args.matrix.read_text(),root).encode()
 payloads['matrix.json']=clean(encoded(matrix).decode(),root).encode()
 payloads['history.json']=encoded(history)
 payloads['publisher.py']=Path(__file__).read_bytes()
 manifest={name:{'sha256':sha(data),'bytes':len(data)} for name,data in sorted(payloads.items())}
 report={'schema':1,'kind':'final_check_publication','status':final['status'],'exit_code':final['exit_code'],'attempt':n,
  'completed_utc':final['completed_utc'],'published_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
  'raw_result_sha256':sha(args.check_result.read_bytes()),'raw_log_sha256':final['log_sha256'],
  'raw_pin_used_sha256':sha(pin),'pin_snapshot_literal':True,
  'rust_passed':final['rust_passed'],'rust_failed':final['rust_failed'],'rust_ignored':final['rust_ignored'],
  'runtime_fixture_human_trace_integrity_accepted':stable and during,'full_check_completion_marker_at_eof':eof_complete,'global_gate_accepted':gate,
  'commit_binding':binding,'eligible_for_root_closure':eligible,'all_close_now':False,
  'matrix_issues':20,'matrix_criteria':60,'current_regressions':regression_results,'concrete_gaps':gaps,
  'scope':'Actual whole check result, including late oracle/coverage failures. Rust/Python/short-oracle subsets do not turn a nonzero complete run into PASS. Full LIRR/steam measurements remain separately versioned evidence.',
  'log_export':'Lossless gzip of sanitized UTF-8 log derivative,mtime0. Original private raw log hash retained; exported/decoded hashes are distinct.',
  'global_pin_policy':'This final receipt,logs,matrix and self-publisher are deliberately NOT appended to the global reference pin. Its exact used snapshot is copied here without modifying that pin.',
  'files':manifest}
 payloads['result.json']=encoded(report)
 args.out.parent.mkdir(parents=True,exist_ok=True)
 with tempfile.TemporaryDirectory(prefix='.final-check-publication-',dir=args.out.parent) as folder:
  stage=Path(folder)/'complete';stage.mkdir()
  for name,data in payloads.items():
   file=stage/name;file.parent.mkdir(parents=True,exist_ok=True);file.write_bytes(data)
  stage.rename(args.out)
 print(json.dumps({'path':str(args.out/'result.json'),'sha256':sha((args.out/'result.json').read_bytes()),'status':final['status'],'exit_code':final['exit_code'],'eligible_for_root_closure':eligible,'global_pin_modified':False}))
if __name__=='__main__':main()
