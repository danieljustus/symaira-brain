from pathlib import Path
import base64,hashlib,json,os,re,subprocess
repo=Path('/workspace/symaira-doctor765-windows');digest=lambda b:hashlib.sha256(b).hexdigest();prefix='/tmp/symaira-doctor-source-independent-third-'
head='34394496b44d0f95581080885d89d4ed5f43bbd5';source='a66b4818f74943812315509a3ba2545b2b6b2ea1'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==head
assert not subprocess.check_output(['git','status','--porcelain'],cwd=repo)
assert not subprocess.check_output(['git','diff',source,head,'--','rust','scripts','.github','Cargo.toml','Cargo.lock'],cwd=repo)
controls=[];gates={};manifests={}
for family,expected,modes in [('source',107,['wrong-exit','wrong-source']),('doctor',282,['wrong-exit','missing-header','missing-core-event']),('setup',169,[])]:
    p=Path(prefix+family+'.json');d=json.loads(p.read_bytes())
    assert d['candidate_head']==head and not d['candidate_dirty'] and d['total']==expected and d['exit']==0
    if family=='setup':assert d['matches'] is True and len(d['cases'])==expected and len(d['observations'])==2*expected
    else:assert d['complete_observations']==d['matched']==expected and not d['failures']
    manifests[family]={}
    for key in ['candidate_source_sha256','go_source_sha256']:
        manifests[family][key]=len(d[key])
        for name,expected_sha in d[key].items():
            b=(repo/name).read_bytes() if key.startswith('candidate') else subprocess.check_output(['git','show','dcddcef0df5789123c7c9a7ebe6e01f10e941f2c:'+name],cwd=repo)
            assert digest(b)==expected_sha,(family,name)
    gates[family]=dict(total=expected,path=str(p),sha256=digest(p.read_bytes()))
    for mode in modes:
        p=Path(prefix+family+'.json.'+mode+'.json');q=json.loads(p.read_bytes());field={'wrong-exit':'exit','wrong-source':'stdout_base64','missing-header':'stdout_base64','missing-core-event':'logs'}[mode]
        assert q['total']==q['complete_observations']==1 and q['matched']==0 and q['exit']==1
        assert q['failures']==[{'case':'explicit-browse-json' if family=='source' else 'correct','fields':[field]}]
        controls.append(dict(family=family,mode=mode,intended_failure=q['failures'],path=str(p),sha256=digest(p.read_bytes())))
original_pairs=0
for name in ['extra','home','boundaries','symlink','original-findings','original-lookup','original-raw-signals']:
    d=json.loads(Path(prefix+name+'.json').read_bytes())
    assert d.get('head',d.get('candidate_head'))==head and not d.get('dirty',d.get('candidate_dirty',False))
    for value in d.values():
        if isinstance(value,list):
            for row in value:
                if isinstance(row,dict) and ('mismatched_fields' in row or 'differences' in row):assert not row.get('mismatched_fields',row.get('differences'));original_pairs+=1
    if name=='original-raw-signals':assert len(d['actual_cancellation'])==2 and all(not row['descendant_active'] and row['actual_cli_exit']==1 for row in d['actual_cancellation'])
