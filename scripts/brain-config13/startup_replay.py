"""Actual live Memory startup owners; bounded per-role state contracts only."""
from pathlib import Path
import argparse,base64,copy,gzip,hashlib,json,os,re,shutil,subprocess,time,sys
import startup_cases as fixtures
import startup_state as state
from startup_fixture_admission import UnavailableRawFixture


def primary(case,paths):
    if 'resolved'in case:return case['resolved']
    if case['kind']in('generated','file'):return state.trim(Path(paths['secret']).read_bytes())
    env=case.get('env',{})
    if case['id']=='configured-literal-before-env':return b'owned-config-key'
    if case['id']=='project-secret-over-global':return b'project-key'
    if 'SYMMEMORY_JWT_SECRET'in env:return os.fsencode(env['SYMMEMORY_JWT_SECRET'])
    if 'OWNED_JWT'in env:return os.fsencode(env['OWNED_JWT'])
    return os.fsencode(env.get('JWT_SECRET_KEY',''))


def sql_comparison(observation):
    sql=copy.deepcopy(observation['sql'])
    if not sql or 'tables'not in sql:return sql
    ledger=state.ledger_interval(sql,*observation['actual_interval'])
    if ledger:
        assert all(check['within_actual_process_interval']for check in ledger['checks']),ledger
        sql['tables']['schema_migrations']['rows']=ledger['versions']
    # Raw DDL and observer-connection pragmas remain verbatim in the report.
    # Acceptance covers exact column/default/FK/index facts, application rows,
    # actual FTS queries/integrity and persistent engine properties. Source-bound
    # live constructor unit probes separately establish per-connection flags.
    # Never discard schema programs. Tables/columns/indexes are represented
    # below; triggers/views need their own complete identity/body contract.
    sql['program_inventory']=state.program_inventory(sql.get('raw_schema',[]))
    sql.pop('raw_schema',None);sql.pop('observer_connection_pragmas',None)
    return sql


