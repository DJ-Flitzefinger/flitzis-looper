"""Reject stale Native interfaces before a portable runtime can activate."""

import runpy
from inspect import Parameter, signature
from pathlib import Path
from types import SimpleNamespace
from typing import Never
from unittest.mock import Mock

import pytest

from tests.test_portable_bank_api import bank_native_module
from tests.test_portable_bpm_runtime_api import bpm_runtime_native_module
from tests.test_portable_mix_capture_api import mix_capture_native_module
from tests.test_portable_ordinary_transport_api import ordinary_transport_native_module

REPO = Path(__file__).resolve().parents[2]


class _CompatibleEngine:
    RESIDENT_PCM_ADMISSION_VERSION = 2

    def __init__(self) -> None:
        pytest.fail("Preflight must not construct an engine or open a device")

    def set_resident_pcm_budget_mib(self, value: int) -> None:
        pass

    def get_resident_pcm_budget_status(self) -> dict[str, int]:
        return {}

    def request_committed_stem_residency(
        self, ticket: object, *, interest: str = "explicit"
    ) -> None:
        pass


def test_compatible_class_is_checked_without_an_engine() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_resident_api"]
    check(_CompatibleEngine)


@pytest.mark.parametrize(
    ("attribute", "value", "message"),
    [
        ("RESIDENT_PCM_ADMISSION_VERSION", 0, "protocol missing"),
        ("RESIDENT_PCM_ADMISSION_VERSION", 1, "automatic residency admission protocol missing"),
        ("set_resident_pcm_budget_mib", None, "set_resident_pcm_budget_mib"),
        ("get_resident_pcm_budget_status", None, "get_resident_pcm_budget_status"),
        ("request_committed_stem_residency", lambda self, ticket: None, "preload interest"),
    ],
)
def test_incompatible_native_is_rejected_before_activation(
    attribute: str,
    value: object,
    message: str,
) -> None:
    engine_type = type("IncompatibleEngine", (_CompatibleEngine,), {attribute: value})
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_resident_api"]
    with pytest.raises(AssertionError, match=message):
        check(engine_type)


class _CompatibleCopyEngine(_CompatibleEngine):
    def capture_pad_copy(self, binding: object, target_id: int, intent_json: str) -> object:
        raise NotImplementedError

    def prepare_pad_copy(self, captured: object) -> object:
        raise NotImplementedError

    def publish_pad_copy(self, ticket: object) -> None:
        raise NotImplementedError

    def reconcile_pad_copy(self, ticket: object) -> bool:
        raise NotImplementedError

    def finish_pad_copy(self, ticket: object) -> None:
        raise NotImplementedError

    def cancel_pad_copy(self, captured: object) -> bool:
        raise NotImplementedError


def test_complete_copy_interface_is_checked_without_an_engine() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_copy_api"]
    check(_CompatibleCopyEngine)


@pytest.mark.parametrize(
    "method",
    [
        "capture_pad_copy",
        "prepare_pad_copy",
        "publish_pad_copy",
        "reconcile_pad_copy",
        "finish_pad_copy",
        "cancel_pad_copy",
    ],
)
def test_incomplete_copy_interface_cannot_activate(method: str) -> None:
    engine_type = type("IncompatibleCopyEngine", (_CompatibleCopyEngine,), {method: None})
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_copy_api"]
    with pytest.raises(AssertionError, match=method):
        check(engine_type)


def test_old_residency_interface_alone_cannot_activate_copy_controls() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_copy_api"]
    with pytest.raises(AssertionError, match="capture_pad_copy"):
        check(_CompatibleEngine)


_PAIR_HOLD_METHODS = (
    "_capture_pad_pair",
    "_prepare_pad_pair",
    "_publish_pad_pair",
    "_reconcile_pad_pair",
    "_validate_pad_pair_acknowledgement",
    "_finish_pad_pair",
    "_cancel_pad_pair",
    "capture_recovered_pad_pair",
    "capture_recovered_pad_copy",
    "validate_recovered_pad_copy",
    "release_recovered_pad_copy",
    "validate_recovered_pad_pair",
    "release_recovered_pad_pair",
    "pause_waveform_hold",
    "release_waveform_hold",
    "trigger_captured_input_pad",
    "prepare_captured_input_trigger",
    "trigger_prepared_input_pad",
    "_resolve_pad_feedback",
    "_resolve_loader_event",
)


def _forbidden_native_work(*_arguments: object, **_keywords: object) -> Never:
    pytest.fail("Preflight must reflect the Native class without executing Native work")


def _capture_pad_pair(
    self: object,
    source_id: int,
    target_id: int,
    *,
    source_binding: object | None = None,
) -> object:
    _forbidden_native_work()


_CompatiblePairHoldEngine = type(
    "CompatiblePairHoldEngine",
    (_CompatibleCopyEngine,),
    dict.fromkeys(_PAIR_HOLD_METHODS, _forbidden_native_work)
    | {"_capture_pad_pair": _capture_pad_pair},
)

