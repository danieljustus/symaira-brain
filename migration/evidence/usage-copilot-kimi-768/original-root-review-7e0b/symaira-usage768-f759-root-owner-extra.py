from pathlib import Path
import subprocess, json, tempfile, hashlib, base64

records = []
go = Path('/tmp/symaira-setup765-source-independent-go')
parent = Path('/tmp/symaira-usage768-f759-root-parent-cli')
current = Path('/workspace/symaira-usage768-local-files/target/debug/symbrain')
for provider in ['copilot', 'kimi']:
    for variant in ['plain', 'symlink-home', 'symlink-explicit-home']:
        if provider == 'copilot' and variant == 'symlink-explicit-home':
            continue
        with tempfile.TemporaryDirectory(prefix='usage-f759-route-owner-') as d:
            root = Path(d)
            (root/'home').mkdir()
            (root/'owner/nested').mkdir(parents=True)
            (root/'owner/home').mkdir()
            (root/'link').symlink_to(root/'owner/nested', target_is_directory=True)
            relative = '.config/github-copilot/apps.json' if provider == 'copilot' else '.kimi-code/credentials/kimi-code.json'
            inputs = {}
            for folder, token in [('home', 'lexical-owner'), ('owner/home', 'physical-owner')]:
                value = {'github.com:a': {'oauth_token': token}} if provider == 'copilot' else {'access_token': token}
                path = root/folder/relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(json.dumps(value))
                inputs[str(path.relative_to(root))] = path.read_bytes().hex()
            home = str(root/'link/../home') if variant == 'symlink-home' else str(root/'home')
            env = {'HOME': home, 'USERPROFILE': home, 'PATH': '', 'ANTHROPIC_OAUTH_TOKEN': 'env://OWNED_ABSENT', 'SYMBRAIN_GO_BINARY': str(root/'absent-go')}
            if variant == 'symlink-explicit-home':
                env['KIMI_CODE_HOME'] = str(root/'link/../home/.kimi-code')
            results = []
            for binary in [go, parent, current]:
                result = subprocess.run([str(binary), 'usage', '--not-a-usage-flag'], env=env, capture_output=True, timeout=15)
                results.append({'exit': result.returncode, 'stdout_b64': base64.b64encode(result.stdout).decode(), 'stderr_b64': base64.b64encode(result.stderr).decode()})
            records.append({'provider': provider, 'variant': variant, 'env': env, 'file_bytes_hex': inputs, 'go': results[0], 'parent': results[1], 'current': results[2], 'current_matches_go': results[0] == results[2]})
            print(provider, variant, 'Go/parent/current exits', [r['exit'] for r in results])
proof = {'candidate': '7e0b63359123f0df8a19176313930a24e0dc4c40', 'parent': 'fdfea20421b77ddf76b9beb94827e3497537cf96', 'oracle_commit': 'dcddcef0df5789123c7c9a7ebe6e01f10e941f2c', 'binary_sha256': {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in [go, parent, current]}, 'pairs': records, 'operator_credentials_or_paid_endpoints': False}
Path('/tmp/symaira-usage768-f759-root-parent-route-extra.json').write_text(json.dumps(proof, indent=2)+'\n')
