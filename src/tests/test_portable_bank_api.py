"""Bank startup cannot activate with a partial or stale Native interface."""

import runpy
from pathlib import Path
from types import SimpleNamespace
from typing import Never

import pytest

REPO = Path(__file__).resolve().parents[2]
_METHODS = (
    "capture_bank_participants",
    "capture_bank_mutation",
    "prepare_bank_mutation",
    "publish_bank_mutation",
    "validate_bank_mutation",
    "reconcile_bank_mutation",
    "finish_bank_mutation",
    "cancel_bank_mutation",
    "capture_recovered_bank",
    "validate_recovered_bank",
    "retry_recovered_bank",
    "release_recovered_bank",
)
_TYPES = {
    "BankParticipants": (),
    "CapturedBankMutation": ("publication_status",),
    "BankMutationAcknowledgement": (),
    "BankMutationTicket": (
        "publication_status",
        "acknowledgement",
        "metadata_json",
        "committed_stem_tickets",
    ),
    "RecoveredBankAcceptance": ("metadata_json",),
}


def _forbidden(*_args: object, **_kwargs: object) -> Never:
    pytest.fail("Bank preflight must not construct an engine or execute Native work")


def _capture(
    self: object,
    source_bank: int | None,
    target_bank: int,
    intents: list[str | None],
    *,
    participants: object | None = None,
) -> Never:
    _forbidden()


def _recover(self: object, participants: object, intents: list[str | None]) -> Never:
    _forbidden()


def _capture_without_original(
    self: object, source_bank: int | None, target_bank: int, intents: list[str | None]
) -> Never:
    _forbidden()


def _capture_positional_original(
    self: object,
    source_bank: int | None,
    target_bank: int,
    intents: list[str | None],
    participants: object | None = None,
) -> Never:
    _forbidden()


def _capture_wrong_default(
    self: object,
    source_bank: int | None,
    target_bank: int,
    intents: list[str | None],
    *,
    participants: object = False,
) -> Never:
    _forbidden()


def _restore_rebound(self: object, target_bank: int, intents: list[str | None]) -> Never:
    _forbidden()


def bank_native_module(engine_base: type = object) -> SimpleNamespace:
    engine = type(
        "Engine",
        (engine_base,),
        dict.fromkeys(_METHODS, _forbidden)
        | {
            "__init__": _forbidden,
            "capture_bank_mutation": _capture,
            "capture_recovered_bank": _recover,
        },
    )
    return SimpleNamespace(
        AudioEngine=engine,
        **{
            name: type(name, (), dict.fromkeys(methods, _forbidden) | {"__init__": _forbidden})
            for name, methods in _TYPES.items()
        },
    )


def _check(native: object) -> None:
    runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_bank_api"](native)


def test_complete_bank_preflight_only_reflects_types_without_native_work() -> None:
    _check(bank_native_module())


@pytest.mark.parametrize("method", _METHODS)
def test_partial_bank_engine_is_rejected_before_activation(method: str) -> None:
    native = bank_native_module()
    setattr(native.AudioEngine, method, None)
    with pytest.raises(AssertionError, match=method):
        _check(native)


@pytest.mark.parametrize("name", _TYPES)
def test_missing_bank_owner_type_is_rejected_before_activation(name: str) -> None:
    native = bank_native_module()
    setattr(native, name, None)
    with pytest.raises(AssertionError, match=name):
        _check(native)


@pytest.mark.parametrize(
    ("name", "method"), [(name, method) for name, methods in _TYPES.items() for method in methods]
)
def test_partial_bank_owner_interface_is_rejected_before_activation(name: str, method: str) -> None:
    native = bank_native_module()
    setattr(getattr(native, name), method, None)
    with pytest.raises(AssertionError, match=method):
        _check(native)


@pytest.mark.parametrize(
    ("method", "replacement"),
    [
        ("capture_bank_mutation", _capture_without_original),
        ("capture_bank_mutation", _capture_positional_original),
        ("capture_bank_mutation", _capture_wrong_default),
        ("capture_recovered_bank", _restore_rebound),
    ],
)
def test_stale_bank_admission_signature_is_rejected_before_activation(
    method: str, replacement: object
) -> None:
    native = bank_native_module()
    setattr(native.AudioEngine, method, replacement)
    with pytest.raises(AssertionError, match="Native Bank API invalid"):
        _check(native)
