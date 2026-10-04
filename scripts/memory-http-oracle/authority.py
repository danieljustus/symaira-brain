#!/usr/bin/env python3
"""Actual narrow authority regression gate; original full-state inputs retained."""
import argparse
from contextlib import closing
import http.client
import json
from pathlib import Path
import socket
import sqlite3
import uuid
from support import GO, Peer, Embeddings, owned_env, seed, token, digest, snapshot
from replay import semantic


def cases():
    return [
        ('duplicate-host-local-first-read','GET','/api/list',['{OWN}','untrusted.invalid'],None,False,True,400),
        ('duplicate-host-foreign-first-read','GET','/api/list',['untrusted.invalid','{OWN}'],None,False,True,400),
        ('duplicate-host-local-twice-read','GET','/api/list',['{OWN}','{OWN}'],None,False,True,400),
        ('duplicate-host-local-first-write','POST','/api/set',['{OWN}','untrusted.invalid'],{'content':'Amber fox'},True,True,400),
        ('duplicate-host-foreign-first-write','POST','/api/set',['untrusted.invalid','{OWN}'],{'content':'Amber fox'},True,True,400),
        ('missing-host-read','GET','/api/list',[],None,False,True,400),
        ('missing-host-write','POST','/api/set',[],{'content':'Amber fox'},True,True,400),
        ('absolute-foreign-target-read','GET','http://untrusted.invalid/api/list',['{OWN}'],None,False,True,403),
        ('absolute-foreign-target-write','POST','http://untrusted.invalid/api/set',['{OWN}'],{'content':'Silver fox'},True,True,403),
        ('absolute-local-target-foreign-host-read','GET','http://{OWN}/api/list',['untrusted.invalid'],None,False,True,200),
        ('duplicate-host-no-auth-write','POST','/api/set',['{OWN}','untrusted.invalid'],{'content':'Amber fox'},False,False,400),
        ('single-local-host-write-positive','POST','/api/set',['{OWN}'],{'content':'Golden fox'},False,True,200),
        ('absolute-local-target-foreign-host-write','POST','http://{OWN}/api/set',['untrusted.invalid'],{'content':'Azure fox'},False,True,200),
        ('absolute-local-target-missing-host-read','GET','http://{OWN}/api/list',[],None,False,True,400),
        ('absolute-local-target-missing-host-write','POST','http://{OWN}/api/set',[],{'content':'Azure fox'},False,True,400),
    ]


def actual(peer, name, method, target, hosts, body, origin, auth):
    authority='127.0.0.1:'+str(peer.port)
    data=json.dumps(body,separators=(',',':')).encode() if body is not None else b''
    target=target.replace('{OWN}',authority)
    lines=[method+' '+target+' HTTP/1.1',*['Host: '+h.replace('{OWN}',authority) for h in hosts]]
    if auth:lines.append('Authorization: Bearer '+token())
    if origin:lines.append('Origin: http://'+authority)
    if not auth:lines.append('X-Requested-With: XMLHttpRequest')
    if body is not None:lines+=['Content-Type: application/json','Content-Length: '+str(len(data))]
    lines.append('Connection: close');wire=('\r\n'.join(lines)+'\r\n\r\n').encode()+data
    before=snapshot(peer.database)
    with socket.create_connection(('127.0.0.1',peer.port),timeout=8) as connection:
        connection.settimeout(8);connection.sendall(wire)
        reply=http.client.HTTPResponse(connection);reply.begin();raw=reply.read()
        row=dict(id=name,status=reply.status,reason=reply.reason,headers=reply.getheaders(),body_hex=raw.hex(),request_hex=wire.hex())
    after=snapshot(peer.database)
    result=dict(raw=row,before=before,after=after,unchanged=before==after)
    if method=='POST' and row['status']==200:
        identifier=json.loads(raw)['id'];assert str(uuid.UUID(identifier))==identifier and uuid.UUID(identifier).version==4
        with closing(sqlite3.connect(peer.database)) as conn, conn:
            columns=[r[1] for r in conn.execute('pragma table_info(memories)')]
            stored=dict(zip(columns,conn.execute('SELECT * FROM memories WHERE id=?',(identifier,)).fetchone()))
            assert stored['valid_from']<=stored['created_at']<=stored['updated_at']
            assert stored['created_by']=='owned-human' and stored['kind']=='' and stored['content']==body['content']
            assert len(after['memories']['rows'])==len(before['memories']['rows'])+1
            audit=conn.execute('SELECT action,memory_id,scope,actor FROM audit_log WHERE memory_id=?',(identifier,)).fetchall()
            assert audit==[('set',identifier,'global','owned-human')]
            conn.execute("INSERT INTO memories_fts(memories_fts) VALUES('integrity-check')")
        metadata=json.loads(stored['metadata']);assert metadata['source_tool']=='http'
        for key in ['id','created_at','updated_at','valid_from']:stored.pop(key)
        metadata.pop('observed_at');stored['metadata']=metadata
        result.update(bound_id=identifier,compared_primary_row=stored,audit_identity=audit,fts_integrity='pass')
    return result


def run(args):
    root=args.report.with_suffix('.evidence');root.mkdir(exist_ok=False)
    embedding=Embeddings();peers=[];records=[];shutdown=[]
    try:
        seedroot=root/'seed';env=owned_env(seedroot,embedding.url);database,seed_record=seed(seedroot,env)
        for native,name,binary in [(False,'go',GO),(True,'native',args.native)]:
            owned=root/name;env=owned_env(owned,embedding.url);target=owned/'current.db'
            with closing(sqlite3.connect(database)) as original, closing(sqlite3.connect(target)) as current:
                with current:original.backup(current)
            peers.append(Peer(binary,owned,env,native,target))
        for name,method,target,hosts,body,origin,auth,status in cases():
            pair=[]
            for index,peer in enumerate(peers):
                request_hosts=hosts;request_target=target
                if index==1 and args.control=='omit-duplicate' and name=='duplicate-host-local-first-read':request_hosts=hosts[:1]
                if index==1 and args.control=='foreign-to-local' and name=='absolute-foreign-target-read':request_target='http://{OWN}/api/list'
                pair.append(actual(peer,name,method,request_target,request_hosts,body,origin,auth))
            compared=[semantic(r['raw']) for r in pair]
            if method=='POST' and status==200:
                for row,actual_row in zip(compared,pair):
                    assert row['body']['id']==actual_row['bound_id']
                    row['body']['id']='<bound-current-owner-id>'
            records.append(dict(name=name,pair=pair,match=compared[0]==compared[1]))
            assert all(r['raw']['status']==status for r in pair) and records[-1]['match'],name
            assert pair[0]['raw']['reason']==pair[1]['raw']['reason'],name
            if method=='GET' or status!=200:assert all(r['unchanged'] for r in pair),name
            else:assert pair[0]['compared_primary_row']==pair[1]['compared_primary_row'],name
    finally:
        for peer in peers:shutdown.append(peer.close())
        embedding.close()
        args.report.write_text(json.dumps(dict(native_sha256=digest(args.native),go_sha256=digest(GO),control=args.control,records=records,shutdown=shutdown,embedding_requests=embedding.requests,limits='Literal requests/body/reason plus selected application headers and complete SQLite refusal state; whole-wire Date/chunking/header ordering is not asserted.'),indent=2)+'\n')
    print(json.dumps(dict(cases=len(records),report=str(args.report))))


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--native',type=Path,required=True);parser.add_argument('--report',type=Path,required=True);parser.add_argument('--control',choices=['omit-duplicate','foreign-to-local']);run(parser.parse_args())
