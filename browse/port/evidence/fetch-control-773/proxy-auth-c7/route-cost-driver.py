import hashlib,json,os,socket,subprocess,sys,tempfile,time
from pathlib import Path
sys.path.insert(0,'/workspace/symaira-fetch773-proxy-auth/browse/port/harness')
import fetch_control_proxy_fixture as fixture
import fetch_control_tls_fixture as tls
import fetch_control_process as common
ROOT=Path('/workspace/symaira-fetch773-proxy-auth')
PREFIX='/tmp/symaira-fetch773-proxy-auth-clean-'
BINARIES={'go':Path('/tmp/symaira-fetch773-proxy-auth-clean-go-probe'),'rust':ROOT/'target/debug/examples/control_probe'}
results=[]
def execute(kind,cases,root,trust):
    root.mkdir()
    env=dict(HOME=str(root),USERPROFILE=str(root),XDG_CONFIG_HOME=str(root/'config'),XDG_CACHE_HOME=str(root/'cache'),XDG_DATA_HOME=str(root/'data'),PATH='',SYMBROWSE_GO_BINARY=str(root/'absent-go'))
    env.update(trust or {})
    start=time.perf_counter_ns(); observed=[]; timestamps=[]
    with subprocess.Popen([str(BINARIES[kind])],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,cwd=root,env=env) as process:
        process.stdin.write(json.dumps(cases).encode());process.stdin.close()
        for line in process.stdout:
            timestamps.append((time.perf_counter_ns()-start)/1e6);observed.append(json.loads(line))
        error=process.stderr.read();code=process.wait(timeout=15)
    assert code==0 and not error,(code,error)
    assert [r['id'] for r in observed]==[c['id'] for c in cases]
    assert all(r.get('error')=='' for r in observed),observed
    return dict(records=observed,response_arrival_ms=timestamps,response_interarrival_ms=[timestamps[0]]+[b-a for a,b in zip(timestamps,timestamps[1:])],process_total_ms=(time.perf_counter_ns()-start)/1e6,exit=code)
with tempfile.TemporaryDirectory(prefix='fetch773-owned-route-cost-') as raw:
    root=Path(raw);contexts,ca=tls.contexts(root/'tls')
    for scheme in ['http','https']:
        server,thread=fixture.start(contexts['valid'] if scheme=='https' else None)
        connections=[];original=server.get_request
        def counted():
            peer,address=original();peer.setsockopt(socket.IPPROTO_TCP,socket.TCP_NODELAY,1);connections.append(address);return peer,address
        server.get_request=counted
        try:
            proxy=f'{scheme}://127.0.0.1:{server.server_port}'
            for port in ['81','080']:
                cases=[dict(id=f'{scheme}-{port}-{i}',url=f'http://93.184.216.34:{port}/body',proxy=proxy) for i in range(30)]
                pair={}
                for kind in ['go','rust']:
                    before=len(connections)
                    pair[kind]=execute(kind,cases,root/f'{scheme}-{port}-{kind}',tls.linux_trust_env(ca) if scheme=='https' else None)
                    pair[kind]['accepted_connections']=len(connections)-before
                assert all(common.comparable(a)==common.comparable(b) for a,b in zip(pair['go']['records'],pair['rust']['records'])),pair
                results.append(dict(proxy_scheme=scheme,target_port=port,cases=cases,matched=30,**pair))
        finally:fixture.stop(server,thread)
receipt=dict(candidate_source=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),binary_sha256={k:hashlib.sha256(p.read_bytes()).hexdigest() for k,p in BINARIES.items()},driver_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),scope='Owned local fixture, thirty consecutive responses in one process per route/client; exact outputs checked. First response includes startup. Interarrival durations are process-visible, not isolated Fetch timings. Explicit-port raw path owns unpooled connections; ordinary paths may pool. TCP_NODELAY fixture peer identically applied; HTTPS per-process owned CA trust Linux only. No global performance threshold/claim.',groups=results)
Path(PREFIX+'route-cost.json').write_text(json.dumps(receipt,indent=2)+'\n')
for group in results:print(group['proxy_scheme'],group['target_port'],{k:{n:group[k][n] for n in ['accepted_connections','process_total_ms']} for k in ['go','rust']})
