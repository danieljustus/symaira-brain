"""Owned actual process and raw HTTP evidence; no operator state is inherited."""
import base64
import hashlib
import hmac
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import signal
import socket
import sqlite3
import subprocess
import threading
import time

GO = Path('/workspace/oracles/symbrain-go-dcddcef0')
GO_SHA = 'a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41'
SECRET = 'synthetic-memory763-owned-http-secret'
TIME = '2000-01-01 00:00:00 +0000 UTC'

def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def token(subject='owned-human', **overrides):
    encode = lambda value: base64.urlsafe_b64encode(value).rstrip(b'=')
    claims = dict(jti='owned-'+subject, iss='symaira-memory', sub=subject, iat=1, exp=4102444800)
    claims.update(overrides)
    body = encode(b'{"alg":"HS256","typ":"JWT"}')+b'.'+encode(json.dumps(claims,separators=(',',':')).encode())
    return (body+b'.'+encode(hmac.new(SECRET.encode(),body,hashlib.sha256).digest())).decode()

def owned_env(root, embedding):
    root.mkdir()
    for name in ['home','config','data','cache']:
        (root/name).mkdir()
    config=root/'config/symmemory'; config.mkdir()
    (config/'config.toml').write_text('[ollama]\nurl = "'+embedding+'"\nmodel = "nomic-embed-text"\n[conflict]\nenabled = false\n')
    return dict(HOME=str(root/'home'),USERPROFILE=str(root/'home'),XDG_CONFIG_HOME=str(root/'config'),XDG_DATA_HOME=str(root/'data'),XDG_CACHE_HOME=str(root/'cache'),PATH='',JWT_SECRET_KEY=SECRET,SYMMEMORY_OLLAMA_URL=embedding,SYMBRAIN_GO_BINARY=str(root/'absent-go'))

class Embeddings:
    def __init__(self):
        self.requests=[]
        owner=self
        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                raw=self.rfile.read(int(self.headers.get('Content-Length','0')))
                owner.requests.append(dict(path=self.path,body_hex=raw.hex()))
                raw=json.dumps({'object':'list','data':[{'object':'embedding','index':0,'embedding':[0.25]*768}]}).encode()
                self.send_response_only(200);self.send_header('Content-Length',str(len(raw)));self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(raw)
            def log_message(self,*args):pass
        self.server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        self.thread=threading.Thread(target=self.server.serve_forever,daemon=True);self.thread.start()
        self.url='http://127.0.0.1:'+str(self.server.server_port)+'/api/embeddings'
    def close(self):
        self.server.shutdown();self.server.server_close();self.thread.join(3)

class Peer:
    def __init__(self, binary, root, env, native, database, extra_env=None):
        self.root=root;self.env=dict(env,**(extra_env or {})); self.database=database
        with socket.socket() as sock:
            sock.bind(('127.0.0.1',0));self.port=sock.getsockname()[1]
        self.out=open(root/'stdout.log','wb');self.err=open(root/'stderr.log','wb')
        args=[str(database),str(self.port)] if native else ['memory','serve','--db',str(database),'--port',str(self.port)]
        self.process=subprocess.Popen([str(binary),*args],env=self.env,cwd=root,stdout=self.out,stderr=self.err)
        deadline=time.monotonic()+15
        while time.monotonic()<deadline:
            if self.process.poll() is not None:raise AssertionError(('server exited',self.process.returncode,(root/'stderr.log').read_text()))
            try:
                with socket.create_connection(('127.0.0.1',self.port),timeout=.1):break
            except OSError:time.sleep(.03)
        else:raise AssertionError('server never listened')
    def request(self,name,path,method='GET',body=None,headers=None):
        raw=json.dumps(body,separators=(',',':')).encode() if isinstance(body,(dict,list)) else body
        headers=dict(headers or {})
        if raw is not None:headers.setdefault('Content-Type','application/json')
        conn=http.client.HTTPConnection('127.0.0.1',self.port,timeout=10)
        conn.request(method,path,body=raw,headers=headers);response=conn.getresponse();data=response.read()
        row=dict(id=name,path=path,method=method,request_headers=headers,request_body_hex=(raw or b'').hex(),status=response.status,headers=response.getheaders(),body_hex=data.hex())
        conn.close();return row
    def close(self):
        start=time.monotonic()
        if self.process.poll() is None:self.process.send_signal(signal.SIGTERM)
        try:self.process.wait(timeout=8)
        except subprocess.TimeoutExpired:self.process.kill();self.process.wait();raise
        self.out.close();self.err.close()
        assert self.process.returncode==0,self.process.returncode
        with socket.socket() as sock:
            assert sock.connect_ex(('127.0.0.1',self.port))!=0,'listener survived shutdown'
        return dict(exit=self.process.returncode,elapsed_seconds=time.monotonic()-start,listener_closed=True)

def snapshot(path):
    with sqlite3.connect(path) as conn:
        return {name:[list(row) for row in conn.execute('SELECT * FROM "'+name+'" ORDER BY rowid')] for (name,) in conn.execute("SELECT name FROM sqlite_master WHERE type='table' AND name IN ('memories','rules','entities','profiles','audit_log','jwt_revocations')")}

def seed(root, env):
    database=root/'seed.db'
    command=[str(GO),'memory','set','--kind','reference','--db',str(database),'Blue fox sample']
    result=subprocess.run(command,cwd=root,env=env,capture_output=True,timeout=15)
    record=dict(argv=command,exit=result.returncode,stdout_hex=result.stdout.hex(),stderr_hex=result.stderr.hex())
    assert result.returncode==0,record
    with sqlite3.connect(database) as conn:
        conn.execute("UPDATE memories SET id='owned-seed',created_at=?,updated_at=?,valid_from=?,created_by='owned-human',updated_by='owned-human',metadata='{}',access_count=1,last_access=NULL,prev_access=NULL",(TIME,TIME,TIME))
        conn.execute('DELETE FROM audit_log')
        for name,role in [('owned-human','readwrite'),('reader','read'),('administrator','admin'),('unknown-role','bogus')]:
            conn.execute('INSERT INTO profiles(id,name,role,created_at,updated_at) VALUES(?,?,?,?,?)',(name,name,role,TIME,TIME))
        conn.execute("INSERT INTO rules(id,content,scope,metadata,created_at,updated_at,created_by,updated_by) VALUES('owned-rule','Blue fox rule','global','{}',?,?,'owned-human','owned-human')",(TIME,TIME))
        conn.execute("INSERT INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES('owned-entity','Blue fox','other','[\"Azure fox\"]','Synthetic description','',?,?)",(TIME,TIME))
        assert conn.execute('SELECT embedding_source,embedding_dim FROM memories').fetchone()==('ollama',768)
    return database,record
