import pathlib,json,hashlib
p=pathlib.Path('/workspace/symaira-memory803-deadline/rust/symbrain-cli/tests/health_stdio_windows_diagnostic.rs');s=p.read_text()
assert s.count('.checked_duration_since(Instant::now())')==2
assert s.count('.filter(|remaining| !remaining.is_zero())')==2
assert s.index('"initialize_begin"')<s.index('.checked_duration_since(Instant::now())')<s.index('client.initialize(remaining)')
assert s.index('"ping_begin"')<s.rindex('.checked_duration_since(Instant::now())')<s.index('client.ping(remaining)')
assert 'Duration::from_secs(5)'in s and 'Duration::from_secs(10)'in s
# Integer nanosecond source schedule projections; these are not Rust/native runs.
def projected(begin,initialize,ping_begin,ping):
 deadline=5_000_000_000;at=begin;calls=[]
 if at<deadline:
  calls.append(('init',deadline-at));at+=initialize
  if initialize<=calls[-1][1]:
   at+=ping_begin
   if at<deadline:
    calls.append(('ping',deadline-at));at+=ping
 return {'calls':calls,'elapsed_ns':at,'success':len(calls)==2 and initialize<=calls[0][1] and ping<=calls[1][1]}
original=projected(100_000_000,4_700_000_000,250_000_000,50_000_000);assert original['calls']==[('init',4_900_000_000)]and not original['success']
exhausted=projected(5_000_000_000,0,0,0);assert not exhausted['calls']
healthy=projected(100_000_000,1_000_000_000,250_000_000,50_000_000);assert healthy['success']and healthy['elapsed_ns']<5_000_000_000
result={'kind':'source-order checks and deterministic integer schedule projection; not native execution','source_sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'original_review_schedule':original,'initialize_begin_exhausts_deadline':exhausted,'healthy_control':healthy,'compiler_or_provider_executions':0,'original_failure_cause_proved':False}
out=pathlib.Path('/workspace/symaira-memory803-deadline/migration/evidence/memory-windows-parity-803/deadline-review-026/corrected-source-control.json');out.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