_CompatibleTimingEngine = type(
    "CompatibleTimingEngine",
    (_CompatiblePairHoldEngine,),
    {
        "_content_placement_pending": _forbidden_native_work,
        "_content_placement_read_token": _forbidden_native_work,
    },
)
_CompatibleTimingTicket = type(
    "CompatibleTimingTicket", (), {"placement_pending": _forbidden_native_work}
)

_PITCH_METHODS = (
    "select_input_pad",
    "input_selected_projection",
    "capture_pad_pitch_action",
    "capture_selected_pitch_action",
    "publish_pitch_action",
    "cancel_pitch_action",
    "set_input_runtime_state",
    "poll_input_events",
    "load_sample_async",
)
_PITCH_CAPTURE_ATTRIBUTES = (
    "accepted_sequence",
    "received_at_ns",
    "due_frame",
    "execution_frame",
    "content_lifetime",
    "content_is_current",
    "base_shift",
    "extra_shift",
    "key_lock",
    "retrigger",
    "resets_key_correction",
    "quantize_step_64ths",
    "sync_enabled",
    "accepted",
    "effective",
    "effective_output_frame",
    "state",
    "reason",
    "current_control_slot",
)


def _capture_pad_pitch(
    self: object, sample_id: int, action_key: str, *, received_at_ns: int | None = None
) -> None:
    _forbidden_native_work(self, sample_id, action_key, received_at_ns=received_at_ns)


def _capture_selected_pitch(
    self: object, action_key: str, *, received_at_ns: int | None = None
) -> None:
    _forbidden_native_work(self, action_key, received_at_ns=received_at_ns)


def _pitch_runtime_state(
    self: object,
    *arguments: object,
    pitch_rows: object = None,
) -> None:
    _forbidden_native_work(self, *arguments, pitch_rows=pitch_rows)


def _pitch_restore_source(
    self: object,
    *arguments: object,
    initial_base_shift: object = None,
    initial_extra_shift: object = None,
    initial_key_lock: object = None,
    initial_gain_db: object = None,
    initial_eq_db: object = None,
) -> None:
    _forbidden_native_work(
        self,
        *arguments,
        initial_base_shift=initial_base_shift,
        initial_extra_shift=initial_extra_shift,
        initial_key_lock=initial_key_lock,
        initial_gain_db=initial_gain_db,
        initial_eq_db=initial_eq_db,
    )


_PITCH_IMPLEMENTATIONS: dict[str, object] = dict.fromkeys(
    _PITCH_METHODS, _forbidden_native_work
) | {
    "capture_pad_pitch_action": _capture_pad_pitch,
    "capture_selected_pitch_action": _capture_selected_pitch,
    "set_input_runtime_state": _pitch_runtime_state,
    "load_sample_async": _pitch_restore_source,
}
_CompatiblePitchEngine = type(
    "CompatiblePitchEngine", (_CompatibleTimingEngine,), _PITCH_IMPLEMENTATIONS
)
_CompatiblePitchCapture = type(
    "CompatiblePitchCapture",
    (),
    dict.fromkeys(_PITCH_CAPTURE_ATTRIBUTES, property(_forbidden_native_work))
    | {"__init__": _forbidden_native_work},
)


def test_complete_pitch_api_is_reflected_without_engine_or_capture_execution() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pitch_action_api"]
    check(
        SimpleNamespace(
            AudioEngine=_CompatiblePitchEngine, CapturedPitchAction=_CompatiblePitchCapture
        )
    )


@pytest.mark.parametrize("method", _PITCH_METHODS)
@pytest.mark.parametrize("missing", [False, True], ids=["not-callable", "absent"])
def test_incomplete_pitch_engine_api_cannot_activate(method: str, *, missing: bool) -> None:
    methods = _PITCH_IMPLEMENTATIONS.copy()
    if missing:
        del methods[method]
    else:
        methods[method] = None
    engine_type = type("IncompletePitchEngine", (_CompatibleTimingEngine,), methods)
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pitch_action_api"]
    with pytest.raises(AssertionError, match=method):
        check(SimpleNamespace(AudioEngine=engine_type, CapturedPitchAction=_CompatiblePitchCapture))


@pytest.mark.parametrize("attribute", _PITCH_CAPTURE_ATTRIBUTES)
def test_incomplete_pitch_receipt_cannot_activate(attribute: str) -> None:
    attributes = dict.fromkeys(_PITCH_CAPTURE_ATTRIBUTES, property(_forbidden_native_work))
    del attributes[attribute]
    capture_type = type("IncompletePitchCapture", (), attributes)
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pitch_action_api"]
    with pytest.raises(AssertionError, match=attribute):
        check(SimpleNamespace(AudioEngine=_CompatiblePitchEngine, CapturedPitchAction=capture_type))


