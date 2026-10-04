import sys
from pathlib import Path
sys.path.insert(0,'/workspace/symaira-memory758-write-fix/scripts/memory-cli-oracle')
import write_failures
write_failures.CALLBACKS = (
 ('removed-after-kind', "CREATE TRIGGER fixture_after_kind AFTER UPDATE OF kind ON memories BEGIN DELETE FROM memories WHERE id=new.id; END", '', True),
 ('kind-ignored-after-side-effect', "CREATE TRIGGER fixture_kind BEFORE UPDATE OF kind ON memories BEGIN UPDATE memories SET importance=7 WHERE id=new.id; SELECT RAISE(IGNORE); END", '', False),
)
# The side-effect case intentionally changes importance from its default; the
# independent observer below keeps that actual value literal in both processes.
original = write_failures.writes.canonical
def strict_with_callback(state, before, interval, expected, returned_id=None, deleted=False):
    if returned_id and state['memories'][0]['importance'] == 7:
        assert state['memories'][0]['importance'] == 7
        check = __import__('copy').deepcopy(state)
        check['memories'][0]['importance'] = 0
        stable, bindings = original(check, before, interval, expected, returned_id, deleted)
        stable['memories'][0]['importance'] = 7
        return stable, bindings
    return original(state, before, interval, expected, returned_id, deleted)
write_failures.writes.canonical = strict_with_callback
write_failures.execute(Path('/workspace/oracles/symbrain-go-dcddcef0'),Path('/workspace/symaira-memory758-writes/target/debug/symbrain'),Path('/tmp/symaira-memory758-corrected-independent-extra-order.json'))
