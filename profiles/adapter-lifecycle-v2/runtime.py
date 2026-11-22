"""Credential-free child invocation and parent-owned synthetic effect channel."""
from concurrent.futures import ThreadPoolExecutor
from dataclasses import fields
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile

from effect_sink import Binding

LIMIT = 65536


def strict_object(pairs):
    result = dict(pairs)
    if len(pairs) != len(result):
        raise ValueError("duplicate_field")
    return result


def serve_effects(channel, sink):
    # The child inherits only the submit endpoint. The harness assigns every ID,
    # commits each effect before acknowledging it and never accepts an ID from the child.
    with channel, channel.makefile("rb") as stream:
        channel.settimeout(45)
        while True:
            line = stream.readline(4097)
            if not line:
                return
            if len(line) > 4096 or not line.endswith(b"\n"):
                raise ValueError("invalid_effect_frame")
            request = json.loads(line, object_pairs_hook=strict_object)
            if set(request) != {f.name for f in fields(Binding)} or not all(
                    isinstance(value, str) and 0 < len(value) <= 256 for value in request.values()):
                raise ValueError("invalid_effect_binding")
            execution_id = sink.invoke(Binding(**request))
            channel.sendall((execution_id + "\n").encode("ascii"))


def invoke(command, workspace, message, sink=None):
    env = dict(PATH=os.defpath, HOME=str(workspace), TSX_DISABLE_CACHE="1", PYTHONDONTWRITEBYTECODE="1")
    with tempfile.TemporaryDirectory() as tmp, tempfile.TemporaryFile() as output, ThreadPoolExecutor(max_workers=1) as pool:
        env["TMPDIR"] = tmp
        endpoint = None
        worker = None
        try:
            if sink is not None:
                parent, endpoint = socket.socketpair()
                fd = endpoint.fileno()
                env["PSYCHE_TEST_EFFECT_FD"] = str(fd)
                worker = pool.submit(serve_effects, parent, sink)
            result = subprocess.run(command, cwd=workspace, env=env, input=json.dumps(message).encode(),
                                    stdout=output, stderr=subprocess.DEVNULL, timeout=45,
                                    pass_fds=() if endpoint is None else (fd,))
        finally:
            if endpoint is not None:
                endpoint.close()
            if worker is not None:
                worker.result(timeout=46)
        if result.returncode:
            raise ValueError("child_failed")
        output.seek(0)
        payload = output.read(LIMIT + 1)
        if len(payload) > LIMIT:
            raise ValueError("oversize_output")
        value = json.loads(payload, object_pairs_hook=strict_object)
        if not isinstance(value, dict):
            raise ValueError("invalid_output")
        return value