@pytest.mark.parametrize(
    ("method", "keyword"),
    [
        ("set_input_runtime_state", "pitch_rows"),
        ("capture_pad_pitch_action", "received_at_ns"),
        ("capture_selected_pitch_action", "received_at_ns"),
        ("load_sample_async", "initial_base_shift"),
        ("load_sample_async", "initial_extra_shift"),
        ("load_sample_async", "initial_key_lock"),
        ("load_sample_async", "initial_gain_db"),
        ("load_sample_async", "initial_eq_db"),
    ],
)
def test_old_pitch_projection_or_fresh_timestamp_only_capture_cannot_activate(
    method: str, keyword: str
) -> None:
    implementation = _PITCH_IMPLEMENTATIONS[method]
    assert callable(implementation)
    complete = signature(implementation)
    old_method = type(
        "OldMethod",
        (),
        {
            "__call__": _forbidden_native_work,
            "__signature__": complete.replace(
                parameters=[item for name, item in complete.parameters.items() if name != keyword]
            ),
        },
    )()
    engine_type = type("OldPitchSignature", (_CompatiblePitchEngine,), {method: old_method})
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pitch_action_api"]
    with pytest.raises(AssertionError, match=keyword):
        check(SimpleNamespace(AudioEngine=engine_type, CapturedPitchAction=_CompatiblePitchCapture))


def test_missing_pitch_capture_type_cannot_activate() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pitch_action_api"]
    with pytest.raises(AssertionError, match="CapturedPitchAction"):
        check(SimpleNamespace(AudioEngine=_CompatiblePitchEngine))


def _set_bpm_lock(self: object, enabled: object, *, anchor_binding: object = None) -> None:
    _forbidden_native_work(self, enabled, anchor_binding=anchor_binding)


def _bpm_lock_status(self: object) -> None:
    _forbidden_native_work(self)


def _set_master_period(
    self: object, period_seconds: float, *, anchor_binding: object = None
) -> None:
    _forbidden_native_work(self, period_seconds, anchor_binding=anchor_binding)


def _set_speed_and_master_period(
    self: object, speed: float, period_seconds: float, *, anchor_binding: object = None
) -> None:
    _forbidden_native_work(self, speed, period_seconds, anchor_binding=anchor_binding)


_GLOBAL_RATE_IMPLEMENTATIONS: dict[str, object] = {
    "GLOBAL_RATE_PROVENANCE_API_VERSION": 1,
    "global_rate_status": _forbidden_native_work,
    "bpm_lock_status": _bpm_lock_status,
    "set_bpm_lock": _set_bpm_lock,
    "set_master_period": _set_master_period,
    "set_speed_and_master_period": _set_speed_and_master_period,
}


def test_global_rate_observation_is_reflected_without_native_work() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_global_rate_api"]
    engine_type = type("RateEngine", (), _GLOBAL_RATE_IMPLEMENTATIONS)
    check(engine_type)


@pytest.mark.parametrize("version", [None, True, 0, 2, 1.0, "1"])
def test_invalid_global_rate_provenance_version_cannot_activate(version: object) -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_global_rate_api"]
    attributes = _GLOBAL_RATE_IMPLEMENTATIONS | {"GLOBAL_RATE_PROVENANCE_API_VERSION": version}
    with pytest.raises(AssertionError, match="GLOBAL_RATE_PROVENANCE_API_VERSION=1"):
        check(type("InvalidProvenanceEngine", (), attributes))


def test_missing_global_rate_provenance_version_cannot_activate() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_global_rate_api"]
    attributes = _GLOBAL_RATE_IMPLEMENTATIONS.copy()
    del attributes["GLOBAL_RATE_PROVENANCE_API_VERSION"]
    with pytest.raises(AssertionError, match="GLOBAL_RATE_PROVENANCE_API_VERSION=1"):
        check(type("OldProvenanceEngine", (), attributes))


@pytest.mark.parametrize("missing", [False, True], ids=["not-callable", "absent"])
def test_global_rate_without_callback_confirmation_cannot_activate(*, missing: bool) -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_global_rate_api"]
    attributes = {} if missing else {"global_rate_status": None}
    with pytest.raises(AssertionError, match=r"AudioEngine\.global_rate_status"):
        check(type("OldRateEngine", (), attributes))


@pytest.mark.parametrize(
    "method",
    ["bpm_lock_status", "set_bpm_lock", "set_master_period", "set_speed_and_master_period"],
)
@pytest.mark.parametrize("missing", [False, True], ids=["not-callable", "absent"])
def test_bpm_lock_without_qualified_api_cannot_activate(method: str, *, missing: bool) -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_global_rate_api"]
    attributes = _GLOBAL_RATE_IMPLEMENTATIONS.copy()
    if missing:
        del attributes[method]
    else:
        attributes[method] = None
    with pytest.raises(AssertionError, match=method):
        check(type("OldBpmEngine", (), attributes))


