"""Actual owned Python child controls; no SDK, product, endpoint or provider."""
import argparse
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(exist_ok=False)
    sys.path.insert(0, str(args.source / 'browse/port/harness'))
    import test_registry_progress as tests

    records = []
    original_event = tests.Progress.event
    original_end = tests.Progress.end_cli
    for mode in ('ordinary', 'missing-launch', 'wrong-case'):
        folder = args.out / mode
        folder.mkdir()

        class RetainedDirectory:
            def __init__(self):
                self.path = tempfile.mkdtemp(prefix='actual-fixture-', dir=folder)

            def __enter__(self):
                return self.path

            def __exit__(self, *_):
                # Retain all actual journal/stream/receipt bytes after failure.
                pass

        def event(self, stage, **fields):
            if stage == 'cli.launch.begin':
                return None
            return original_event(self, stage, **fields)

        def end(self, case, result, **diagnostic):
            return original_end(self, case + 1000, result, **diagnostic)

        with patch.object(tests.tempfile, 'TemporaryDirectory', RetainedDirectory):
            changes = (patch.object(tests.Progress, 'event', event) if mode == 'missing-launch'
                       else patch.object(tests.Progress, 'end_cli', end) if mode == 'wrong-case'
                       else patch.object(tests.Progress, 'event', original_event))
            with changes, (folder / 'unittest.txt').open('w') as output:
                test = tests.ProgressTests('test_concurrent_cases_keep_complete_lines_and_distinct_case_links')
                result = unittest.TextTestRunner(stream=output).run(unittest.TestSuite([test]))
        journals = list(folder.glob('actual-fixture-*/progress.jsonl'))
        assert len(journals) == 1
        journal = journals[0]
        lines = [json.loads(line) for line in journal.read_bytes().splitlines()]
        starts = {line['sequence'] for line in lines if line['stage'] == 'cli.begin'}
        ends = [line for line in lines if line['stage'] == 'cli.end']
        assert len(starts) == len(ends) == 8 and all(line['exit'] == 0 for line in ends)
        assert not result.errors and result.testsRun == 1
        if mode == 'ordinary':
            assert result.wasSuccessful() and len(lines) == 33
            assert {line['case'] for line in ends} == starts
        elif mode == 'missing-launch':
            assert len(result.failures) == 1 and len(lines) == 25
            assert not any(line['stage'] == 'cli.launch.begin' for line in lines)
            assert {line['case'] for line in ends} == starts
            assert 'Lists differ' in result.failures[0][1]
        else:
            assert len(result.failures) == 1 and len(lines) == 33
            assert {line['case'] for line in ends} == {case + 1000 for case in starts}
            assert 'not found in' in result.failures[0][1]
        records.append(dict(mode=mode,actual_Python_children=8,records=len(lines),
                            test_success=result.wasSuccessful(),intended_control_verified=True,
                            journal=str(journal),raw_streams_retained=True))
    (args.out / 'results.json').write_text(json.dumps(dict(rows=records,product_SDK_executions=0), indent=2) + '\n')


if __name__ == '__main__':
    main()
