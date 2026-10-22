"""Admission-only pause runtimes cannot activate controls that require Native truth."""

import runpy
from pathlib import Path
from types import SimpleNamespace
from typing import Never

import pytest

REPO = Path(__file__).resolve().parents[2]
_ADMISSION_GETTERS = ("content_lifetime", "transport_revision", "requested_paused")
_OBSERVATION_GETTERS = (
    "sample_id",
    "content_lifetime",
    "transport_revision",
    "observation_sequence",
    "status_kind",
    "requested_effect",
    "outcome",
    "active",
    "ordinary_paused",
)


def _forbidden(*_args: object, **_kwargs: object) -> Never:
    pytest.fail("Ordinary transport preflight must never execute Native work or open a device")


def _self(self: object) -> Never:
    _forbidden()


def _pad(self: object, id: int) -> Never:  # noqa: A002 - Preserve the Native signature.
    _forbidden()


def _resolve(self: object, message: object) -> Never:
    _forbidden()


def _admission(self: object, admission: object) -> Never:
    _forbidden()


def ordinary_transport_native_module(engine_base: type = object) -> SimpleNamespace:
    """Return reflection-only current capabilities for complete-runtime preflight fixtures."""
    admission = type(
        "OrdinaryTransportAdmission",
        (),
        {name: property(_self) for name in _ADMISSION_GETTERS}
        | {"__init__": _forbidden, "current_control_slot": _self, "current_observation": _self},
    )
    observation = type(
        "OrdinaryTransportObservation",
        (),
        {name: property(_self) for name in _OBSERVATION_GETTERS}
        | {
            "__init__": _forbidden,
            "matches_admission": _admission,
            "settles_admission": _admission,
            "current_control_slot": _self,
            "current_observation": _self,
        },
    )
    feedback = type(
        "OrdinaryTransportFeedback",
        (),
        {
            name: property(_self)
            for name in (
                "owner",
                "cell",
                "lifetime",
                "transport_revision",
                "observation_sequence",
            )
        }
        | {"__init__": _forbidden, "feedback_kind": _self},
    )
    return SimpleNamespace(
        AudioEngine=type(
            "Engine",
            (engine_base,),
            {
                "__init__": _forbidden,
                "ORDINARY_TRANSPORT_API_VERSION": 1,
                "pause_sample": _pad,
                "resume_sample": _pad,
                "_resolve_ordinary_transport_feedback": _resolve,
                "_current_ordinary_transport_observation": _pad,
            },
        ),
        OrdinaryTransportAdmission=admission,
        OrdinaryTransportObservation=observation,
        AudioMessage=type("AudioMessage", (), {"OrdinaryTransportFeedback": feedback}),
    )


def _check(native: object) -> None:
    runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_ordinary_transport_api"](
        native
    )


def test_complete_ordinary_transport_interface_is_reflected_without_native_work() -> None:
    _check(ordinary_transport_native_module())


@pytest.mark.parametrize("version", [None, False, True, 0, 2, 1.0, "1"])
def test_ordinary_transport_version_requires_exact_current_integer(version: object) -> None:
    native = ordinary_transport_native_module()
    native.AudioEngine.ORDINARY_TRANSPORT_API_VERSION = version
    with pytest.raises(AssertionError, match="ORDINARY_TRANSPORT_API_VERSION"):
        _check(native)


@pytest.mark.parametrize(
    ("class_name", "method"),
    [
        ("AudioEngine", "pause_sample"),
        ("AudioEngine", "resume_sample"),
        ("AudioEngine", "_resolve_ordinary_transport_feedback"),
        ("AudioEngine", "_current_ordinary_transport_observation"),
        ("OrdinaryTransportAdmission", "current_control_slot"),
        ("OrdinaryTransportAdmission", "current_observation"),
        ("OrdinaryTransportObservation", "matches_admission"),
        ("OrdinaryTransportObservation", "settles_admission"),
        ("OrdinaryTransportObservation", "current_control_slot"),
        ("OrdinaryTransportObservation", "current_observation"),
    ],
)
@pytest.mark.parametrize("missing", [False, True])
def test_incomplete_ordinary_transport_methods_cannot_activate(
    class_name: str, method: str, *, missing: bool
) -> None:
    native = ordinary_transport_native_module()
    if missing:
        delattr(getattr(native, class_name), method)
    else:
        setattr(getattr(native, class_name), method, None)
    with pytest.raises(AssertionError, match=method):
        _check(native)


@pytest.mark.parametrize(
    ("class_name", "attribute"),
    [("OrdinaryTransportAdmission", name) for name in _ADMISSION_GETTERS]
    + [("OrdinaryTransportObservation", name) for name in _OBSERVATION_GETTERS],
)
def test_ordinary_state_requires_readonly_observation_getters(
    class_name: str, attribute: str
) -> None:
    native = ordinary_transport_native_module()
    setattr(getattr(native, class_name), attribute, property(_self, _forbidden))
    with pytest.raises(AssertionError, match=attribute):
        _check(native)


def test_old_pause_none_signature_cannot_hide_missing_receipt_capability() -> None:
    native = ordinary_transport_native_module()
    del native.OrdinaryTransportAdmission
    with pytest.raises(AssertionError, match="OrdinaryTransportAdmission"):
        _check(native)


def test_ordinary_current_query_cannot_gain_optional_lookalike_arguments() -> None:
    native = ordinary_transport_native_module()

    def current_observation(self: object, *, current: bool = True) -> Never:
        _forbidden()

    native.OrdinaryTransportAdmission.current_observation = current_observation
    with pytest.raises(AssertionError, match="current_observation"):
        _check(native)


def test_ordinary_feedback_requires_stable_cell_and_sequence() -> None:
    native = ordinary_transport_native_module()
    del native.AudioMessage.OrdinaryTransportFeedback.observation_sequence
    with pytest.raises(AssertionError, match="observation_sequence"):
        _check(native)
