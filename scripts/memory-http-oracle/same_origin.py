#!/usr/bin/env python3
"""Explicit desired same-origin divergence; never normalized into Go parity."""
import argparse
import json
from pathlib import Path
import sqlite3
import subprocess
import time
from support import GO,Peer,Embeddings,owned_env,seed,token,digest,snapshot


def run(args):
    repo=Path(__file__).resolve().parents[2];root=args.report.with_suffix('.evidence');root.mkdir(exist_ok=False)
    embeddings=Embeddings();peers=[];records=[];shutdown=[]
    try:
        seedroot=root/'seed';env=owned_env(seedroot,embeddings.url);database,seed_record=seed(seedroot,env)
        for native,name,binary in [(False,'go',GO),(True,'native',args.native)]:
            owned=root/name;env=owned_env(owned,embeddings.url);current=owned/'current.db'
            with sqlite3.connect(database) as original,sqlite3.connect(current) as copy:original.backup(copy)
            peers.append(Peer(binary,owned,env,native,current))
        admin={'Authorization':'Bearer '+token()}
        def actual(name,path,method,body=None,headers=None,native_status=403,go_status=403,origin=None,host=None,readonly=True):
            time.sleep(.61);pair=[]
            for peer in peers:
                before=snapshot(peer.database);authority='127.0.0.1:'+str(peer.port)
                h=dict(admin if headers is None else headers)
                h['Origin']=origin(authority,peer) if origin else 'http://'+authority
                if host:h['Host']=host(authority,peer)
                row=peer.request(name,path,method,body,h);after=snapshot(peer.database)
                pair.append(dict(actual=row,state_before=before,state_after=after,state_unchanged=before==after))
            records.append(dict(id=name,go=pair[0],native=pair[1],desired_divergence=go_status!=native_status))
            assert pair[0]['actual']['status']==go_status and pair[1]['actual']['status']==native_status,records[-1]
            assert pair[0]['state_unchanged'],records[-1]
            if readonly:assert pair[1]['state_unchanged'],records[-1]
            return pair
        actual('exact-preflight','/api/set','OPTIONS',headers={},native_status=200)
        actual('exact-origin-auth-missing','/api/list','GET',headers={},native_status=401)
        actual('exact-origin-invalid-jwt','/api/set','POST',{'content':'Azure fox specimen'},headers={'Authorization':'Bearer invalid'},native_status=401)
        actual('exact-origin-read-only','/api/set','POST',{'content':'Azure fox specimen'},headers={'Authorization':'Bearer '+token('reader')},native_status=403)
        pair=actual('exact-origin-search','/api/search','POST',{'query':'Blue fox sample'},native_status=200,readonly=False)
        assert json.loads(bytes.fromhex(pair[1]['actual']['body_hex']))[0]['memory']['id']=='owned-seed'
        pair=actual('exact-origin-set','/api/set','POST',{'content':'Azure fox specimen'},native_status=200,readonly=False)
        identifier=json.loads(bytes.fromhex(pair[1]['actual']['body_hex']))['id']
        actual('exact-origin-delete','/api/delete?id='+identifier,'DELETE',native_status=200,readonly=False)
        hostile=[('foreign',lambda a,p:'http://untrusted.invalid'),('prefix',lambda a,p:'http://'+a+'.evil'),('other-port',lambda a,p:'http://127.0.0.1:'+str(p.port+1)),('https',lambda a,p:'https://'+a),('null',lambda a,p:'null'),('opaque',lambda a,p:'file://'),('slash',lambda a,p:'http://'+a+'/'),('userinfo',lambda a,p:'http://owned@'+a),('similar-host',lambda a,p:'http://localhost.evil:'+str(p.port))]
        for name,origin in hostile:
            actual('hostile-'+name,'/api/set','POST',{'content':'Azure fox specimen'},origin=origin)
        actual('forged-matching-wrong-listener-port','/api/set','POST',{'content':'Azure fox specimen'},origin=lambda a,p:'http://127.0.0.1:'+str(p.port+1),host=lambda a,p:'127.0.0.1:'+str(p.port+1))
        actual('hostile-preflight','/api/set','OPTIONS',headers={},origin=lambda a,p:'https://untrusted.invalid')
        for row in records:
            native=row['native']['actual']
            if native['status']==200:
                expected=native['request_headers']['Origin']
                assert dict((k.lower(),v) for k,v in native['headers'])['access-control-allow-origin']==expected
    finally:
        for peer in peers:shutdown.append(peer.close())
        embeddings.close()
        args.report.write_text(json.dumps(dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=repo)),go_sha256=digest(GO),native_sha256=digest(args.native),seed=seed_record,records=records,shutdown=shutdown,limits='Actual supplied same-Origin/hostile headers and live Go/native peers; positive behavior is an approved explicit native divergence, not Go parity or graphical-browser network proof.'),indent=2)+'\n')
    print(json.dumps(dict(cases=len(records),report=str(args.report))))

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--native',type=Path,required=True);parser.add_argument('--report',type=Path,required=True);run(parser.parse_args())
