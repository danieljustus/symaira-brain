#!/usr/bin/env python3
"""Owned jsdom runtime exercises actual native assets and retains original defects."""
import argparse
import json
from pathlib import Path
import sqlite3
import subprocess
from support import Embeddings,Peer,owned_env,seed,token,digest
parser=argparse.ArgumentParser();parser.add_argument('--native',type=Path,required=True);parser.add_argument('--node',type=Path,required=True);parser.add_argument('--jsdom',type=Path,required=True);parser.add_argument('--report',type=Path,required=True);args=parser.parse_args()
repo=Path(__file__).resolve().parents[2];root=args.report.with_suffix('.evidence');root.mkdir(exist_ok=False)
embedding=Embeddings();peers=[];runs=[]
try:
    seedroot=root/'seed';env=owned_env(seedroot,embedding.url);database,actual_seed=seed(seedroot,env)
    for name,legacy in [('native',None),('original',Path('/workspace/oracles/daemon772-go-source/internal/memory/web/static/app.js'))]:
        owned=root/name;env=owned_env(owned,embedding.url);target=owned/'current.db'
        with sqlite3.connect(database) as original,sqlite3.connect(target) as copy:original.backup(copy)
        peer=Peer(args.native,owned,env,True,target);peers.append(peer)
        report=owned/'dom.json';command=[str(args.node),str(repo/'scripts/memory-http-oracle/dom.cjs'),'http://127.0.0.1:'+str(peer.port),token(),str(report)]
        if legacy:command.append(str(legacy))
        result=subprocess.run(command,env=dict(PATH='',MEMORY_DOM_MODULE=str(args.jsdom)),capture_output=True,timeout=25)
        runs.append(dict(mode=name,exit=result.returncode,stdout_hex=result.stdout.hex(),stderr_hex=result.stderr.hex(),report=str(report),sha256=digest(report),original_script_sha256=digest(legacy) if legacy else None))
        assert result.returncode==0,runs[-1]
finally:
    shutdown=[peer.close() for peer in peers];embedding.close()
    args.report.write_text(json.dumps(dict(native=str(args.native),native_sha256=digest(args.native),runs=runs,shutdown=shutdown,embedding_requests=embedding.requests,limits='Actual owned jsdom DOM/JS/network runtime, not a graphical browser screenshot/render/layout/CSP claim; no Go backend was used for the native surface.'),indent=2)+'\n')
print(json.dumps(dict(runs=len(runs),report=str(args.report))))
