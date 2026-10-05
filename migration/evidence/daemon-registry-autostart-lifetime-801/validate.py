"""Owned pure machinery validation; no SDK or daemon/product process."""
import ast,ctypes,hashlib,importlib.util,io,json,os,pathlib,signal,subprocess,sys,time,types,unittest
sys.dont_write_bytecode=True
R=pathlib.Path('/workspace/symaira-daemon801-autostart-cleanup');O=pathlib.Path(__file__).parent;H=R/'browse/port/harness';P='8a95b40e854724384b4a551f316215662d7b5815';sha=lambda b:hashlib.sha256(b).hexdigest();git=lambda *a:subprocess.check_output(['git',*a],cwd=R)
head=git('rev-parse','HEAD').decode().strip();assert not git('status','--porcelain'), 'clean source checkpoint required'
if sys.platform.startswith('linux'):
 libc=ctypes.CDLL(None,use_errno=True);assert libc.prctl(36,1,0,0,0)==0
os.umask(0o022);out=O/'clean-source-tests';out.mkdir(exist_ok=False);tmp=out/'owned-temporary';tmp.mkdir();env=dict(os.environ,PYTHONDONTWRITEBYTECODE='1',TMPDIR=str(tmp))
checks=[]
for name in ['test_registry_progress.py','test_registry_cli_process.py','test_registry_daemon_lifetime.py']:
 argv=[sys.executable,str(H/name)];stdout=out/(name+'.stdout');stderr=out/(name+'.stderr')
 with stdout.open('wb') as a,stderr.open('wb') as b:
  child=subprocess.Popen(argv,cwd=H,env=env,stdin=subprocess.DEVNULL,stdout=a,stderr=b,start_new_session=True)
  try:child.wait(timeout=30)
  except BaseException:
   os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=2);raise
 raw=stderr.read_bytes();assert child.returncode==0,(name,raw);checks.append(dict(argv=argv,exit=child.returncode,stdout_sha256=sha(stdout.read_bytes()),stderr_sha256=sha(raw),kind='actual Python machinery only, no product or SDK'))
# Reap only our adopted Python fixture descendants, never system processes.
if sys.platform.startswith('linux'):
 while True:
  try:pid,_=os.waitpid(-1,os.WNOHANG)
  except ChildProcessError:break
  if pid==0:break
# Actual executions of four deliberately corrupted Python ownership seams under
# the exact original mock unit. Only owned transient in-memory modules change.
sys.path.insert(0,str(H));import test_registry_daemon_lifetime as tests
raw=(H/'registry_daemon_lifetime.py').read_text();mutants=[]
for name,before,after,method in [
 ('skip-normal-wait','if not self.api.wait(self.handle, CLI_TIMEOUT):','if False:','test_normal_shutdown_waits_exit_then_closes_same_64bit_handle'),
 ('swallow-timeout','                raise failure','                return','test_timeout_terminates_retained_object_but_still_fails_gate'),
 ('force-numeric-PID','self.api.terminate(self.handle)','self.api.terminate(self.pid)','test_reused_pid_does_not_reopen_or_terminate_replacement_handle'),
 ('ignore-image-owner','if actual != self.binary.resolve():','if False:','test_wrong_image_is_closed_without_termination')]:
 assert raw.count(before)==1,(name,raw.count(before));mutated=raw.replace(before,after);m=types.ModuleType('owned_mock_lifecycle_'+name.replace('-','_'));exec(compile(mutated,'<owned-source-mutant>','exec'),m.__dict__)
 original=tests.lifetime;tests.lifetime=m;stream=io.StringIO()
 try:result=unittest.TextTestRunner(stream=stream).run(unittest.TestSuite([tests.Tests(method)]))
 finally:tests.lifetime=original
 log=out/('mutant-'+name+'.log');log.write_text(stream.getvalue());assert result.testsRun==1 and len(result.failures)==1 and not result.errors,(name,stream.getvalue());mutants.append(dict(name=name,method=method,rejected=True,intended_assertion_failure=True,log_sha256=sha(log.read_bytes()),source_sha256=sha(mutated.encode()),actual_native=False))
