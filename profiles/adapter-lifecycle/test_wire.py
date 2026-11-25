"""Exercise the packaged lifecycle command and the receipt comparator embedded inside it."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
from zipfile import ZipFile


def sha(data):
    return hashlib.sha256(data).hexdigest()


def receipt_contract(kit, root):
    checker = root / 'receipt.pyz'
    with ZipFile(kit) as bundle:
        checker.write_bytes(bundle.read('receipt-replay.pyz'))
    with ZipFile(checker) as inner:
        launch = json.loads(inner.read('golden/request.json'))
        observation = json.loads(inner.read('golden/observation.json'))
    descendant = launch['project_root'] + '/child'
    cases = [
        (descendant, 1, 'passed'),
        (launch['project_root'] + '-sibling', 1, 'cwd_outside_project_root'),
        (descendant, 1.0, 'passed'),
        (descendant, 1.5, 'invalid_observation'),
        (descendant, True, 'invalid_observation'),
    ]
    for cwd, executions, expected in cases:
        raw = json.dumps(dict(launch, cwd=cwd), sort_keys=True, separators=(',', ':')).encode()
        first = dict(observation, request_digest='sha256:' + sha(raw))
        (root / 'request').write_bytes(raw)
        (root / 'first').write_text(json.dumps(first))
        (root / 'replay').write_text(json.dumps(dict(first, executions=executions)))
        run = subprocess.run([sys.executable, '-I', str(checker), 'request', 'first', 'replay'],
                             cwd=root, capture_output=True)
        result = json.loads(run.stdout)
        if expected == 'passed':
            valid = run.returncode == 0 and result.get('result') == expected
        else:
            valid = run.returncode == 1 and result == {'result': 'denied', 'code': expected}
        if not valid:
            raise AssertionError((cwd, executions, expected, result))
        print(json.dumps(dict(checker='embedded', cwd=cwd, executions=executions, outcome=expected)))


def fixture(phase, spelling):
    # This is a wire-format fixture. Build's native canary proves the actual lifecycle.
    request = json.load(sys.stdin)
    phase_requested = request['phase']
    uncertain = request.get('fault') == 'lost_ack'
    if uncertain:
        Path('lost-ack').touch()
    digest = sha(bytes.fromhex(request['request_hex']))
    binding = dict(task_id='task', action_id='action', approval_id='approval', approval_digest='1' * 64,
                   receipt_hex='01', state='unknown' if Path('lost-ack').exists() else 'succeeded', executions=1)
    if request['profile'].endswith('.v2'):
        reply = dict(code='unsupported_version', executions=0)
    elif request.get('fault') in ('agent_approval', 'cancel'):
        code = 'operator_required' if request['fault'] == 'agent_approval' else 'action_invalidated'
        reply = dict(code=code, executions=0)
    elif request['reader_version'] == 0:
        reply = dict(code='incompatible_downgrade', executions=1)
    elif request['request_id'] != json.loads(bytes.fromhex(request['request_hex']))['request_id']:
        reply = dict(code='stale_correlation', executions=1)
        phase_requested = 'denial'
    else:
        reply = binding
        if phase_requested != 'rollback':
            reply.update(request_digest='sha256:' + digest, request_id=request['request_id'])
    # Inject the literal token only at the designated stage; other stages remain valid.
    raw = json.dumps(reply)
    if phase_requested == phase and (phase == 'denial' or 'code' not in reply):
        raw = raw.replace('"executions": 1', '"executions": ' + spelling)
    print(raw)


def lifecycle(kit, root):
    pin = sha(kit.read_bytes())
    wrong = {'first': 'invalid_observation', 'denial': 'stale_correlation_wrong_denial',
             'replay': 'success_replay_diverged', 'rollback': 'success_rollback_diverged'}
    for phase, denial in wrong.items():
        for token in ('1', '1.0', '1e0', '1.00000000000000000001', '1.5', 'true'):
            run = subprocess.run([sys.executable, '-I', str(kit), '--sha256', pin, '--',
                                  sys.executable, '-I', str(Path(__file__).resolve()), 'fixture', phase, token],
                                 cwd=root, capture_output=True)
            result = json.loads(run.stdout)
            positive = token in ('1', '1.0', '1e0')
            expected = 'passed' if positive else denial
            actual = result.get('code', result['result'])
            if run.returncode != (0 if positive else 1) or actual != expected:
                raise AssertionError((phase, token, expected, actual))
            print(json.dumps(dict(phase=phase, token=token, outcome=actual)))


if __name__ == '__main__':
    if sys.argv[1] == 'fixture':
        fixture(*sys.argv[2:])
    else:
        archive = Path(sys.argv[1]).resolve()
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            receipt_contract(archive, root)
            lifecycle(archive, root)