@pytest.mark.parametrize("variant", ["absent", "positional", "required", "wrong-default"])
@pytest.mark.parametrize(
    "method", ["set_bpm_lock", "set_master_period", "set_speed_and_master_period"]
)
def test_bpm_lock_rejects_unqualified_setter_signatures(variant: str, method: str) -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_global_rate_api"]
    complete = signature(_GLOBAL_RATE_IMPLEMENTATIONS[method])
    anchor = complete.parameters["anchor_binding"]
    if variant == "positional":
        anchor = anchor.replace(kind=Parameter.POSITIONAL_OR_KEYWORD)
    elif variant == "required":
        anchor = anchor.replace(default=Parameter.empty)
    elif variant == "wrong-default":
        anchor = anchor.replace(default=False)
    parameters = list(complete.parameters.values())[:-1]
    if variant != "absent":
        parameters.append(anchor)
    setter = type(
        "OldBpmSetter",
        (),
        {
            "__call__": _forbidden_native_work,
            "__signature__": complete.replace(parameters=parameters),
        },
    )()
    with pytest.raises(AssertionError, match="anchor_binding"):
        check(type("OldBpmEngine", (), _GLOBAL_RATE_IMPLEMENTATIONS | {method: setter}))


_ORDINARY_START_METHODS = (
    "capture_pad_start",
    "prepare_captured_pad_start",
    "publish_pad_start",
)
_ORDINARY_CAPTURE_METHODS = (
    "is_current",
    "same_owner",
    "original_owner_status",
    "state",
    "reason",
    "accepted",
    "effective_frame",
    "original_frame",
    "execution_frame",
    "cancel",
)


def _capture_pad_start(
    self: object,
    id: int,  # noqa: A002 - Native API names the original pad slot.
    start_s: float,
    end_s: float | None = None,
    volume: float = 1.0,
    *,
    exclusive: bool = False,
    received_at_ns: int | None = None,
    expected_source_generation: int | None = None,
    explicit_target: object | None = None,
) -> None:
    _forbidden_native_work(
        self,
        id,
        start_s,
        end_s,
        volume,
        exclusive=exclusive,
        received_at_ns=received_at_ns,
        expected_source_generation=expected_source_generation,
        explicit_target=explicit_target,
    )


def _prepare_captured_pad_start(self: object, captured: object, window: object = None) -> None:
    _forbidden_native_work(self, captured, window)


def _publish_pad_start(self: object, captured: object, window: object = None) -> None:
    _forbidden_native_work(self, captured, window)


def _capture_global_start(
    self: object, entries: object, *, received_at_ns: int | None = None
) -> None:
    _forbidden_native_work(self, entries, received_at_ns=received_at_ns)


def _publish_captured_global_start(
    self: object, captured: object, *, capture: object | None = None
) -> None:
    _forbidden_native_work(self, captured)


def _set_key_lock_start_behavior(self: object, behavior: str) -> bool:
    _forbidden_native_work(self, behavior)


_KEY_START_IMPLEMENTATIONS: dict[str, object] = {
    "key_lock_start_behavior": _forbidden_native_work,
    "can_change_key_lock_start_behavior": _forbidden_native_work,
    "set_key_lock_start_behavior": _set_key_lock_start_behavior,
}


_ORDINARY_START_IMPLEMENTATIONS: dict[str, object] = {
    "capture_pad_start": _capture_pad_start,
    "prepare_captured_pad_start": _prepare_captured_pad_start,
    "publish_pad_start": _publish_pad_start,
    "capture_global_start": _capture_global_start,
    "publish_captured_global_start": _publish_captured_global_start,
}
_CompatibleOrdinaryEngine = type(
    "CompatibleOrdinaryEngine",
    (_CompatiblePitchEngine,),
    _ORDINARY_START_IMPLEMENTATIONS | _KEY_START_IMPLEMENTATIONS | _GLOBAL_RATE_IMPLEMENTATIONS,
)


def test_global_start_policy_interface_is_reflected_without_native_work() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_key_lock_start_behavior_api"
    ]
    check(_CompatibleOrdinaryEngine)


@pytest.mark.parametrize("method", tuple(_KEY_START_IMPLEMENTATIONS))
@pytest.mark.parametrize("missing", [False, True], ids=["not-callable", "absent"])
def test_missing_global_start_policy_interface_cannot_activate(
    method: str, *, missing: bool
) -> None:
    methods = dict(_KEY_START_IMPLEMENTATIONS)
    if missing:
        del methods[method]
    else:
        methods[method] = None
    old_engine = type("MissingKeyLockStartPolicyEngine", (), methods)
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_key_lock_start_behavior_api"
    ]
    with pytest.raises(AssertionError, match=method):
        check(old_engine)


def test_policy_setter_requires_explicit_behavior_without_native_execution() -> None:
    old_engine = type(
        "MissingExplicitKeyLockBehavior",
        (_CompatibleOrdinaryEngine,),
        {"set_key_lock_start_behavior": _forbidden_native_work},
    )
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_key_lock_start_behavior_api"
    ]
    with pytest.raises(AssertionError, match="behavior"):
        check(old_engine)


