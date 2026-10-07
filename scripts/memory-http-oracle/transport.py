#!/usr/bin/env python3
"""The eight original raw transport controls, using actual private owners."""
import argparse
from contextlib import closing
import json
from pathlib import Path
import socket
import sqlite3
import time
from support import GO, Peer, Embeddings, owned_env, seed, token, digest, snapshot


def raw(peer,name,headers,body=b'',half_close=False):
    before=snapshot(peer.database);start=time.monotonic()
    wire=headers.replace(b'{PORT}',str(peer.port).encode())+body
    with socket.create_connection(('127.0.0.1',peer.port),timeout=8) as conn:
        conn.settimeout(8);conn.sendall(wire)
        if half_close:conn.shutdown(socket.SHUT_WR)
        reply=b''
        while True:
            try:chunk=conn.recv(65536)
            except (socket.timeout,ConnectionResetError):break
            if not chunk:break
            reply+=chunk
    after=snapshot(peer.database)
    status=int(reply.split(b' ',2)[1]) if reply else None
    return dict(id=name,request_hex=wire.hex(),response_hex=reply.hex(),status=status,elapsed=time.monotonic()-start,before=before,after=after,unchanged=before==after)


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
        auth=('Authorization: Bearer '+token()+'\r\n').encode()
        cases=[
            ('no-auth-large-declared-body',b'POST /api/set HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nContent-Length: 1000000000\r\nX-Requested-With: XMLHttpRequest\r\nConnection: close\r\n\r\n',b'',False,401),
            ('host-denial-before-body',b'POST /api/set HTTP/1.1\r\nHost: untrusted.invalid\r\n'+auth+b'Content-Length: 1000000000\r\nConnection: close\r\n\r\n',b'',False,403),
            ('csrf-before-host',b'POST /api/set HTTP/1.1\r\nHost: untrusted.invalid\r\nContent-Length: 1000000000\r\nConnection: close\r\n\r\n',b'',False,403),
            ('invalid-body-half-close',b'POST /api/set HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\n'+auth+b'Content-Length: 99\r\nConnection: close\r\n\r\n',b'{',True,400),
            ('conflicting-lengths',b'POST /api/set HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\n'+auth+b'Content-Length: 1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n',b'{}',True,400),
            ('duplicate-host',b'GET /api/list HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nHost: untrusted.invalid\r\n'+auth+b'Connection: close\r\n\r\n',b'',False,400),
            ('missing-host',b'GET /api/list HTTP/1.1\r\n'+auth+b'Connection: close\r\n\r\n',b'',False,400),
            ('incomplete-header-timeout',b'GET /api/list HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\n',b'',False,None),
        ]
        for name,headers,body,half,expected in cases:
            pair=[raw(peer,name,headers,body,half) for peer in peers]
            records.append(dict(name=name,pair=pair))
            assert all(row['status']==expected and row['unchanged'] for row in pair),name
            if name=='incomplete-header-timeout':assert all(4.0<row['elapsed']<7.0 for row in pair)
    finally:
        for peer in peers:shutdown.append(peer.close())
        embedding.close()
        args.report.write_text(json.dumps(dict(native_sha256=digest(args.native),go_sha256=digest(GO),records=records,shutdown=shutdown),indent=2)+'\n')
    print(json.dumps(dict(cases=len(records),report=str(args.report))))


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--native',type=Path,required=True);parser.add_argument('--report',type=Path,required=True);run(parser.parse_args())