def compare(go,native,case,paths,root):
    failures=[]
    for key in ['exit','stdout_base64','stderr_base64','before']:
        if go[key]!=native[key]:failures.append(key)
    if set(go['after'])!=set(native['after']):failures.append('complete-file-set')
    generated=case['kind']=='generated'
    special={os.fsencode(Path(paths[k]).relative_to(root)).hex():k for k in ['secret','database','rotation']}
    for suffix in ['-wal','-shm']:
        special[os.fsencode(Path(paths['database']+suffix).relative_to(root)).hex()]=suffix
    for path in set(go['after'])&set(native['after']):
        left,right=go['after'][path],native['after'][path]
        if (left['kind'],left['mode'])!=(right['kind'],right['mode']):failures.append('type/mode:'+path)
        role=special.get(path)
        if left['kind']=='file'and not(role in('database','-wal','-shm')or role=='secret'and generated or role=='rotation'and go.get('crypto',{}).get('exit')==0):
            if left['sha256']!=right['sha256']:failures.append('readonly/full-bytes:'+path)
    contracts=[{k:v for k,v in (o.get('key_contract') or {}).items()if k!='raw_entropy'}for o in [go,native]]
    if contracts[0]!=contracts[1]:failures.append('key-contract')
    if generated:
        for label,contract in zip(['go','native'],contracts):
            if not contract.get('lowercase_hex32_plus_newline')or contract.get('length')!=65:failures.append(label+':invalid-generated-key-format')
    if go.get('crypto')or native.get('crypto'):
        for key in ['exit','stdout_base64','stderr_base64']:
            if go.get('crypto',{}).get(key)!=native.get('crypto',{}).get(key):failures.append('crypto:'+key)
    for label,observation in [('go',go),('native',native)]:
        if observation['wal']and not observation['wal']['valid']:failures.append(label+':WAL-checksum/structure')
        if observation['shm']and not observation['shm']['valid']:failures.append(label+':SHM-header/WAL-identity')
        if observation['sql']and observation['sql'].get('integrity')!=[('ok',)]:failures.append(label+':sqlite-integrity')
    try:
        if sql_comparison(go)!=sql_comparison(native):failures.append('full-SQL-facts/defaults/appRows/FTS')
    except AssertionError:failures.append('ledger-actual-interval')
    return failures


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['go','native','sdk','root','out','secret-peer']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--head',required=True)
    p.add_argument('--case')
    p.add_argument('--corrections',action='store_true')
    p.add_argument('--final-corrections',action='store_true')
    p.add_argument('--fixture-sdk',type=Path)
    p.add_argument('--control',choices=['input-key','schema','mode','missing-key','missing-trigger','changed-trigger','added-view','changed-view','memory-config-database','memory-config-key'])
    a=p.parse_args();a.out.mkdir(exist_ok=False)
    assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=a.root,text=True).strip()==a.head
    assert not subprocess.check_output(['git','status','--porcelain'],cwd=a.root)
    assert shutil.disk_usage(a.out).free>=700*1024*1024
    rows=[];keys=[];salts=[];report=dict(source_head=a.head,go_sha256=state.sha(a.go.read_bytes()),native_sha256=state.sha(a.native.read_bytes()),sdk_sha256=state.sha(a.sdk.read_bytes()),secret_peer_sha256=state.sha(a.secret_peer.read_bytes()),runner_sha256=state.sha(Path(__file__).read_bytes()),actual_platform=os.name,rows=rows,control=a.control,scope='Owned Memory live constructors; every raw byte retained. Only generated key/engine role entropy and interval-bound schema_migrations.applied_at have explicit local observation contracts; original strict102 snapshots untouched.')
    def persist():
        report.update(total=len(rows),executed=sum('differences' in r for r in rows),unavailable=sum(r.get('status')=='UNEXECUTED' for r in rows),equal=sum('differences' in r and not r['differences'] for r in rows),generated_key_repetition_unique=len(keys)==len(set(keys)),rotation_salt_nonce_repetition_unique=len(salts)==len(set(salts)))
        (a.out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    if a.corrections or a.final_corrections:
        import startup_correction_cases
        if a.final_corrections:
            import startup_final_cases
            planned=startup_final_cases
        else:planned=startup_correction_cases
        assert a.fixture_sdk and a.fixture_sdk.is_file()
        report['fixture_sdk_sha256']=state.sha(a.fixture_sdk.read_bytes())
    else:planned=fixtures
    for case in planned.cases():
        if a.case and case['id']!=a.case:continue
        selected_control_case = ('final-memory-marker-global-fffe' if a.control in ('memory-config-database','memory-config-key') else 'legacy-rotation-valid' if a.control=='input-key' else 'generated-default')
        if a.control and case['id'] != selected_control_case: continue
        root=a.out/'owned-live';row=dict(case=json.loads(json.dumps(case,default=lambda value: {'bytes_base64':state.b64(value)})),observations={});rows.append(row)
        for label,binary in [('go',a.go),('native',a.native)]:
            assert not root.exists()
            try: paths=planned.fixture(root,case)
            except UnavailableRawFixture as error:
                assert not a.control and not row['observations'], 'control/partly executed fixture is not unavailable'
                assert case['id'] == 'corrected-secret-nested-raw-blocker'
                row.update(status='UNEXECUTED', fixture_admission=error.observation,
                    fixture_state_before_cleanup=state.snapshot(root,a.out))
                shutil.rmtree(root)
                assert not root.exists()
                row['fixture_cleanup_complete']=True
                persist()
                break

            if 'encrypted_plaintext'in case:
                data=json.dumps(dict(Primary=state.b64(primary(case,paths)),Plaintext=state.b64(case['encrypted_plaintext']))).encode()
                built=subprocess.run([str(a.fixture_sdk.resolve())],input=data,capture_output=True,timeout=10)
                assert built.returncode==0 and not built.stderr and len(built.stdout)>=44
                rotation=Path(paths['rotation']);rotation.write_bytes(built.stdout);rotation.chmod(0o640)
                row.setdefault('authenticated_fixture',{})[label]=dict(input=state.retain(data,a.out),payload=state.retain(built.stdout,a.out))
            row['case']=json.loads(json.dumps(case,default=lambda value:{'bytes_base64':state.b64(value)}))
            base={k:v for k,v in os.environ.items()if not k.startswith(('SYMBRAIN_','SYMMEMORY_','XDG_','JWT_','OWNED_'))and k not in('HOME','USERPROFILE','PWD','HOMEDRIVE','HOMEPATH')}
            base.update(HOME=str(root/'home'),USERPROFILE=str(root/'home'),XDG_CONFIG_HOME=str(root/'config'),XDG_DATA_HOME=str(root/'data'),XDG_CACHE_HOME=str(root/'cache'),PATH='',SYMBRAIN_GO_BINARY=str(root/'missing-fallback'))
            base.update(case.get('env',{}));base.update(case.get('original_case',{}).get('env',{}))
            if 'provider'in case:
                peer=root/'owned-provider';peer.mkdir();binary_dir=root/'bin';binary_dir.mkdir()
                executable=binary_dir/('symvault.exe'if os.name=='nt'else'symvault');shutil.copyfile(a.secret_peer,executable);executable.chmod(0o755)
                for name in ['stdout','stderr']:(peer/name).write_bytes(case['provider'][name])
                (peer/'exit').write_text(str(case['provider']['code']))
                base.update(PATH=str(binary_dir),OWNED_JWT_PEER_ROOT=str(peer))
                if 'signal'in case['provider']:
                    assert os.name!='nt'
                    # A real owned provider resets inherited SIGPIPE explicitly.
                    # No process-global handler or production runner is changed.
                    source=('#!'+sys.executable+'\n'
                        'import json,os,pathlib,resource,signal,sys\n'
                        'root=pathlib.Path(os.environ["OWNED_JWT_PEER_ROOT"])\n'
                        'root.joinpath("actual-argv.json").write_bytes(json.dumps([list(os.fsencode(a)) for a in sys.argv[1:]]).encode())\n'
                        'os.write(1,root.joinpath("stdout").read_bytes())\n'
                        'os.write(2,root.joinpath("stderr").read_bytes())\n'
                        'resource.setrlimit(resource.RLIMIT_CORE,(0,0))\n'
                        'sig=getattr(signal,"SIG'+case['provider']['signal']+'")\n'
                        'if sig != signal.SIGKILL: signal.signal(sig,signal.SIG_DFL)\n'
                        'os.kill(os.getpid(),sig)\n'
                        'raise AssertionError("signal provider survived")\n').encode()
                    executable.write_bytes(source);executable.chmod(0o755)
                    row.setdefault('signal_provider_source',{})[label]=state.retain(source,a.out)
            if label=='native'and a.control=='input-key':base['JWT_SECRET_KEY']='owned-mutated-key'
            if label=='native'and a.control=='memory-config-database':base['SYMMEMORY_DATABASE_PATH']='owned-mutated.db'
            if label=='native'and a.control=='memory-config-key':base['SYMMEMORY_JWT_SECRET']='owned-mutated-key'
            before=state.snapshot(root,a.out);start=time.time()
            actual=subprocess.run([str(binary.resolve()),'mcp','--profile-file',str(root/'profile.toml')],cwd=root/'project',env=base,input=b'',capture_output=True,timeout=20);end=time.time()
            if label=='native'and a.control=='schema':
                import sqlite3
                connection=sqlite3.connect(paths['database']);connection.execute('CREATE TABLE owned_schema_mutation(value TEXT DEFAULT \'wrong\')');connection.close()
            if label=='native'and a.control=='mode':Path(paths['secret']).chmod(0o644)
            if label=='native'and a.control=='missing-key':Path(paths['secret']).unlink()
            if a.control in ('missing-trigger','changed-trigger','added-view','changed-view'):
                import sqlite3
                connection=sqlite3.connect(paths['database'])
                if a.control in ('added-view','changed-view'):
                    # Identical baseline view, then a real native-only change.
                    connection.execute('CREATE VIEW owned_acceptance_view AS SELECT id,content FROM memories')
                    if label=='native'and a.control=='changed-view':
                        connection.execute('DROP VIEW owned_acceptance_view')
                        connection.execute('CREATE VIEW owned_acceptance_view AS SELECT id,scope AS content FROM memories')
                    if label=='native'and a.control=='added-view':
                        connection.execute('CREATE VIEW owned_extra_view AS SELECT id FROM memories')
                elif label=='native':
                    connection.execute('DROP TRIGGER trg_memories_oplog_insert')
                    if a.control=='changed-trigger':
                        connection.execute("CREATE TRIGGER trg_memories_oplog_insert AFTER INSERT ON memories BEGIN INSERT INTO sync_oplog(op,memory_id) VALUES ('delete',NEW.id); END")
                connection.commit();connection.close()
            after=state.snapshot(root,a.out)
            if case.get('no_provider'):assert not(root/'owned-provider/actual-argv.json').exists(),'validation accessed credential subprocess'
            key_file=Path(paths['secret']);key_contract=None
            if case['kind']=='generated'and key_file.is_file():
                raw=key_file.read_bytes();keys.append(raw)
                key_contract=dict(length=len(raw),lowercase_hex32_plus_newline=bool(re.fullmatch(b'[0-9a-f]{64}\n',raw)),raw=state.retain(raw,a.out))
            elif case['kind']=='file'and key_file.is_file():
                key_contract=dict(original_file_unchanged=key_file.read_bytes()==case['secret'])
            obs=dict(actual_interval=[start,end],exit=actual.returncode,stdout_base64=state.b64(actual.stdout),stderr_base64=state.b64(actual.stderr),before=before,after=after,actual_environment={k:v for k,v in base.items()if k.startswith(('SYMMEMORY_','JWT_','OWNED_'))},key_contract=key_contract,sql=state.sql_state(Path(paths['database']),a.out/'owned-observer'),wal=state.wal_state(Path(paths['database']),a.out),shm=state.shm_state(Path(paths['database']),a.out))
            if Path(paths['rotation']).is_file():
                payload=Path(paths['rotation']).read_bytes();secret=primary(case,paths)
                sdk_input=json.dumps(dict(Primary=state.b64(secret),Payload=state.b64(payload))).encode()
                result=subprocess.run([str(a.sdk.resolve())],input=sdk_input,capture_output=True,timeout=10)
                obs['crypto']=dict(exit=result.returncode,stdout_base64=state.b64(result.stdout),stderr_base64=state.b64(result.stderr),raw_sdk_input=state.retain(sdk_input,a.out),raw_payload=state.retain(payload,a.out))
                # Existing authenticated fixtures use deliberately fixed test
                # bytes. Only actual owner-created envelopes test CSPRNG reuse.
                original_envelope=row.get('authenticated_fixture',{}).get(label)
                unchanged_fixture=original_envelope and payload==gzip.decompress(Path(original_envelope['payload']['gzip']).read_bytes())
                obs['crypto']['unchanged_authenticated_fixture']=bool(unchanged_fixture)
                if result.returncode==0 and not unchanged_fixture:salts.append(payload[:28])
            row['observations'][label]=obs;persist();shutil.rmtree(root)
        if row.get('status') == 'UNEXECUTED': continue
        go,native=row['observations']['go'],row['observations']['native']
        # Retain raw entropy in each record while comparing only its format;
        # equality of generated random bytes is neither required nor fabricated.
        for observation in [go,native]:
            contract=observation.get('key_contract')
            if contract and 'raw'in contract:contract['raw_entropy']=contract.pop('raw')
        row['differences']=compare(go,native,case,paths,root)
        persist()
    persist()
    assert rows and all('differences'in row or row.get('status')=='UNEXECUTED' for row in rows)
    if a.control:
        assert len(rows)==1 and rows[0]['differences'],'actual owner/input/state mutant escaped'
        if a.control=='memory-config-database':assert 'complete-file-set' in rows[0]['differences']
        if a.control=='memory-config-key':assert 'stderr_base64' in rows[0]['differences']
        if a.control in ('missing-trigger','changed-trigger','added-view','changed-view'):
            assert 'full-SQL-facts/defaults/appRows/FTS'in rows[0]['differences']
            left,right=[state.program_inventory(rows[0]['observations'][label]['sql']['raw_schema']) for label in ('go','native')]
            assert left!=right,'intended schema-program mutation absent'
    else:
        assert all(not row.get('differences',[])for row in rows),[(row['case']['id'],row['differences'])for row in rows if row.get('differences')]
        assert report['generated_key_repetition_unique']and report['rotation_salt_nonce_repetition_unique']

if __name__=='__main__':main()