_ORDINARY_CAPTURE_IMPLEMENTATIONS = dict.fromkeys(
    _ORDINARY_CAPTURE_METHODS, _forbidden_native_work
) | {
    "diagnostic_slot": property(_forbidden_native_work),
    "__init__": _forbidden_native_work,
}
_CompatibleOrdinaryCapture = type(
    "CompatibleOrdinaryCapture", (), _ORDINARY_CAPTURE_IMPLEMENTATIONS
)
_CompatibleOrdinaryTrigger = type(
    "CompatibleOrdinaryTrigger",
    (),
    {
        "captured_start": property(_forbidden_native_work),
        "__init__": _forbidden_native_work,
    },
)

_GLOBAL_CAPTURE_METHODS = ("diagnostic_slots", "original_frame", "state", "reason", "cancel")
_CompatibleGlobalCapture = type(
    "CompatibleGlobalCapture",
    (),
    dict.fromkeys(_GLOBAL_CAPTURE_METHODS, _forbidden_native_work)
    | {"__init__": _forbidden_native_work},
)


def test_complete_global_capture_api_is_reflected_without_native_work() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_captured_global_start_api"
    ]
    check(
        SimpleNamespace(
            AudioEngine=_CompatibleOrdinaryEngine, CapturedGlobalStart=_CompatibleGlobalCapture
        )
    )


@pytest.mark.parametrize("method", _GLOBAL_CAPTURE_METHODS)
def test_missing_original_global_capture_method_cannot_activate(method: str) -> None:
    capture = type(
        "IncompleteGlobalCapture",
        (),
        {name: _forbidden_native_work for name in _GLOBAL_CAPTURE_METHODS if name != method},
    )
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_captured_global_start_api"
    ]
    with pytest.raises(AssertionError, match=method):
        check(SimpleNamespace(AudioEngine=_CompatibleOrdinaryEngine, CapturedGlobalStart=capture))


@pytest.mark.parametrize("method", ["capture_global_start", "publish_captured_global_start"])
def test_missing_original_global_engine_method_cannot_activate(method: str) -> None:
    engine = type("IncompleteGlobalEngine", (_CompatibleOrdinaryEngine,), {method: None})
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_captured_global_start_api"
    ]
    with pytest.raises(AssertionError, match=method):
        check(SimpleNamespace(AudioEngine=engine, CapturedGlobalStart=_CompatibleGlobalCapture))


def test_missing_original_global_capture_type_cannot_activate() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_captured_global_start_api"
    ]
    with pytest.raises(AssertionError, match="CapturedGlobalStart"):
        check(SimpleNamespace(AudioEngine=_CompatibleOrdinaryEngine))


@pytest.mark.parametrize("incompatibility", ["absent", "required", "wrong-default"])
def test_global_capture_timestamp_contract_cannot_activate_when_incompatible(
    incompatibility: str,
) -> None:
    complete = signature(_capture_global_start)
    parameters = list(complete.parameters.values())
    if incompatibility == "absent":
        parameters = [parameter for parameter in parameters if parameter.name != "received_at_ns"]
    else:
        parameters[-1] = parameters[-1].replace(
            default=Parameter.empty if incompatibility == "required" else 0
        )
    method = type(
        "OldGlobalCaptureMethod",
        (),
        {
            "__call__": _forbidden_native_work,
            "__signature__": complete.replace(parameters=parameters),
        },
    )()
    engine = type(
        "OldGlobalCaptureEngine", (_CompatibleOrdinaryEngine,), {"capture_global_start": method}
    )
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_captured_global_start_api"
    ]
    with pytest.raises(AssertionError, match="received_at_ns"):
        check(SimpleNamespace(AudioEngine=engine, CapturedGlobalStart=_CompatibleGlobalCapture))


@pytest.mark.parametrize("incompatibility", ["absent", "required", "wrong-default", "positional"])
def test_global_publication_requires_optional_keyword_capture(
    incompatibility: str,
) -> None:
    complete = signature(_publish_captured_global_start)
    parameters = list(complete.parameters.values())
    if incompatibility == "absent":
        parameters.pop()
    elif incompatibility == "positional":
        parameters[-1] = parameters[-1].replace(kind=Parameter.POSITIONAL_OR_KEYWORD)
    else:
        parameters[-1] = parameters[-1].replace(
            default=Parameter.empty if incompatibility == "required" else False
        )
    method = type(
        "OldGlobalPublicationMethod",
        (),
        {
            "__call__": _forbidden_native_work,
            "__signature__": complete.replace(parameters=parameters),
        },
    )()
    engine = type(
        "OldGlobalPublicationEngine",
        (_CompatibleOrdinaryEngine,),
        {"publish_captured_global_start": method},
    )
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_captured_global_start_api"
    ]
    with pytest.raises(AssertionError, match="capture"):
        check(SimpleNamespace(AudioEngine=engine, CapturedGlobalStart=_CompatibleGlobalCapture))


