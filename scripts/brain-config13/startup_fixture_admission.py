"""Exact owned raw-name admission; unavailable never counts as CLI equality."""
import os
import sys

RAW_COMPONENT = b'blocked-\xff\xe2\x82'


class UnavailableRawFixture(Exception):
    def __init__(self, path, error):
        self.observation = dict(case_id='corrected-secret-nested-raw-blocker',
            path_bytes_hex=os.fsencode(path).hex(), component_hex=RAW_COMPONENT.hex(),
            platform=sys.platform, operation='create-owned-blocker', errno=error.errno,
            diagnostic=str(error), status='UNEXECUTED', cli_children=0)
        super().__init__(self.observation)


def unavailable(path, error):
    return (sys.platform == 'darwin' and error.errno == 92
            and os.fsencode(path.name) == RAW_COMPONENT)


def create_raw_blocker(path):
    assert os.fsencode(path.name) == RAW_COMPONENT
    try:
        path.write_bytes(b'owned immutable raw blocker')
    except OSError as error:
        if unavailable(path, error):
            assert not path.exists(), 'unavailable fixture left a filesystem entry'
            raise UnavailableRawFixture(path, error) from error
        raise
