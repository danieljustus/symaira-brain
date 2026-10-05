"""Go-only complete-value baseline for the proposed valid-UTF8 device-id family."""
import json
from pathlib import Path
import sys

cases = []
values = ['owned-café', '设备-東京', 'owned-🙂', 'e\u0301', '١٢٣', 'owned\u00a0inside',
          'owned\u200binside', 'owned\ufeffinside', 'owned\u0085inside', 'owned\u2028inside',
          'owned-Ω', 'owned-𐐀', '\u00a0设备\u3000', '\u0085设备\u2028', '\t设备\r\n',
          'owned_ascii', '  owned_ascii\n', ' \u00a0\u3000\t\n', '', '\ufeff设备\ufeff']
for index, value in enumerate(values):
    cases.append(dict(id='device-value-' + str(index), device_hex=value.encode().hex(),
                      device_present=True, token='owned-cli', api='', web='', responses=[200], kind='value'))
for name, statuses, api, web in [
    ('api-short-circuit', [200], 'owned-api', 'owned-web'),
    ('api-to-cli', [401, 200], 'owned-api', 'owned-web'),
    ('cli-to-web', [401, 200], '', 'owned-web'),
    ('all-rejected', [401, 401, 401], 'owned-api', 'owned-web'),
    ('cli-403', [403], '', ''), ('cli-429', [429], '', ''),
    ('cli-500', [500], '', ''), ('cli-invalid-json', [200], '', '')]:
    cases.append(dict(id=name, device_hex='设备-🙂'.encode().hex(), device_present=True,
                      token='owned-cli', api=api, web=web, responses=statuses, kind=name))
for name, value in [('invalid-ff', b'owned-\xff'), ('invalid-e282', b'owned-\xe2\x82'),
                    ('internal-lf', b'owned\ndevice'), ('internal-crlf', b'owned\r\ndevice'),
                    ('internal-tab', b'owned\tdevice'), ('internal-del', b'owned\x7fdevice'),
                    ('internal-nul', b'owned\x00device')]:
    cases.append(dict(id=name, device_hex=value.hex(), device_present=True, token='owned-cli',
                      api='', web='', responses=[200], kind='retain-go-control'))
for name, value in [('exact65536', b'x' * 65534 + 'é'.encode()), ('oversize65537', b'x' * 65535 + 'é'.encode())]:
    cases.append(dict(id=name, device_hex=value.hex(), device_present=True, token='owned-cli',
                      api='', web='', responses=[200], kind='bounded-source'))
cases.append(dict(id='device-absent', device_hex='', device_present=False, token='owned-cli', api='', web='', responses=[200], kind='absent'))
cases.append(dict(id='device-unicode-no-cli-api', device_hex='设备'.encode().hex(), device_present=True,
                  token='', api='owned-api', web='', responses=[200], kind='irrelevant-device'))
assert len(cases) == len({row['id'] for row in cases}) == 39
Path(sys.argv[1]).write_text(json.dumps(cases, indent=2) + '\n')