def test_complete_ordinary_start_api_is_reflected_without_native_work() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_ordinary_start_api"]
    check(
        SimpleNamespace(
            AudioEngine=_CompatibleOrdinaryEngine,
            CapturedPadStart=_CompatibleOrdinaryCapture,
            CapturedInputPadTrigger=_CompatibleOrdinaryTrigger,
        )
    )


@pytest.mark.parametrize("method", _ORDINARY_START_METHODS)
@pytest.mark.parametrize("missing", [False, True], ids=["not-callable", "absent"])
def test_incomplete_ordinary_start_engine_cannot_activate(method: str, *, missing: bool) -> None:
    methods: dict[str, object] = dict(_ORDINARY_START_IMPLEMENTATIONS)
    if missing:
        del methods[method]
    else:
        methods[method] = None
    engine_type = type("IncompleteOrdinaryEngine", (_CompatiblePitchEngine,), methods)
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_ordinary_start_api"]
    with pytest.raises(AssertionError, match=method):
        check(SimpleNamespace(AudioEngine=engine_type, CapturedPadStart=_CompatibleOrdinaryCapture))


@pytest.mark.parametrize("attribute", ["diagnostic_slot", *_ORDINARY_CAPTURE_METHODS])
@pytest.mark.parametrize("missing", [False, True], ids=["not-callable", "absent"])
def test_incomplete_ordinary_start_receipt_cannot_activate(
    attribute: str, *, missing: bool
) -> None:
    attributes = _ORDINARY_CAPTURE_IMPLEMENTATIONS.copy()
    if missing:
        del attributes[attribute]
    else:
        attributes[attribute] = None
    capture_type = type("IncompleteOrdinaryCapture", (), attributes)
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_ordinary_start_api"]
    with pytest.raises(AssertionError, match=attribute):
        check(SimpleNamespace(AudioEngine=_CompatibleOrdinaryEngine, CapturedPadStart=capture_type))


@pytest.mark.parametrize(
    ("method", "keyword"),
    [
        ("capture_pad_start", "exclusive"),
        ("capture_pad_start", "received_at_ns"),
        ("capture_pad_start", "expected_source_generation"),
        ("capture_pad_start", "explicit_target"),
        ("publish_pad_start", "window"),
        ("prepare_captured_pad_start", "window"),
    ],
)
@pytest.mark.parametrize(
    "incompatibility", ["absent", "positional-only", "required", "wrong-default"]
)
def test_incompatible_ordinary_start_keywords_cannot_activate(
    method: str, keyword: str, incompatibility: str
) -> None:
    implementation = _ORDINARY_START_IMPLEMENTATIONS[method]
    assert callable(implementation)
    complete = signature(implementation)
    parameters = list(complete.parameters.values())
    position = next(
        index for index, parameter in enumerate(parameters) if parameter.name == keyword
    )
    if incompatibility == "absent":
        del parameters[position]
    elif incompatibility == "positional-only":
        positional = [
            parameter.replace(kind=Parameter.POSITIONAL_ONLY)
            for parameter in parameters
            if parameter.name != keyword
            and parameter.kind in {Parameter.POSITIONAL_ONLY, Parameter.POSITIONAL_OR_KEYWORD}
        ]
        target = parameters[position].replace(kind=Parameter.POSITIONAL_ONLY)
        remaining = [
            parameter
            for parameter in parameters
            if parameter.name != keyword and parameter.kind == Parameter.KEYWORD_ONLY
        ]
        parameters = [*positional, target, *remaining]
    elif incompatibility == "wrong-default":
        parameters[position] = parameters[position].replace(
            default=not parameters[position].default
        )
    else:
        parameters[position] = parameters[position].replace(default=Parameter.empty)
    old_method = type(
        "OldOrdinaryMethod",
        (),
        {
            "__call__": _forbidden_native_work,
            "__signature__": complete.replace(parameters=parameters),
        },
    )()
    engine_type = type("OldOrdinarySignature", (_CompatibleOrdinaryEngine,), {method: old_method})
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_ordinary_start_api"]
    with pytest.raises(AssertionError, match=keyword):
        check(SimpleNamespace(AudioEngine=engine_type, CapturedPadStart=_CompatibleOrdinaryCapture))


def test_missing_ordinary_start_capture_type_cannot_activate() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_ordinary_start_api"]
    with pytest.raises(AssertionError, match="CapturedPadStart"):
        check(SimpleNamespace(AudioEngine=_CompatibleOrdinaryEngine))


@pytest.mark.parametrize(
    "trigger_type",
    [
        None,
        type("MissingOriginalStartGetter", (), {}),
        type("DisabledOriginalStartGetter", (), {"captured_start": None}),
        type("NonGetterOriginalStart", (), {"captured_start": object()}),
    ],
    ids=["missing-type", "missing-getter", "disabled-getter", "not-a-getter"],
)
def test_missing_original_midi_start_observation_cannot_activate(trigger_type: object) -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_ordinary_start_api"]
    with pytest.raises(AssertionError, match="CapturedInputPadTrigger"):
        check(
            SimpleNamespace(
                AudioEngine=_CompatibleOrdinaryEngine,
                CapturedPadStart=_CompatibleOrdinaryCapture,
                CapturedInputPadTrigger=trigger_type,
            )
        )


