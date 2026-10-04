#!/usr/bin/env python3
"""Frozen Go CLI versus the actual bounded native owner, not full serve admission."""
import argparse
import json
from pathlib import Path
import sqlite3
import subprocess
import time
import uuid
from datetime import datetime
from support import GO,GO_SHA,Peer,Embeddings,digest,token,owned_env,seed,snapshot

HEADERS=['content-type','x-content-type-options','x-frame-options','content-security-policy','access-control-allow-origin','access-control-allow-methods','access-control-allow-headers','vary','allow']

def semantic(row):
    raw=bytes.fromhex(row['body_hex'])
    try:body=json.loads(raw)
    except (ValueError,UnicodeError):body=raw.hex()
    headers={key.lower():value for key,value in row['headers'] if key.lower() in HEADERS}
    return dict(status=row['status'],body=body,headers=headers)

def run(args):
    repo=Path(__file__).resolve().parents[2];root=args.report.with_suffix('.evidence');root.mkdir(exist_ok=False)
    assert digest(GO)==GO_SHA,'frozen executable changed'
    embedding=Embeddings();peers=[];records=[];limits=[];shutdown=[]
    frozen=Path('/workspace/oracles/daemon772-go-source')
    frozen_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=frozen,text=True).strip()
    assert frozen_head=='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c'
    frozen_files=sorted((frozen/'internal/memory').rglob('*.go'))
    frozen_manifest={str(p.relative_to(frozen)):digest(p) for p in frozen_files}
    for name,sha in frozen_manifest.items():
        literal=subprocess.check_output(['git','show',frozen_head+':'+name],cwd=frozen)
        import hashlib
        assert hashlib.sha256(literal).hexdigest()==sha,name
    receipt=dict(frozen_go_revision=frozen_head,frozen_go_source_sha256=frozen_manifest,candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=repo)),control=args.control,go=str(GO),go_sha256=digest(GO),native=str(args.native),native_sha256=digest(args.native),head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),source_manifest={str(p.relative_to(repo)):digest(p) for p in [repo/'Cargo.lock',repo/'rust/symbrain-memory/Cargo.toml',*sorted((repo/'rust/symbrain-memory/src').rglob('*.rs')),*sorted((repo/'rust/symbrain-memory/assets').rglob('*'))] if p.is_file()},records=records,limits=limits,shutdown=shutdown)
    try:
        seedroot=root/'seed';seedenv=owned_env(seedroot,embedding.url);database,receipt['actual_go_seed']=seed(seedroot,seedenv)
        for native,label,binary in [(False,'go',GO),(True,'rust',args.native)]:
            owned=root/label;env=owned_env(owned,embedding.url);target=owned/'current.db';
            with sqlite3.connect(database) as original, sqlite3.connect(target) as copied:
                original.backup(copied)
                assert copied.execute('SELECT id FROM memories').fetchone()[0]=='owned-seed'
                assert copied.execute('SELECT count(*) FROM entities').fetchone()[0]==1
            
            if native and args.control=='wrong-secret':env['JWT_SECRET_KEY']='synthetic-deliberately-wrong-key'
            if native and args.control=='empty-state':
                with sqlite3.connect(target) as conn:conn.execute('DELETE FROM memories')
            peers.append(Peer(binary,owned,env,native,target))
        auth={'Authorization':'Bearer '+token()}
        def compare(name,path,method='GET',body=None,headers=None,expected=None,normalize=None):
            # Default burst is 20 and refill 100/minute in both actual owners.
            time.sleep(.61)
            pair=[peer.request(name,path,method,body,headers) for peer in peers]
            decoded=[semantic(row) for row in pair]
            if normalize:decoded=[normalize(row) for row in decoded]
            if normalize and name in ['list-after-get','list-post-legacy','search']:
                for raw_row in pair:
                    body=json.loads(bytes.fromhex(raw_row['body_hex']))
                    rows=[hit['memory'] for hit in body] if name=='search' else body
                    for memory in rows:
                        assert memory['id']=='owned-seed' and memory['access_count']==2
                        observed=datetime.fromisoformat(memory['last_access'].replace('Z','+00:00'))
                        assert observed.year>=2026 and observed.timestamp()<=time.time()+2
            row=dict(id=name,go=pair[0],rust=pair[1],match=decoded[0]==decoded[1],compared=decoded)
            records.append(row)
            assert row['match'],json.dumps(row,indent=2)
            if expected is not None:assert pair[0]['status']==expected,(name,pair[0])
            return pair
        for name,path in [('root','/'),('css','/style.css'),('js','/app.js')]:
            pair=[peer.request(name,path) for peer in peers];records.append(dict(id=name,go=pair[0],rust=pair[1],intentional_asset_delta=name in ['css','js']))
            assert all(row['status']==200 for row in pair)
            actual=bytes.fromhex(pair[1]['body_hex']);assert actual==(repo/'rust/symbrain-memory/assets/web'/('index.html' if name=='root' else 'style.css' if name=='css' else 'app.js')).read_bytes()
            if name=='root':assert pair[0]['body_hex']==pair[1]['body_hex']
            if name=='css':assert b''.join(bytes.fromhex(pair[0]['body_hex']).split())==b''.join(actual.split())
        for name,path,headers,status in [('host-denied','/',{'Host':'untrusted.invalid'},403),('host-loopback-v4','/',{'Host':'127.42.1.3:abc'},200),('host-loopback-v6','/',{'Host':'[::1]:55'},200),('host-casefold','/',{'Host':'LOCALHOST'},200),('missing-auth','/api/list',{},401),('invalid-auth','/api/list',{'Authorization':'Bearer invalid'},401),('public-status','/api/status',{},200),('unknown-api','/api/unknown',{},404)]:
            compare(name,path,headers=headers,expected=status,normalize=(lambda d:dict(d,body={k:v for k,v in d['body'].items() if k!='version'})) if name=='public-status' else None)
        for name,claims in [('expired',{'exp':1}),('wrong-issuer',{'iss':'other'}),('blank-subject',{'sub':' '}),('typed-exp',{'exp':'4102444800'}),('typed-iat',{'iat':True})]:
            compare(name,'/api/list',headers={'Authorization':'Bearer '+token(**claims)},expected=401)
        for name,path,method,headers,status in [('csrf-before-host','/', 'POST',{'Host':'untrusted.invalid'},403),('csrf-static','/', 'POST',{},403),('csrf-local','/', 'POST',{'Origin':'http://localhost:21'},200),('csrf-casefold-origin','/', 'POST',{'Origin':'http://LOCALHOST:21'},403),('cors-denied','/api/list','GET',dict(auth,Origin='https://untrusted.invalid'),403),('cors-preflight','/api/list','OPTIONS',{'Origin':'chrome-extension://owned'},200),('cors-extension','/api/list','GET',dict(auth,Origin='moz-extension://owned'),200)]:
            compare(name,path,method=method,headers=headers,expected=status)
        for name,path in [('list','/api/list'),('list-project','/api/list?scope=project'),('list-clamp','/api/list?limit=0'),('list-invalid-limit','/api/list?limit=abc'),('list-repeated-limit','/api/list?limit=1&limit=100'),('rules','/api/rules'),('entities','/api/entities'),('get-missing-id','/api/get'),('get-not-found','/api/get?id=owned-absent')]:
            compare(name,path,headers=auth)
        compare('get-existing','/api/get?id=owned-seed',headers=auth,expected=200)
        # Returned row precedes feedback; compare subsequent access count, not
        # two independently generated wall-clock instants.
        normalize_times=lambda d:dict(d,body=[{k:v for k,v in r.items() if k not in ['last_access','prev_access']} for r in d['body']])
        compare('list-after-get','/api/list',headers=auth,normalize=normalize_times)
        compare('list-post-legacy','/api/list','POST',headers=auth,normalize=normalize_times)
        for subject in ['reader','unknown-role']:
            compare('profile-denial-'+subject,'/api/set','POST',{'content':'Blue fox sample'},{'Authorization':'Bearer '+token(subject)},403)
        for path in ['/api/search','/api/set','/api/token/revoke']:
            compare('method-'+path,path,headers=auth,expected=405)
        for index,body in enumerate([b'{',b'{} {}',b'{"content":4}']):
            compare('json-invalid-'+str(index),'/api/set','POST',body,auth,400)
        compare('oversize','/api/set','POST',b'x'*(1048576+1),auth,413)
        pair=compare('search','/api/search','POST',{'query':'Blue fox sample','scope':'global'},auth,200,normalize=lambda d:dict(d,body=[dict(hit,memory={k:v for k,v in hit['memory'].items() if k not in ['last_access','prev_access']}) for hit in d['body']]))
        assert len(json.loads(bytes.fromhex(pair[0]['body_hex'])))==1,'empty search cannot prove nested UI shape'
        compare('search-empty','/api/search','POST',{'query':'Blue fox sample','scope':'project'},auth,200)
        pair=compare('set-direct','/api/set','POST',{'content':'Azure fox specimen','scope':'global','metadata':{'owned':'synthetic'}},auth,200,normalize=lambda d:dict(d,body=dict(d['body'],id='<per-process-uuid>')))
        ids=[json.loads(bytes.fromhex(row['body_hex']))['id'] for row in pair]
        assert all(str(uuid.UUID(identifier))==identifier and uuid.UUID(identifier).version==4 for identifier in ids)
        inserted=[]
        for peer,identifier in zip(peers,ids):
            with sqlite3.connect(peer.database) as conn:
                cols=[r[1] for r in conn.execute('pragma table_info(memories)')]
                row=dict(zip(cols,conn.execute('select * from memories where id=?',(identifier,)).fetchone()))
                assert row['valid_from']<=row['created_at']<=row['updated_at']
                for key in ['id','created_at','updated_at','valid_from']:row.pop(key)
                metadata=json.loads(row['metadata']);metadata.pop('observed_at');row['metadata']=metadata
                inserted.append(row)
                assert row['kind']=='' and row['created_by']=='owned-human' and metadata['source_tool']=='http'
        receipt['inserted_current_rows']=inserted;assert inserted[0]==inserted[1],inserted
        # Actual per-process generated ids are sent back to the same owner.
        time.sleep(.61)
        pair=[peer.request('delete-direct','/api/delete?id='+identifier,'DELETE',headers=auth) for peer,identifier in zip(peers,ids)]
        records.append(dict(id='delete-direct',go=pair[0],rust=pair[1],match=semantic(pair[0])==semantic(pair[1])))
        assert all(row['status']==200 for row in pair) and semantic(pair[0])==semantic(pair[1])
        audits=[]
        for peer,identifier in zip(peers,ids):
            with sqlite3.connect(peer.database) as conn:
                audit=conn.execute('SELECT action,memory_id,scope,session,actor,detail,target_type,target_id FROM audit_log ORDER BY rowid').fetchall()
                assert len(audit)==2 and all(row[1]==identifier for row in audit)
                audits.append([[*row[:1],'<bound-primary-id>',*row[2:]] for row in audit])
        receipt['http_audit_identity_bindings']=audits;assert audits[0]==audits[1]
        compare('delete-not-found' ,'/api/delete?id=owned-absent','DELETE',headers=auth,expected=404)
        compare('revoke-malformed','/api/token/revoke','POST',{'token':'only.two'},auth,400)
        compare('revoke','/api/token/revoke','POST',{'jti':'owned-owned-human'},auth,200)
        compare('revoked-auth','/api/list',headers=auth,expected=401)
        # Explicit unsupported operations are native-only boundaries, not parity.
        admin={'Authorization':'Bearer '+token('administrator')}
        for name,path,method,body in [('sync-unported','/api/sync/changes','GET',None),('policy-unported','/api/list?client_id=other','GET',None),('pii-unported','/api/set','POST',{'content':'alice@example.invalid'}),('working-unported','/api/set','POST',{'content':'Blue fox','working':True})]:
            time.sleep(.61);before=snapshot(peers[1].database);row=peers[1].request(name,path,method,body,admin);after=snapshot(peers[1].database)
            limits.append(dict(id=name,actual_native=row,state_unchanged=before==after));assert row['status']==501 and before==after,(name,row)
        with sqlite3.connect(peers[1].database) as conn:
            conn.execute("UPDATE memories SET content='alice@example.invalid' WHERE id='owned-seed'")
        for name,path,method,body in [('unsafe-list','/api/list','GET',None),('unsafe-get','/api/get?id=owned-seed','GET',None),('unsafe-search','/api/search','POST',{'query':'Blue fox sample'})]:
            time.sleep(.61);before=snapshot(peers[1].database);row=peers[1].request(name,path,method,body,admin);after=snapshot(peers[1].database)
            limits.append(dict(id=name,actual_native=row,state_unchanged=before==after));assert row['status']==501 and before==after,(name,row)
    finally:
        for peer in peers:shutdown.append(peer.close())
        embedding.close();receipt['embedding_requests']=embedding.requests
        args.report.write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps(dict(report=str(args.report),paired=len(records),explicit_limits=len(limits),shutdown=shutdown)))

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--native',type=Path,required=True);parser.add_argument('--report',type=Path,required=True);parser.add_argument('--control',choices=['wrong-secret','empty-state']);run(parser.parse_args())