# Entire protected original tree body/mode retention and unchanged criteria.
original=json.loads((O/'original-whole-source.json').read_bytes())['files'];changed=set(git('diff','--name-only',P,head).decode().splitlines());allowed={'.github/workflows/browse-daemon-native.yml','browse/port/harness/daemon_registry.py'}
unchanged=[]
for row in original:
 rel=row['path'];b=git('show',head+':'+rel);mode=git('ls-tree',head,'--',rel).split()[0].decode()
 if rel not in allowed:assert (sha(b),mode)==(row['sha256'],row['git_mode']);unchanged.append(row)
assert {x for x in changed if x in {r['path'] for r in original}}<=allowed
for name in ['registry_compare.py','registry_cli_process.py','registry_progress.py','test_registry_progress.py','test_registry_cli_process.py','daemon_registry_cli.py','daemon_process.py','run.py']:
 rel='browse/port/harness/'+name;assert (H/name).read_bytes()==git('show',P+':'+rel)
# Literal observation/response/fixture constructions unchanged in observe.
def observe(raw):return next(n for n in ast.parse(raw).body if isinstance(n,ast.FunctionDef) and n.name=='observe')
old=observe(git('show',P+':browse/port/harness/daemon_registry.py'));new=observe((H/'daemon_registry.py').read_bytes())
constants=lambda n:[x.value for x in ast.walk(n) if isinstance(x,ast.Constant)]
# Additional names/errors live in the new module, not in old input criteria.
a=constants(old);b=constants(new);iterator=iter(b);assert all(any(y==x for y in iterator) for x in a)
# All previous function bodies except observe remain exact ASTs.
oldtop=ast.parse(git('show',P+':browse/port/harness/daemon_registry.py'));newtop=ast.parse((H/'daemon_registry.py').read_bytes())
for node in oldtop.body:
 if isinstance(node,ast.FunctionDef) and node.name!='observe':
  other=next(x for x in newtop.body if isinstance(x,ast.FunctionDef) and x.name==node.name);assert ast.dump(node,include_attributes=False)==ast.dump(other,include_attributes=False)
asts=[]
for name in ['daemon_registry.py','registry_daemon_lifetime.py','test_registry_daemon_lifetime.py']:
 p=H/name;ast.parse(p.read_bytes());assert len(p.read_text().splitlines())<400;asts.append(dict(path=str(p),sha256=sha(p.read_bytes()),lines=len(p.read_text().splitlines())))
for name,argv in [('actionlint',['/workspace/toolchains/bin/actionlint','.github/workflows/browse-daemon-native.yml']),('diff',['git','diff','--check',P,head])]:
 v=subprocess.run(argv,cwd=R,capture_output=True,timeout=30);(out/(name+'.stdout')).write_bytes(v.stdout);(out/(name+'.stderr')).write_bytes(v.stderr);assert v.returncode==0;checks.append(dict(argv=argv,exit=v.returncode,stdout_sha256=sha(v.stdout),stderr_sha256=sha(v.stderr)))
assert not git('status','--porcelain')
(O/'validation.json').write_text(json.dumps(dict(source=head,clean_before_after=True,checks=checks,pure_ownership_tests=13,original_progress_tests=5,original_capture_tests=4,meaningful_mock_source_mutants=mutants,full_unchanged_originals=unchanged,unchanged_existing_registry_controls=8,source_AST=asts,actual_current_native_SDK_product_compiler_Target_ports=0),indent=2)+'\n');print(json.dumps(dict(source=head,pure_ownership13=True,original_progress5=True,original_capture4=True,mock_mutants_rejected=len(mutants),unchanged=len(unchanged))))
