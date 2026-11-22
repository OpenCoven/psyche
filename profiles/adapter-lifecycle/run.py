"""Exercise one explicitly supplied offline adapter in fresh processes."""
import argparse
import hashlib
import json
import os
from decimal import Decimal, InvalidOperation
from pathlib import Path
import re
import subprocess
import sys
from tempfile import TemporaryDirectory
from zipfile import ZipFile, BadZipFile

PROFILE = 'psyche.experimental.adapter-lifecycle.v1'
LIMIT = 16 * 1024
MEMBERS = {'__main__.py', 'vectors.json', 'request.json', 'receipt-replay.pyz',
           'adapter-input.schema.json', 'adapter-output.schema.json'}


class Failure(ValueError):
    pass


def require(ok, code):
    if not ok:
        raise Failure(code)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def unique(pairs):
    result = dict(pairs)
    require(len(result) == len(pairs), 'duplicate_field')
    return result


def invoke(argv, root, message=None):
    # Use private files to avoid unbounded buffering of child output in memory.
    with TemporaryDirectory() as io:
        output = Path(io, 'stdout')
        with output.open('wb') as target:
            result = subprocess.run(argv, cwd=root, input=json.dumps(message).encode(),
                                    stdout=target, stderr=subprocess.DEVNULL, timeout=30,
                                    env={'PATH': os.defpath, 'HOME': str(root), 'TMPDIR': io,
                                         'TSX_DISABLE_CACHE': '1', 'PYTHONDONTWRITEBYTECODE': '1'})
        require(result.returncode == 0, 'child_failed')
        require(output.stat().st_size <= LIMIT, 'oversize_output')
        return json.loads(output.read_bytes(), object_pairs_hook=unique, parse_float=Decimal)


def snapshot(root):
    return {p.relative_to(root): digest(p.read_bytes()) for p in root.rglob('*') if p.is_file()}


def load_bundle(path, pin):
    require(digest(path.read_bytes()) == pin, 'archive_digest_mismatch')
    with ZipFile(path) as archive:
        require(sorted(archive.namelist()) == sorted(MEMBERS | {'manifest.json'}), 'archive_members')
        manifest = json.loads(archive.read('manifest.json'))
        files = {n: archive.read(n) for n in MEMBERS}
    require(manifest['profile'] == PROFILE and manifest['supported_readers'] == [1], 'wrong_manifest')
    require({n: digest(data) for n, data in files.items()} == manifest['files'], 'member_digest_mismatch')
    return files


def check_count(value, expected, error):
    # JSON Schema accepts any spelling of an integral number. Compare before
    # converting so a tiny fraction cannot round to one or a large exponent expand.
    count = value.get('executions') if isinstance(value, dict) else None
    require(type(count) in (int, Decimal) and count == expected, error)
    value['executions'] = expected


def observation(value, expected, schema):
    check_count(value, expected['executions'], 'invalid_observation')
    require(set(value) == set(schema['oneOf'][1]['required']) | {'request_id', 'request_digest'}, 'invalid_fields')
    for key in ('task_id', 'action_id', 'approval_id', 'approval_digest', 'receipt_hex', 'request_id', 'request_digest'):
        rule = schema['properties'][key]
        raw = value[key]
        require(isinstance(raw, str) and len(raw) <= rule.get('maxLength', LIMIT)
                and re.fullmatch(rule['pattern'], raw), 'invalid_binding')
    require(all(value[k] == v for k, v in expected.items()), 'unexpected_observation')


def qualify(files, adapter, root, results):
    request = files['request.json']
    request_id = json.loads(request)['request_id']
    message = dict(profile=PROFILE, reader_version=1, request_hex=request.hex(), request_id=request_id)
    vectors = json.loads(files['vectors.json'])
    schema = json.loads(files['adapter-output.schema.json'])
    comparator = root / 'receipt-replay.pyz'
    comparator.write_bytes(files['receipt-replay.pyz'])
    request_path = root / 'request.json'
    request_path.write_bytes(request)

    def check_denial(case, workspace, **changes):
        before = snapshot(workspace)
        out = invoke(adapter, workspace, {**message, **changes})
        check_count(out, vectors[case]['executions'], case + '_wrong_denial')
        require(out == vectors[case], case + '_wrong_denial')
        if changes['phase'] == 'replay' or case == 'unknown_version':
            require(before == snapshot(workspace), case + '_mutated_store')
        results.append(dict(case=case, **out))

    for case, extra in [('unknown_version', {'profile': 'psyche.experimental.adapter-lifecycle.v2'}),
                        ('authority_denial', {'fault': 'agent_approval'}), ('cancellation', {'fault': 'cancel'})]:
        workspace = root / case
        workspace.mkdir()
        check_denial(case, workspace, phase='first', **extra)

    for case in ('success', 'ambiguous_adoption'):
        workspace = root / case
        workspace.mkdir()
        extra = {'fault': 'lost_ack'} if case == 'ambiguous_adoption' else {}
        first = invoke(adapter, workspace, dict(message, phase='first', **extra))
        observation(first, vectors[case], schema)
        require(first['request_id'] == request_id and first['request_digest'] == 'sha256:' + digest(request), 'uncorrelated_request')
        native_binding = {k: v for k, v in first.items() if k not in ('request_id', 'request_digest')}
        if case == 'success':
            check_denial('stale_correlation', workspace, phase='replay', request_id='req_01J00000000000000000000001')
            check_denial('incompatible_downgrade', workspace, phase='replay', reader_version=0)
        first_path = root / 'first.json'
        for phase, observed in [('first', first), ('replay', None), ('rollback', None)]:
            if observed is None:
                observed = invoke(adapter, workspace, dict(message, phase=phase, task_id=first['task_id'], action_id=first['action_id']))
            error = case + '_' + phase + '_diverged'
            check_count(observed, vectors[case]['executions'], error)
            require(observed == (native_binding if phase == 'rollback' else first), error)
            path = first_path if phase == 'first' else root / 'current.json'
            path.write_text(json.dumps({'schema_version': 'psyche.experimental.receipt-replay.v1',
                'request_id': first['request_id'], 'request_digest': first['request_digest'],
                'receipt_hex': observed['receipt_hex'], 'executions': observed['executions']}))
            if phase != 'first':
                compared = invoke([sys.executable, '-I', str(comparator), str(request_path), str(first_path), str(path)], root)
                require(compared['additional_executions'] == 0, 'repeat_execution')
            results.append(dict(case=case, phase=phase, **{k: v for k, v in observed.items() if k != 'receipt_hex'},
                                receipt_sha256=digest(bytes.fromhex(observed['receipt_hex']))))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--sha256', required=True)
    parser.add_argument('adapter', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    argv = args.adapter[1:] if args.adapter[:1] == ['--'] else args.adapter
    results = []
    try:
        require(argv and Path(argv[0]).is_absolute(), 'expected_absolute_adapter')
        files = load_bundle(Path(sys.argv[0]), args.sha256)
        with TemporaryDirectory() as root:
            qualify(files, argv, Path(root), results)
    except (OSError, ValueError, KeyError, TypeError, InvalidOperation, BadZipFile, subprocess.TimeoutExpired) as exc:
        print(json.dumps(dict(result='failed', code=str(exc) if isinstance(exc, Failure) else 'invalid_run', cases=results)))
        return 1
    print(json.dumps(dict(result='passed', profile=PROFILE, sha256=args.sha256, cases=results), sort_keys=True))
    return 0


if __name__ == '__main__':
    sys.exit(main())
