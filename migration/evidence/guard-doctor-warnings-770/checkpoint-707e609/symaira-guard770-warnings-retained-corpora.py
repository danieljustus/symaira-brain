from pathlib import Path
import json,importlib.util,subprocess,sys,tempfile
root=Path('/workspace/symaira-guard770-doctor-warnings')
sys.path.insert(0,str(root/'scripts/guard-standalone-oracle'))
import replay
GO=Path('/tmp/symaira-guard770-warnings-final-process-binaries/go')
RUST=Path('/workspace/symaira-guard770-diagnostics/target/debug/symguard')
current={c['id']:c for c in replay.cases()}
renames={'doctor-unported-invalid-default':'doctor-invalid-default','doctor-unported-invalid-threshold':'doctor-invalid-threshold'}
original94=root/'migration/evidence/guard-diagnostics-770/original-wip-4a9/source-snapshot/scripts/guard-standalone-oracle/replay.py'
with tempfile.TemporaryDirectory(prefix='guard770-original-corpora-') as raw:
 tmp=Path(raw)
 original80=tmp/'original80.py'
 original80.write_bytes(subprocess.check_output(['git','show','2f50ae75f10fba200bc1cfbb6892da0b64651740:scripts/guard-standalone-oracle/replay.py'],cwd=root))
 for count,path in [(80,original80),(94,original94)]:
  spec=importlib.util.spec_from_file_location('original'+str(count),path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);module.ROOT=root
  cases=module.cases();assert len(cases)==count
  rows=[]
  for i,old in enumerate(cases):
   new=current[renames.get(old['id'],old['id'])]
   assert (old['args'],old['state'],old['payload'])==(new['args'],new['state'],new['payload']), old['id']
   pair={side:replay.observe(binary,new,tmp/str(count)/str(i)/side,native) for side,binary,native in [('go',GO,False),('rust',RUST,True)]}
   disposition=replay.compare(new,pair['go'],pair['rust'])
   rows.append(dict(original_id=old['id'],current_id=new['id'],args=old['args'],state=old['state'],payload_hex=old['payload'].hex(),original_contract=old['contract'],current_contract=new['contract'],disposition=disposition,**pair))
  output=dict(candidate_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=root)),original_case_source_sha256=replay.digest(path.read_bytes()),original_source_ref='2f50ae75f10fba200bc1cfbb6892da0b64651740' if count==80 else 'preserved exact dirty4a9 source-snapshot',binaries_sha256=dict(go=replay.digest(GO.read_bytes()),rust=replay.digest(RUST.read_bytes())),total=len(rows),matched=sum(r['disposition']=='matched' for r in rows),results=rows,note='All original inputs byte-identical. Newly admitted errors must now satisfy full equality; original conservative contracts remain explicitly listed.')
  Path(f'/tmp/symaira-guard770-warnings-original{count}.json').write_text(json.dumps(output,indent=2)+'\n')
  print(count,output['matched'],[(r['original_id'],r['disposition']) for r in rows if r['disposition']!='matched'])