def test_timing_completion_interface_is_reflected_without_native_work() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_timing_completion_api"
    ]
    check(
        SimpleNamespace(
            AudioEngine=_CompatibleTimingEngine,
            ConstantTimingTicket=_CompatibleTimingTicket,
            AcceptedTimingRefreshTicket=_CompatibleTimingTicket,
        )
    )


@pytest.mark.parametrize(
    ("class_name", "method"),
    [
        ("AudioEngine", "_content_placement_pending"),
        ("AudioEngine", "_content_placement_read_token"),
        ("ConstantTimingTicket", "placement_pending"),
        ("AcceptedTimingRefreshTicket", "placement_pending"),
    ],
)
@pytest.mark.parametrize("missing", [False, True], ids=["not-callable", "absent"])
def test_incomplete_timing_completion_interface_cannot_activate(
    class_name: str, method: str, *, missing: bool
) -> None:
    classes = {
        "AudioEngine": _CompatibleTimingEngine,
        "ConstantTimingTicket": _CompatibleTimingTicket,
        "AcceptedTimingRefreshTicket": _CompatibleTimingTicket,
    }
    classes[class_name] = type("IncompleteTimingType", (), {} if missing else {method: None})
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_timing_completion_api"
    ]
    with pytest.raises(AssertionError, match=class_name):
        check(SimpleNamespace(**classes))


def test_complete_pair_hold_input_interface_is_reflected_without_native_work() -> None:
    bootstrap = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
    for name in ("check_resident_api", "check_pad_copy_api", "check_pad_pair_hold_api"):
        bootstrap[name](_CompatiblePairHoldEngine)


@pytest.mark.parametrize(
    "incompatibility", ["absent", "positional-only", "required", "wrong-default"]
)
def test_incompatible_pair_original_source_keyword_cannot_activate(incompatibility: str) -> None:
    complete = signature(_capture_pad_pair)
    parameters = list(complete.parameters.values())
    original = parameters.pop()
    if incompatibility == "positional-only":
        parameters = [parameter.replace(kind=Parameter.POSITIONAL_ONLY) for parameter in parameters]
        parameters.append(original.replace(kind=Parameter.POSITIONAL_ONLY))
    elif incompatibility == "required":
        parameters.append(original.replace(default=Parameter.empty))
    elif incompatibility == "wrong-default":
        parameters.append(original.replace(default=False))
    old_method = type(
        "OldPairCaptureMethod",
        (),
        {
            "__call__": _forbidden_native_work,
            "__signature__": complete.replace(parameters=parameters),
        },
    )()
    engine_type = type(
        "OldPairCaptureSignature",
        (_CompatiblePairHoldEngine,),
        {"_capture_pad_pair": old_method},
    )
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_pair_hold_api"]
    with pytest.raises(AssertionError, match="source_binding"):
        check(engine_type)


@pytest.mark.parametrize("method", _PAIR_HOLD_METHODS)
@pytest.mark.parametrize("missing", [False, True], ids=["not-callable", "absent"])
def test_incomplete_pair_hold_input_interface_cannot_activate(
    method: str, *, missing: bool
) -> None:
    methods: dict[str, object] = dict.fromkeys(_PAIR_HOLD_METHODS, _forbidden_native_work)
    if missing:
        del methods[method]
    else:
        methods[method] = None
    engine_type = type("IncompatiblePairHoldEngine", (_CompatibleCopyEngine,), methods)
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_pair_hold_api"]
    with pytest.raises(AssertionError, match=method):
        check(engine_type)


def test_copy_interface_alone_cannot_activate_pair_hold_controls() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_pair_hold_api"]
    with pytest.raises(AssertionError, match="_capture_pad_pair"):
        check(_CompatibleCopyEngine)


def test_old_native_is_rejected_before_any_app_or_device_activation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    bootstrap = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
    checkout = tmp_path / "flitzis-looper"
    native = SimpleNamespace(
        flitzis_looper_audio=SimpleNamespace(
            __file__=str(checkout / "src/flitzis_looper_audio/native.pyd")
        ),
        native_build_profile=lambda: "release",
        **vars(bank_native_module(_CompatibleCopyEngine)),
    )
    imported = Mock(return_value=native)
    monkeypatch.setattr(bootstrap["importlib"], "import_module", imported)
    with pytest.raises(AssertionError, match="_capture_pad_pair"):
        bootstrap["check_runtime"](tmp_path, checkout, tmp_path / "Runtime")
    imported.assert_called_once_with("flitzis_looper_audio")


