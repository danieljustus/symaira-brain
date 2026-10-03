"""Source-bound Go flag and typed-config process cases; no operator data."""
import json
import os


def arguments():
    cases=[[],["-h"],["--help"],["help"],["unknown"],["--unknown"]]
    verbs=['list','rules','query-log','search','set','delete']
    for verb in verbs:
     for args in [['--undefined'],['----wrong'],['-=bad'],['---'],['--db'],['--db='],['positional','--undefined'],['--help'],['--db','--help'],['--undefined','--help'],['--','--help'],['-help'],['-h=foo'],['-help=foo']]:cases.append([verb]+args)
    for verb in ['list','rules','query-log']:
     for args in [['one'],['one','--undefined'],['--','--undefined'],['-'],['--scope','global','--scope','project'],['-s=global','--scope=project']]:cases.append([verb]+args)
    for verb in ['list','query-log','search']:
     for limit in ['0','-1','2','0x10','075','08','0b11','0o11','1_000','0x_10','_1','1_','9223372036854775808','18446744073709551616','18446744073709551616z','+3']:
      cases.append([verb,'--limit='+limit])
    for verb in ['search','set','delete']:
     for args in [[],[''],[' '],['one','two'],['--','-'],['-'],['value','--db'],['--db','path','--','-']]:cases.append([verb]+args)
    for kind in ['','invalid','USER',' Preference ','USER-pref','project-rule','u_s_e_r','kind: user-preferences']:
     cases.append(['set','value','--kind='+kind,'other'])
    for boolv in ['true','false','TRUE','True','FALSE','False','t','f','T','F','1','0','yes','','2']:
     cases.append(['set','value','--staged='+boolv])
    for verb in verbs:
     cases.append([verb,'--db','bad/path','--undefined'])
    if os.name=='posix':
     cases.append([b'\xff'])
     for verb in verbs:
      for value in [b'\xff',b'\xe2\x82',b'\xff\xff']:
       cases.append([verb.encode(),b'--limit='+value])
       cases.append([verb.encode(),b'--'+value])
    return cases


def configuration(fields):
    cases=[]
    for key,kind in fields:
     if key=='database.path':continue
     bad='1' if kind in ('String','Bool','PointerBool','Strings','Map') else '"wrong"'
     cases.append((f'invalid-file-{key}',f'database.path="configured.db"\n{key}={bad}\n','',{}))
     cases.append((f'zero-file-{key}',f'database.path="configured.db"\n{key}=0\n','',{}))
     if kind!='Map':cases.append((f'invalid-env-{key}','database.path="configured.db"\n','',{'SYMMEMORY_'+key.replace('.','_').upper():('wrong' if kind not in ('String','Strings') else 'valid')}))
    for val in ['false','true','FALSE','False','0','1','yes','']:
     cases.append(('boolean-file-'+val,'database.path="configured.db"\nconflict.enabled='+json.dumps(val)+'\n','',{}))
    for val in ['0x1.fffffffffffff7p1023','0x1.fffffffffffff8p1023','0x1p1023','0x0.1p1027','0x.00001p1043','0x'+('f'*1000)+'p-8000','1_000.5','0x1p10','0x_1p0','0x1.8p-1','NaN','+NaN','-NaN','Infinity','-inf','1e309','1e-9999','0x1p1024','0x1fffffffffffff8p971','0x1fffffffffffff7p971','0x'+('f'*100)+'p-2000','0x1p-9999999999999999999999999999999','0x0p9999999999999999999999999999999','nan','NAN','nanx',' 1.0','1._0','1_.0','1_0','1e1_0','0x1p1_0']:
     cases.append(('float-file-'+val,'database.path="configured.db"\nranking.spreading_weight='+json.dumps(val)+'\n','',{}))
    cases += [('global-project-env','database.path="configured.db"\n','database.path="project.db"\n',{'SYMMEMORY_DATABASE_PATH':'env.db'}),('project-zero','database.path="configured.db"\n','database.path=""\n',{}),('unknown-env','database.path="configured.db"\n','',{'SYMMEMORY_UNKNOWN':'bad'}),('invalid-project','database.path="configured.db"\n','server.http_port="bad"\n',{}),('duplicate-key','database.path="configured.db"\ndatabase.path="project.db"\n','',{}),('non-table-section','database.path="configured.db"\nsecurity=1\n','',{}),('unknown-wrong-type','database.path="configured.db"\nunknown=1979-05-27T07:32:00Z\n','',{}),('inline-table','database={path="configured.db"}\n','',{})]
    return cases
