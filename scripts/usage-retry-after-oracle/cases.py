"""Fixed grammar/boundary inputs plus deterministic binary64 rounding probes."""
import json
from pathlib import Path
import random
import sys

values=['','garbage','3','-3','-0','-0.0','+0','1.9','0x1p2','0X_1.FP+2','1_0','1_0.5_0e+1_0','NaN','nan','NAN','+NaN','-NaN','Inf','+Inf','Infinity','+infinity','-Inf','-Infinity','1e19','9223372036854775807','9223372036854774784','9223372036854775808','2147483647','2147483648','1e309','-1e309','1e-99999','0e99999','-0e99999','.5','5.','01','0x1','0x1p','0x1p_2','0x_1p2','0x1_p2','0x1p2_','1_e2','1e_2','_1','1_','1__0','0x1p1_0','0x1.00000000000008p0','0x1.00000000000008000000000001p0','0x1.00000000000018p0','0x1p-1074','0x1p-1075','0x1.000000000000000000001p-1075','0x1.fffffffffffffp1023','0x1.fffffffffffff8p1023','0x1p1024','0x0p99999','-0x0p-99999',' 1.9\t','1 0','1\u200b','\ufeff3','0x1.2.3p0','1e2e3','0b1','1f','1_0x','0x1p+0','0x1p-0']
whitespace=[*range(9,14),32,0x85,0xa0,0x1680,*range(0x2000,0x200b),0x2028,0x2029,0x202f,0x205f,0x3000]
public=[dict(id=f'grammar-{i}',value_hex=v.encode().hex())for i,v in enumerate(values)]
public += [dict(id=f'trim-{point:04x}',value_hex=(chr(point)+'1.9'+chr(point)).encode().hex())for point in whitespace]
public += [dict(id='raw-'+str(i),value_hex=b.hex())for i,b in enumerate([b'\xff3',b'3\xe2\x82',b'3\x00',b'3\r\n4'])]
randomizer=random.Random(768)
for i in range(1000):
 digits=''.join(randomizer.choice('0123456789abcdef')for _ in range(randomizer.randrange(1,65)))
 point=randomizer.randrange(len(digits)+1)
 value='0x'+digits[:point]+'.'+digits[point:]+'p'+str(randomizer.randrange(-1300,1301))
 public.append(dict(id=f'hex-round-{i}',value_hex=value.encode().hex()))
for i in range(500):
 digits=''.join(randomizer.choice('0123456789')for _ in range(randomizer.randrange(1,101)))
 point=randomizer.randrange(len(digits)+1)
 value=digits[:point]+'.'+digits[point:]+'e'+str(randomizer.randrange(-450,451))
 public.append(dict(id=f'decimal-round-{i}',value_hex=value.encode().hex()))
# Header controls forbid raw CR/LF/VT/FF; those stay in the public scalar
# corpus, while all21 legal HTTP outer-whitespace points are actual TLS cases.
wire_values=[v for v in values if '\r'not in v and '\n'not in v]
wire_values += [chr(point)+'1.9'+chr(point)for point in whitespace if point not in [10,11,12,13]]
variants=[('claude','api','ANTHROPIC_ADMIN_KEY','claude-admin-cost.json'),('claude','oauth','ANTHROPIC_OAUTH_TOKEN','claude-oauth-usage.json'),('codex','oauth','CODEX_ACCESS_TOKEN','codex-wham-usage.json'),('copilot','oauth','COPILOT_ACCESS_TOKEN','copilot-user.json'),('cursor','web','CURSOR_COOKIE','cursor-usage-summary.json'),('kimi','api','KIMI_CODE_API_KEY','kimi-api-usages.json'),('kimi','cli','','kimi-api-usages.json'),('kimi','web','KIMI_AUTH_TOKEN','kimi-web-usages.json'),('moonshot','api','MOONSHOT_API_KEY','moonshot-balance-ai.json'),('nous','oauth','NOUS_PORTAL_ACCESS_TOKEN','nous-account.json'),('opencode','web','OPENCODE_COOKIE','opencode-subscription-json.txt'),('openrouter','api','OPENROUTER_API_KEY','openrouter-credits.json')]
wire=[]
for provider,source,env,fixture in variants:
 for i,value in enumerate(wire_values):wire.append(dict(id=f'retry-{provider}-{source}-{i}',provider=provider,source=source,env=env,fixture=fixture,status=429,kind='retry',retry_after=value,responses=[429]*4))
 for i,headers in enumerate([['3','17'],['','17'],['0x1p2','17'],['\u00a01.9\u00a0','17']]):
  wire.append(dict(id=f'first-{provider}-{source}-{i}',provider=provider,source=source,env=env,fixture=fixture,status=429,kind='retry',retry_after_values_hex=[v.encode().hex()for v in headers],responses=[429]*4))
 wire.append(dict(id=f'first-{provider}-{source}-invalid-byte',provider=provider,source=source,env=env,fixture=fixture,status=429,kind='retry',retry_after_values_hex=['ff33','3137'],responses=[429]*4))
assert len(public)==len({r['id']for r in public})
assert len(wire)==len({r['id']for r in wire})
Path(sys.argv[1]).write_text(json.dumps(dict(public=public,wire=wire,grammar_values=values,whitespace=whitespace,public_cases=len(public),wire_cases=len(wire)),indent=2)+'\n')
print('RetryAfter corpus',len(public),'public scalars',len(wire),'full TLS constructor cases')
