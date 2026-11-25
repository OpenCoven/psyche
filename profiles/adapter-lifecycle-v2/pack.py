"""Package the lifecycle driver with the exact reviewed receipt comparator input."""
import hashlib
import json
from pathlib import Path
import sys
from zipfile import ZipFile, ZipInfo

HERE = Path(__file__).resolve().parent
REPLAY_SHA = '6ee74ca1f7e67b2f1c8f6afacac921318a8121b1cd4eb0f970081f4714bfb3a8'
PROFILE = 'psyche.experimental.adapter-lifecycle.v2'


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode('ascii')


def build(replay, output):
    data = replay.read_bytes()
    if hashlib.sha256(data).hexdigest() != REPLAY_SHA:
        raise ValueError('unrecognized replay archive')
    with ZipFile(replay) as archive:
        request = archive.read('golden/request.json')
    files = {'__main__.py': (HERE / 'run.py').read_bytes(), 'receipt-replay.pyz': data, 'request.json': request}
    for name in ('runtime.py', 'effect_sink.py', 'evidence_checks.py'):
        files[name] = (HERE / name).read_bytes()
    for name in ('packaged_controls.py', 'reference_fixture.py'):
        files[name] = (HERE / 'tests' / name).read_bytes()
    manifest = {'profile': PROFILE, 'status': 'experimental', 'supported_readers': [2],
                'receipt_source': 'bada1b6a854e72e0cc20cfe8e54ca2d11c6ca7a9',
                'files': {name: hashlib.sha256(raw).hexdigest() for name, raw in files.items()}}
    files['manifest.json'] = canonical(manifest)
    with ZipFile(output, 'w') as bundle:
        for name, raw in sorted(files.items()):
            info = ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            bundle.writestr(info, raw)
    print(hashlib.sha256(output.read_bytes()).hexdigest(), output.name)


if __name__ == '__main__':
    build(Path(sys.argv[1]), Path(sys.argv[2]))
