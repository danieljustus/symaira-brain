"""Every remote Usage provider and strategy affected by HTTP status delivery."""
import json
from pathlib import Path
import sys
variants=[('claude','api','ANTHROPIC_ADMIN_KEY','claude-admin-cost.json'),('claude','oauth','ANTHROPIC_OAUTH_TOKEN','claude-oauth-usage.json'),('codex','oauth','CODEX_ACCESS_TOKEN','codex-wham-usage.json'),('copilot','oauth','COPILOT_ACCESS_TOKEN','copilot-user.json'),('cursor','web','CURSOR_COOKIE','cursor-usage-summary.json'),('kimi','api','KIMI_CODE_API_KEY','kimi-api-usages.json'),('kimi','cli',None,'kimi-api-usages.json'),('kimi','web','KIMI_AUTH_TOKEN','kimi-web-usages.json'),('moonshot','api','MOONSHOT_API_KEY','moonshot-balance-ai.json'),('nous','oauth','NOUS_PORTAL_ACCESS_TOKEN','nous-account.json'),('opencode','web','OPENCODE_COOKIE','opencode-subscription-json.txt'),('openrouter','api','OPENROUTER_API_KEY','openrouter-credits.json')]
rows=[]
for provider,source,env,fixture in variants:
    for kind,status in [('401',401),('403',403),('429',429),('500',500),('success',200),('malformed',200),('empty',200)]:
        rows.append(dict(id=f'status-{provider}-{source}-{kind}',provider=provider,source=source,env=env or '',fixture=fixture,status=status,kind=kind,responses=[status]*4))
assert len(rows)==84
Path(sys.argv[1]).write_text(json.dumps(rows,indent=2)+'\n', encoding='utf-8')
