"""Keep owned config-path process observations before any gate assertion."""
import json
import os
from pathlib import Path
import tempfile


class Journal:
    def __init__(self, report, identities):
        self.path = Path(str(report) + '.progress.json')
        self.data = dict(status='running', identities=identities, events=[])
        self.write()

    def write(self):
        staged = None
        try:
            with tempfile.NamedTempFile(dir=self.path.parent, prefix=self.path.name + '.',
                                       delete=False) as stream:
                staged = Path(stream.name)
                stream.write((json.dumps(self.data, indent=2) + '\n').encode('utf-8'))
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(staged, self.path)
        finally:
            if staged is not None:
                staged.unlink(missing_ok=True)

    def event(self, phase, **fields):
        self.data['events'].append(dict(phase=phase, **fields))
        self.write()

    def failed(self, error):
        self.data.update(status='failed', exception_type=type(error).__name__,
                         exception=str(error))
        self.write()

    def complete(self, report):
        self.data.update(status='complete', result=report)
        self.write()