assert original_pairs==35
log=Path(prefix+'tests.log').read_bytes();summaries=[tuple(map(int,x)) for x in re.findall(rb'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)]
assert len(summaries)==40 and tuple(map(sum,zip(*summaries)))==(402,0,0)
for name in ['clippy','build']:assert b'Finished' in Path(prefix+name+'.log').read_bytes()
for name in ['fmt','actionlint','whitespace']:assert not Path(prefix+name+'.log').read_bytes()
binaries={name:digest(Path(name).read_bytes()) for name in re.findall(r'Running [^\n]+ \(([^\n]+)\)',log.decode())}
cli=Path('/workspace/symaira-setup765-source/target/debug/symbrain');binaries[str(cli)]=digest(cli.read_bytes());assert len(binaries)==38
assert binaries[str(cli)]=='a2f9be69dbf324e12a20b5ed3e573f06bb9c66750fec2c3a2a16bd4716bdfc52'
author=json.loads((repo/'migration/evidence/doctor-windows-765/final-a66/validation.json').read_bytes())
assert binaries=={name:item['sha256'] for name,item in author['actual_test_cli_executables'].items()}
setup_extra=[]
for label in ['consumers','fix-consumers']:
    d=json.loads(Path(prefix+label+'-setup.json').read_bytes());assert d['candidate_head']==head and not d['candidate_dirty'] and d['total']==8 and not d['matches'] and d['exit']==1
    for i,name in enumerate(d['cases']):
        a,b=d['observations'][2*i:2*i+2]
        normalized=lambda x,k:base64.b64decode(x[k]).replace(x['fixture_root'].encode(),b'<root>')
        same_stdout=normalized(a,'stdout_base64')==normalized(b,'stdout_base64')
        assert a['exit']==b['exit'] and normalized(a,'stderr_base64')==normalized(b,'stderr_base64')
        assert a['filesystem_after']==b['filesystem_after'] and a['filesystem_before']==b['filesystem_before']
        assert same_stdout==name.endswith('json-False')
        if not same_stdout:
            go=normalized(a,'stdout_base64');rust=normalized(b,'stdout_base64')
            assert json.loads(go)==json.loads(rust)
            assert all(c in go for c in [b'\\u0026',b'\\u003c',b'\\u003e',b'\\u2028',b'\\u2029'])
        setup_extra.append(dict(family=label,case=name,go_exit=a['exit'],rust_exit=b['exit'],stdout_match=same_stdout,stderr_and_full_state_match=True))
consumer_good=0
for family in ['source','doctor']:
    d=json.loads(Path(prefix+'consumers-'+family+'.json').read_bytes());assert d['head']==head and not d['dirty']
    assert len(d['cases'])==(4 if family=='source' else 3)
    for row in d['cases']:assert not row['differences'];consumer_good+=1
baseline=json.loads(Path(prefix+'baseline-consumers-setup.json').read_bytes());assert baseline['rust_binary_sha256']=='44ffc3c6ecd498aa29aa82dd828abeeef161ca073f14990ed695b9fb9c681b80'
baseline_case=baseline['cases'].index('raw-install-None-lexical-False-json-True');a,b=baseline['observations'][2*baseline_case:2*baseline_case+2]
assert a['exit']==b['exit']==0
normalize=lambda x:base64.b64decode(x['stdout_base64']).replace(x['fixture_root'].encode(),b'<root>')
assert json.loads(normalize(a))==json.loads(normalize(b)) and normalize(a)!=normalize(b)
windows_generic=json.loads(Path(prefix+'windows-lexical.json').read_bytes());assert windows_generic['cases']==189679 and len(windows_generic['differences'])==101
windows_managed=json.loads(Path(prefix+'windows-managed-join.json').read_bytes());assert windows_managed['cases']==189679
assert windows_managed['differences']==[{'input_hex':'','sdk_hex':'2e73796d616972615c62696e','rust_hex':'5c2e73796d616972615c62696e'}]
assert '.filter(|home| !home.is_empty())?' in (repo/'rust/symbrain-cli/src/managed_home.rs').read_text()
users=[];target='/workspace/symaira-setup765-source/target'
for p in Path('/proc').iterdir():
    if not p.name.isdigit() or int(p.name)==os.getpid():continue
    for item in [p/'exe',p/'cwd']+list((p/'fd').glob('*')):
        try:
            actual=os.readlink(item)
            if actual.startswith(target):users.append(dict(pid=p.name,path=actual))
        except OSError:pass
assert not users,users
changed=subprocess.check_output(['git','diff','--name-only','9705777b09bbfe600406ad60ba704d0fd8759e48',source,'--','rust','scripts','docs/adr/2026-10-03-managed-owner-clean-and-mkdir.md','migration/contract-matrix.csv','.github/workflows/ci.yml'],cwd=repo,text=True).splitlines()
inputs={name:digest((repo/name).read_bytes()) for name in changed if (repo/name).is_file()}
assert not [name for name in changed if name.endswith('.go') or name.startswith('rust/symbrain-cli/tests/fixtures/')]
artifacts={str(p):digest(p.read_bytes()) for p in sorted(Path('/tmp').glob('symaira-doctor-source-independent-third-*')) if p.is_file() and p.suffix in ['.py','.json','.log'] and 'review-receipt' not in p.name and 'report' not in p.name and 'verify.log' not in p.name}
result=dict(head=head,validated_source=source,normal_main='e3dbda6cbb95429237d15a9b176209b7d107792c',target_unchanged=True,target_released=True,target_users=users,
    gates=gates,controls=controls,manifest_counts=manifests,fresh_tests=dict(passed=402,failed=0,ignored=0,complete_summaries=40),binary_sha256=binaries,
    strict_clippy=True,fmt=True,actionlint_all_workflows=True,original_35_pairs_match=True,actual_SIGINT_SIGTERM_match=True,original_two_P2_groups_closed=True,
    extra_current_setup_pairs=16,extra_setup_json_mismatches=8,extra_setup_human_matches=8,extra_source_doctor_pairs_match=consumer_good,extra_setup_pair_details=setup_extra,
    inherited_setup_html_omission_confirmed_with_actual_a301_cli=True,windows_sdk_generic_clean_exploratory_mismatches=101,
    actual_windows_managed_join_admitted_nonempty_HOME_values=189678,actual_windows_managed_join_admitted_mismatches=0,
    windows_excluded_empty_HOME_precondition_verified=True,native_windows_macos_runtime_verified=False,provenance=json.loads(Path(prefix+'provenance.json').read_bytes()),inputs=inputs,artifacts=artifacts,
    disposition='REQUEST CHANGES: one remaining P2 shared Setup JSON report escaping omission; original owner-selection and raw MkdirAll findings closed.',
    reviewer_notes=['Fetch quiet hold paused only outer sequencing shell. Already admitted Source/Doctor gates finished unchanged; next commands resumed only after root release. Hold receipt records initial process-read race explicitly.',
    'Additional Setup consumer gate correctly failed four JSON pairs, preserving raw output/full state. Source/Doctor sections were executed separately unchanged afterward; no failed observation rewritten or hidden.',
    'Exploratory standalone generic Clean differs for101 inputs through SDK lazybuf/postClean behavior. Exact actual managed_path assembly +SDK Join comparison admits189678 nonempty HOME values and all match; one empty HOME difference is excluded by actual bin_dir filter before managed_path. Both failed exploratory probes and all outputs/source digests retained. Pure Linux algorithm execution is not native Windows runtime evidence.',
    'Actual archived a301 baseline proves HTML/separator omission inherited in Setup finish; this review does not mislabel it as a regression from lexical/mkdir correction. It remains a literal Go contract gap in the modified Setup report consumer.'])
Path(prefix+'review-receipt.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:result[k] for k in ['head','fresh_tests','manifest_counts','original_35_pairs_match','extra_setup_json_mismatches','actual_windows_managed_join_admitted_nonempty_HOME_values','target_users','disposition']},indent=2))
