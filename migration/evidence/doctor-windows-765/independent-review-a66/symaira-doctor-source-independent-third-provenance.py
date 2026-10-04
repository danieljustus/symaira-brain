from pathlib import Path
import base64,gzip,hashlib,json,subprocess,zipfile
repo=Path('/workspace/symaira-doctor765-windows');digest=lambda b:hashlib.sha256(b).hexdigest()
publication='34394496b44d0f95581080885d89d4ed5f43bbd5';source='a66b4818f74943812315509a3ba2545b2b6b2ea1'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==publication
assert not subprocess.check_output(['git','status','--porcelain'],cwd=repo)
assert not subprocess.check_output(['git','diff',source,publication,'--','rust','scripts','.github','Cargo.toml','Cargo.lock'],cwd=repo)
subprocess.run(['git','merge-base','--is-ancestor','e3dbda6cbb95429237d15a9b176209b7d107792c',publication],cwd=repo,check=True)
final=repo/'migration/evidence/doctor-windows-765/final-a66';v=json.loads((final/'validation.json').read_bytes())
assert v['source_head']==source and v['rust_binary_sha256']=='a2f9be69dbf324e12a20b5ed3e573f06bb9c66750fec2c3a2a16bd4716bdfc52'
for name,sha in v['artifacts'].items():
    assert digest((final/name).read_bytes())==sha,name
    original=Path('/tmp')/name
    if original.is_file():assert digest(original.read_bytes())==sha,name
logs=json.loads((final/'logs.json').read_bytes())
for name,item in logs.items():
    b=base64.b64decode(item['original_bytes_base64']);assert digest(b)==item['sha256']
    assert (Path('/tmp')/name).read_bytes()==b,name
for name,item in v['actual_test_cli_executables'].items():
    b=Path(name).read_bytes();assert digest(b)==item['sha256'] and len(b)==item['bytes'],name
assert digest(Path(v['preserved_actual_cli']).read_bytes())==v['rust_binary_sha256']
counts={};archive_counts={}
for root,key,expected in [(repo/'migration/evidence/doctor-windows-765/independent-review-a301','original_files',46),(repo/'migration/evidence/doctor-windows-765/independent-review-0b56','original_files',35),(repo/'migration/evidence/setup-source-765/independent-review-86ea','originals',24)]:
    retained=json.loads((root/'retention.json').read_bytes())[key];assert len(retained)==expected
    for name,item in retained.items():
        p=root/name
        b=p.read_bytes() if p.is_file() else base64.b64decode(json.loads((root/(name+'.json')).read_bytes())['original_bytes_base64'])
        assert digest(b)==item['sha256'],name
        if 'bytes' in item:assert len(b)==item['bytes']
        original=Path('/tmp')/name
        if original.is_file():assert original.read_bytes()==b,name
    counts[str(root)]=len(retained)
for name,expected in [('a301',38),('0b56',41)]:
    root=repo/f'migration/evidence/doctor-windows-765/independent-review-{name}'
    archive=json.loads((root/f'binary-retention-{name}.json').read_bytes())['executables'];assert len(archive)==expected
    for item in archive.values():
        compressed=Path(item['gzip_archive']).read_bytes();assert digest(compressed)==item['archive_sha256']
        b=gzip.decompress(compressed);assert digest(b)==item['original_sha256'] and len(b)==item['original_bytes']
    archive_counts[name]=len(archive)
windows=repo/'migration/evidence/doctor-windows-765/original-windows-1288';w=json.loads((windows/'receipt.json').read_bytes())
for name,item in w.items():
    if not isinstance(item,dict) or 'sha256' not in item:continue
    p=windows/item.get('stored_path',name);b=base64.b64decode(json.loads(p.read_bytes())['original_bytes_base64']) if item.get('stored_encoding') else p.read_bytes();assert digest(b)==item['sha256'],name
original=json.loads((windows/'windows-process.json').read_bytes());five=json.loads((windows/'five-full-failure-records.json').read_bytes())
assert len(five['failures'])==5 and five['failures']==original['failures'] and five['observations']==[x for x in original['observations'] if not x['matches']]
with zipfile.ZipFile(windows/'doctor-windows-failure.zip') as z:assert any(z.read(name)==(windows/'windows-process.json').read_bytes() for name in z.namelist() if name.endswith('.json'))
previous=repo/'migration/evidence/doctor-windows-765/final-a301';named={};manifests={}
for family in ['source','doctor','setup']:
    new=json.loads((final/f'symaira-doctor765-owner-a66-{family}.json').read_bytes())
    old=json.loads((previous/f'symaira-doctor765-home-a301-{family}.json').read_bytes())
    old_names=set(old['cases'] if family=='setup' else [x['case'] for x in old['observations']]);new_names=set(new['cases'] if family=='setup' else [x['case'] for x in new['observations']])
    assert old_names<=new_names
    named[family]=dict(original=len(old_names),new=len(new_names),retained=True)
    manifests[family]={}
    for key in ['candidate_source_sha256','go_source_sha256']:
        manifests[family][key]=len(new[key])
        for name,expected in new[key].items():
            b=(repo/name).read_bytes() if key.startswith('candidate') else subprocess.check_output(['git','show','dcddcef0df5789123c7c9a7ebe6e01f10e941f2c:'+name],cwd=repo)
            assert digest(b)==expected,(family,name)
sdk=Path('/workspace/toolchains/go1.26.7/src')
for name,expected in v['SDK_path_and_mkdir_inputs'].items():assert digest((sdk/name).read_bytes())==expected
typed=json.loads((final/'signature-stub-harness.json').read_bytes())
for item in typed['files'].values():assert digest(item['text'].encode())==item['sha256']
result=dict(head=publication,source=source,author_actual_artifacts_verified=len(v['artifacts']),author_encoded_raw_logs_verified=len(logs),author_current_actual_binaries_verified=len(v['actual_test_cli_executables']),old_review_file_counts=counts,previous_executable_archive_counts=archive_counts,
            original_windows_zip_and_all_five_failures_verified=True,original_windows_process_sha256=digest((windows/'windows-process.json').read_bytes()),named_case_retention=named,author_manifest_counts=manifests,sdk_source_inputs_verified=5,typed_stub_source_hashes_verified=True,native_windows_macos_runtime_verified=False,rust_sha256=v['rust_binary_sha256'])
Path('/tmp/symaira-doctor-source-independent-third-provenance.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