def test_runtime_rejects_native_without_ordinary_start_before_app_imports(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    bootstrap = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
    checkout = tmp_path / "flitzis-looper"
    native = SimpleNamespace(
        flitzis_looper_audio=SimpleNamespace(
            __file__=str(checkout / "src/flitzis_looper_audio/native.pyd")
        ),
        native_build_profile=lambda: "release",
        **vars(bank_native_module(_CompatibleOrdinaryEngine)),
        CapturedPitchAction=_CompatiblePitchCapture,
        ConstantTimingTicket=_CompatibleTimingTicket,
        AcceptedTimingRefreshTicket=_CompatibleTimingTicket,
    )
    imported = Mock(return_value=native)
    monkeypatch.setattr(bootstrap["importlib"], "import_module", imported)
    with pytest.raises(AssertionError, match="CapturedPadStart"):
        bootstrap["check_runtime"](tmp_path, checkout, tmp_path / "Runtime")
    imported.assert_called_once_with("flitzis_looper_audio")


def test_runtime_preflight_accepts_complete_reflected_native_before_activation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    bootstrap = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
    checkout = tmp_path / "flitzis-looper"
    runtime = tmp_path / "Runtime"
    (checkout / "assets/models").mkdir(parents=True)
    (checkout / "assets/models/keynet.onnx").write_bytes(b"preflight path fixture")
    monkeypatch.chdir(checkout)
    monkeypatch.setattr(bootstrap["sys"], "executable", str(runtime / "python/python.exe"))
    complete_native = bank_native_module(_CompatibleOrdinaryEngine)
    vars(complete_native).update(vars(mix_capture_native_module(complete_native.AudioEngine)))
    vars(complete_native).update(vars(bpm_runtime_native_module(complete_native.AudioEngine)))
    vars(complete_native).update(
        vars(ordinary_transport_native_module(complete_native.AudioEngine))
    )
    native = SimpleNamespace(
        flitzis_looper_audio=SimpleNamespace(
            __file__=str(checkout / "src/flitzis_looper_audio/native.pyd")
        ),
        native_build_profile=lambda: "release",
        **vars(complete_native),
        CapturedPitchAction=_CompatiblePitchCapture,
        CapturedPadStart=_CompatibleOrdinaryCapture,
        CapturedInputPadTrigger=_CompatibleOrdinaryTrigger,
        ConstantTimingTicket=_CompatibleTimingTicket,
        CapturedGlobalStart=_CompatibleGlobalCapture,
        AcceptedTimingRefreshTicket=_CompatibleTimingTicket,
    )
    modules: dict[str, object] = {
        "flitzis_looper_audio": native,
        "sitecustomize": SimpleNamespace(__file__=str(runtime / "python/sitecustomize.py")),
        "flitzis_looper.controller.stems": SimpleNamespace(StemController=object()),
        "flitzis_looper.ui.render.sidebar_left": object(),
        "flitzis_looper.controller.stem_generation": SimpleNamespace(
            default_demucs_model_cache_dir=lambda: tmp_path / "models/torch/hub/checkpoints"
        ),
        "flitzis_looper": SimpleNamespace(
            __file__=str(checkout / "src/flitzis_looper/__init__.py")
        ),
    }
    for name in ("imgui_bundle", "torch", "torchaudio", "torchcodec"):
        modules[name] = SimpleNamespace(__file__=str(runtime / f"python/{name}/__init__.py"))
    monkeypatch.setattr(bootstrap["importlib"], "import_module", modules.__getitem__)
    monkeypatch.setitem(bootstrap["check_runtime"].__globals__, "private_dlls", lambda *_: {})
    runner = Mock()
    monkeypatch.setattr(bootstrap["subprocess"], "run", runner)
    bootstrap["check_runtime"](tmp_path, checkout, runtime)
    assert runner.call_count == 3
    assert '"portable_preflight": "passed"' in capsys.readouterr().out


@pytest.mark.parametrize("method", ["global_rate_status", "bpm_lock_status"])
def test_runtime_rejects_missing_rate_confirmation_before_app_imports(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, method: str
) -> None:
    bootstrap = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
    checkout = tmp_path / "flitzis-looper"
    old_engine = type("OldRateEngine", (_CompatibleOrdinaryEngine,), {method: None})
    native = SimpleNamespace(
        flitzis_looper_audio=SimpleNamespace(
            __file__=str(checkout / "src/flitzis_looper_audio/native.pyd")
        ),
        native_build_profile=lambda: "release",
        **vars(bank_native_module(old_engine)),
        CapturedPitchAction=_CompatiblePitchCapture,
        CapturedPadStart=_CompatibleOrdinaryCapture,
        CapturedInputPadTrigger=_CompatibleOrdinaryTrigger,
        ConstantTimingTicket=_CompatibleTimingTicket,
        CapturedGlobalStart=_CompatibleGlobalCapture,
        AcceptedTimingRefreshTicket=_CompatibleTimingTicket,
    )
    imported = Mock(return_value=native)
    monkeypatch.setattr(bootstrap["importlib"], "import_module", imported)
    with pytest.raises(AssertionError, match=rf"AudioEngine\.{method}"):
        bootstrap["check_runtime"](tmp_path, checkout, tmp_path / "Runtime")
    imported.assert_called_once_with("flitzis_looper_audio")
