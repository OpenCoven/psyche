"""Independent lifecycle evidence checks. The caller collects native records and sink rows independently."""
from dataclasses import dataclass

from effect_sink import Binding


@dataclass(frozen=True)
class Observation:
    binding: Binding
    state: str
    receipt: bytes
    executions: int


def verify_terminal(binding, journal_state, journal_receipt, reply, effects, expected_state, previous_id=None):
    if expected_state not in ("succeeded", "unknown") or journal_state != expected_state or reply.state != journal_state:
        raise ValueError("terminal_state_mismatch")
    if reply.binding != binding:
        raise ValueError("native_binding_mismatch")
    if not isinstance(journal_receipt, bytes) or not journal_receipt or reply.receipt != journal_receipt:
        raise ValueError("native_receipt_mismatch")
    if len(effects) != 1 or type(reply.executions) is not int or reply.executions != len(effects):
        raise ValueError("effect_count_mismatch")
    effect = effects[0]
    if effect.binding != binding:
        raise ValueError("effect_binding_mismatch")
    if not effect.execution_id or (previous_id is not None and previous_id != effect.execution_id):
        raise ValueError("execution_identity_mismatch")
    return effect.execution_id


def verify_refusal(expected_code, actual_code, effects_before, effects_after):
    # Existing effects can precede a stale replay. A refusal never adds, replaces or removes one.
    if not expected_code or actual_code != expected_code:
        raise ValueError("refusal_mismatch")
    if effects_before != effects_after:
        raise ValueError("refusal_changed_effects")
