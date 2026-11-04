"""Reject mixed output-control builds without opening an Engine or device."""

import inspect
import runpy
from pathlib import Path
from types import SimpleNamespace
from typing import Never

import pytest

REPO = Path(__file__).resolve().parents[2]
OUTPUT_METHODS = (
    "audio_output_inventory_json",
    "output_device_descriptor",
    "audio_stream_failed",
    "begin_audio_output_change",
    "audio_output_change_epoch",
    "prepare_audio_output_restore_plan",
    "cancel_audio_output_change_before_close",
    "retire_audio_output_for_change",
    "last_audio_output_close_receipt_json",
    "reset_after_audio_output_retirement",
    "enable_audio_output_restore",
    "with_audio_output_restore",
    "ensure_audio_output_change_sources_settled",
    "ensure_audio_output_startup_sources_settled",
    "finish_audio_output_change",
    "finish_empty_audio_output_startup",
    "commit_audio_output_configuration",
    "close_failed_audio_output_owner",
    "capture_existing_legacy_stems_for_output",
    "publish_existing_legacy_stems_for_output",
    "restore_legacy_stem_output_selection",
)


def _forbidden(*_args: object, **_kwargs: object) -> Never:
    pytest.fail("Output preflight must never execute Native work or open a device")


def _run(
    self: object, output_options_json: str, *, match_engine_format: bool | None = None
) -> Never:
    _forbidden()


def audio_output_native_module(engine_base: type = object) -> SimpleNamespace:
    """Expose the full output protocol with forbidden constructors and methods."""
    return SimpleNamespace(
        AudioEngine=type(
            "OutputEngine",
            (engine_base,),
            {
                name: _forbidden
                for name in OUTPUT_METHODS
                if not callable(inspect.getattr_static(engine_base, name, None))
            }
            | {"__init__": _forbidden, "run_with_output_options": _run},
        ),
        ExistingLegacyStemWork=type(
            "ExistingLegacyStemWork",
            (),
            dict.fromkeys(("__init__", "prepare", "cancel"), _forbidden),
        ),
        PreparedExistingLegacyStems=type(
            "PreparedExistingLegacyStems",
            (),
            {"__init__": _forbidden},
        ),
        LegacyStemRestoreReceipt=type(
            "LegacyStemRestoreReceipt",
            (),
            dict.fromkeys(("__init__", "status", "is_current", "cancel"), _forbidden),
        ),
    )


def _check(native: object) -> None:
    runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_audio_output_api"](native)


def test_complete_output_protocol_is_reflected_without_native_work() -> None:
    _check(audio_output_native_module())


@pytest.mark.parametrize("method", ["run_with_output_options", *OUTPUT_METHODS])
def test_missing_output_owner_operation_prevents_activation(method: str) -> None:
    native = audio_output_native_module()
    setattr(native.AudioEngine, method, None)
    with pytest.raises(AssertionError, match=method):
        _check(native)


@pytest.mark.parametrize(
    ("name", "method"),
    [
        ("ExistingLegacyStemWork", "prepare"),
        ("ExistingLegacyStemWork", "cancel"),
        ("LegacyStemRestoreReceipt", "status"),
        ("LegacyStemRestoreReceipt", "is_current"),
        ("LegacyStemRestoreReceipt", "cancel"),
    ],
)
def test_incomplete_original_legacy_receipt_prevents_activation(name: str, method: str) -> None:
    native = audio_output_native_module()
    setattr(getattr(native, name), method, None)
    with pytest.raises(AssertionError, match=f"{name}.{method}"):
        _check(native)


@pytest.mark.parametrize(
    "name", ["ExistingLegacyStemWork", "PreparedExistingLegacyStems", "LegacyStemRestoreReceipt"]
)
def test_missing_legacy_type_prevents_activation(name: str) -> None:
    native = audio_output_native_module()
    delattr(native, name)
    with pytest.raises(AssertionError, match=name):
        _check(native)


def _missing_policy(self: object, output_options_json: str) -> Never:
    _forbidden()


def _required_policy(self: object, output_options_json: str, *, match_engine_format: bool) -> Never:
    _forbidden()


def _positional_policy(
    self: object, output_options_json: str, match_engine_format: object = None
) -> Never:
    _forbidden()


@pytest.mark.parametrize("run", [_missing_policy, _required_policy, _positional_policy])
def test_inexact_rollback_policy_signature_prevents_activation(run: object) -> None:
    native = audio_output_native_module()
    native.AudioEngine.run_with_output_options = run
    with pytest.raises(AssertionError, match="match_engine_format"):
        _check(native)
