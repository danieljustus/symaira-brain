#!/usr/bin/python3
import os,sys
from pathlib import Path
root=Path(os.environ['USAGE_V2_TEST_HOME'])
root.mkdir(parents=True,exist_ok=True)
for name in list(os.environ):
    if name not in {'OPENROUTER_V1_EVIDENCE', 'OPENROUTER_GO_V1', 'OPENROUTER_NATIVE_V1'} and name.startswith(('OPENCODE_','ANTHROPIC_','CLAUDE_','CODEX_','COPILOT_','CURSOR_','KIMI_','MOONSHOT_','NOUS_','HERMES_','OPENROUTER_','ZAI_','DEVIN_','GROK_','OLLAMA_','XDG_','SYMBRAIN_USAGE_')):
        os.environ.pop(name,None)
os.environ.update(HOME=str(root),USERPROFILE=str(root),XDG_DATA_HOME=str(root/'data'),XDG_CONFIG_HOME=str(root/'config'),XDG_CACHE_HOME=str(root/'cache'))
os.execvpe(sys.argv[1],sys.argv[1:],os.environ)
