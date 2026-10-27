"""Global control, existing runtime publication and actual-feedback fences."""

import json
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.transport.source_sync import SourceSyncStatus
from flitzis_looper.input_mapping.actions import (
    LooperAction,
    global_realtime_bpm_toggle_action,
    global_source_sync_toggle_action,
)
from flitzis_looper.input_mapping.storage import load_midi_mapping_file
from flitzis_looper.source_sync_intent import PadSourceSyncIntent
from flitzis_looper.timing_policy import GlobalTimingPolicy
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.test_source_beatmap_selection import _assigned
from tests.flitzis_looper.controller.test_source_sync import _adoption, _feedback
from tests.flitzis_looper.controller.transport.test_source_map_loop import _bind_map, _geometry

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.mark.parametrize(
    ("other_active", "other_phase", "expected"),
    [(False, None, True), (True, True, True), (True, None, None), (None, None, None)],
)
def test_global_phase_requires_all_relevant_active_or_unconfirmed_participants(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    *,
    other_active: bool | None,
    other_phase: bool | None,
    expected: bool | None,
) -> None:
    controller.project.sample_paths[0] = "samples/first.wav"
    controller.project.sample_paths[1] = "samples/second.wav"
    first = SourceSyncStatus(
        requested=True,
        active=True,
        effective=True,
        state="effective",
        grid_phase_confirmed=True,
    )
    other = SourceSyncStatus(
        requested=True,
        active=other_active,
        state="stopped" if other_active is False else "pending",
        grid_phase_confirmed=other_phase,
    )
    monkeypatch.setattr(
        controller.transport.source_sync, "status", Mock(side_effect=[first, other])
    )
    assert controller.transport.timing_policy.status("sync").grid_phase_confirmed is expected


def test_stopped_pads_cannot_create_global_phase_confirmation(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.sample_paths[0] = "samples/stopped.wav"
    stopped = SourceSyncStatus(
        requested=True,
        active=False,
        state="stopped",
        grid_phase_confirmed=True,
    )
    monkeypatch.setattr(controller.transport.source_sync, "status", Mock(return_value=stopped))
    assert controller.transport.timing_policy.status("sync").grid_phase_confirmed is None


@pytest.fixture(autouse=True)
def available_timing_parents(controller: AppController) -> None:
    """Policy/adoption regressions exercise children below explicitly enabled parents."""
    controller.project.multi_loop = True
    controller.project.bpm_lock = True


def _policy(*, sync: bool | None, realtime: bool | None, revision: int = 2) -> GlobalTimingPolicy:
    return GlobalTimingPolicy(schema_version=1, sync=sync, realtime_bpm=realtime, revision=revision)


def _pending_off_adoption(
    controller: AppController, audio: Mock, **changes: object
) -> tuple[dict[str, object], dict[str, object]]:
    _assigned(controller, audio)
    controller.project.global_timing_policy = _policy(sync=False, realtime=False)
    adoption = _adoption(
        target_mode="legacy",
        target_policy_revision=2,
        target_policy_sync=False,
        target_policy_realtime_bpm=True,
        **changes,
    )
    feedback = _feedback(
        audio,
        active=True,
        effective=True,
        intent_revision=0,
        map_revision=None,
        policy_mode="fixed",
        policy_revision=1,
        policy_sync=True,
        policy_realtime_bpm=False,
        settled=True,
        adoption=adoption,
    )
    return feedback, adoption


def test_current_live_refusal_retains_actual_audio_and_only_safe_global_guidance(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _pending_off_adoption(controller, audio_engine_mock, state="refused", reason="unsupported")
    diagnostics = Mock()
    controller.transport.source_sync.bind_diagnostics(diagnostics)
    before = controller.project.model_dump()

    status = controller.transport.source_sync.status(0)

    assert status.state == "refused"
    assert status.requested is False
    assert status.active is True
    assert status.effective is True
    assert status.policy_mode == "fixed"
    assert status.actual_realtime_bpm is False
    assert status.guidance_text == "Live timing change refused."
    assert status.detail is not None
    assert "unsupported" in status.detail
    diagnostics.observe.assert_called_once_with("source_sync", status.detail, pad_id=0, request=0)
    global_status = controller.transport.timing_policy.status("sync")
    assert global_status.requested is False
    assert global_status.effective is True
    assert global_status.pending is True
    assert global_status.detail == status.guidance_text
    assert global_status.detail is not None
    assert "unsupported" not in global_status.detail
    assert controller.project.model_dump() == before


@pytest.mark.parametrize(
    ("state", "execution_frame", "guidance"),
    [
        ("pending", None, "pending"),
        ("applied", 123, "settling"),
    ],
)
def test_live_attempt_and_applied_unsettled_effect_never_acknowledge_audio(
    controller: AppController,
    audio_engine_mock: Mock,
    state: str,
    execution_frame: int | None,
    guidance: str,
) -> None:
    feedback, _ = _pending_off_adoption(
        controller, audio_engine_mock, state=state, execution_frame=execution_frame
    )
    feedback.update(
        effective=False,
        policy_mode="legacy",
        policy_revision=2,
        policy_sync=False,
        policy_realtime_bpm=True,
        settled=False,
    )

    status = controller.transport.source_sync.status(0)

    assert status.state == "pending"
    assert status.guidance == guidance
    assert status.effective is False
    assert status.actual_realtime_bpm is None
    assert controller.transport.timing_policy.status("sync").pending is True


def test_matching_actual_settlement_wins_over_current_refusal_and_old_launch_error(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    feedback, _ = _pending_off_adoption(
        controller, audio_engine_mock, state="refused", reason="capacity"
    )
    controller.transport.residency.report_launch_error(0, "older launch error")
    feedback.update(
        effective=False,
        policy_mode="legacy",
        policy_revision=2,
        policy_sync=False,
        policy_realtime_bpm=True,
    )

    status = controller.transport.source_sync.status(0)

    assert status.state == "off"
    assert status.guidance is None
    assert status.detail is None
    assert status.actual_realtime_bpm is False
    global_status = controller.transport.timing_policy.status("sync")
    assert global_status.pending is False
    assert global_status.detail is None


def _disabled_sync_target(
    controller: AppController, audio: Mock, *, disabled_by: str
) -> GlobalTimingPolicy:
    _assigned(controller, audio)
    policy = _policy(sync=disabled_by != "global", realtime=True)
    controller.project.global_timing_policy = policy
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(
        requested=disabled_by == "global", revision=5
    )
    assert controller.transport.source_sync.requested_mode(0) == "legacy"
    assert controller.project.resolve_pad_source_sync(0) is False
    return policy


@pytest.mark.parametrize("disabled_by", ["global", "pad"])
def test_disabled_sync_with_active_settled_legacy_is_off_without_restart_pending(
    controller: AppController, audio_engine_mock: Mock, disabled_by: str
) -> None:
    policy = _disabled_sync_target(controller, audio_engine_mock, disabled_by=disabled_by)
    before = controller.project.model_dump()
    _feedback(
        audio_engine_mock,
        active=True,
        effective=False,
        intent_revision=5,
        map_revision=None,
        policy_mode="legacy",
        policy_revision=policy.revision,
        policy_sync=policy.sync,
        policy_realtime_bpm=policy.realtime_bpm,
        settled=True,
    )
    status = controller.transport.source_sync.status(0)
    assert status.state == "off"
    assert status.requested is False
    assert status.active is True
    assert status.effective is False
    assert status.policy_mode == "legacy"
    assert status.actual_realtime_bpm is False
    assert status.guidance is None
    assert status.detail is None
    for control in ("sync", "realtime_bpm"):
        global_status = controller.transport.timing_policy.status(control)
        assert global_status.effective is False
        assert global_status.pending is False
        assert global_status.detail is None
    assert controller.project.model_dump() == before


@pytest.mark.parametrize("disabled_by", ["global", "pad"])
def test_disabled_sync_tracks_legacy_adoption_until_actual_settlement(
    controller: AppController, audio_engine_mock: Mock, disabled_by: str
) -> None:
    policy = _disabled_sync_target(controller, audio_engine_mock, disabled_by=disabled_by)
    adoption = _adoption(
        target_mode="legacy",
        target_policy_revision=policy.revision,
        target_policy_sync=policy.sync,
        target_policy_realtime_bpm=policy.realtime_bpm,
        target_intent_revision=5,
    )
    feedback = _feedback(
        audio_engine_mock,
        active=True,
        effective=True,
        intent_revision=4,
        map_revision="a" * 64,
        policy_mode="mapped",
        policy_revision=1,
        policy_sync=True,
        policy_realtime_bpm=True,
        settled=True,
        adoption=adoption,
    )
    status = controller.transport.source_sync.status(0)
    assert status.state == "pending"
    assert status.guidance == "pending"
    assert status.policy_mode == "mapped"
    assert status.effective is True
    assert status.actual_realtime_bpm is True
    assert controller.transport.timing_policy.status("realtime_bpm").pending is True
    adoption.update(state="applied", execution_frame=256)
    feedback.update(
        effective=False,
        intent_revision=5,
        map_revision=None,
        policy_mode="legacy",
        policy_revision=policy.revision,
        policy_sync=policy.sync,
        settled=False,
        target_current=True,
    )
    status = controller.transport.source_sync.status(0)
    assert status.state == "pending"
    assert status.guidance == "settling"
    assert status.actual_realtime_bpm is None
    assert controller.transport.timing_policy.status("realtime_bpm").pending is True
    feedback["settled"] = True
    status = controller.transport.source_sync.status(0)
    assert status.state == "off"
    assert status.policy_mode == "legacy"
    assert status.active is True
    assert status.effective is False
    assert status.actual_realtime_bpm is False
    assert status.guidance is None
    assert controller.transport.timing_policy.status("sync").pending is False
    assert controller.transport.timing_policy.status("realtime_bpm").pending is False


def test_applied_attempt_does_not_describe_a_stopped_voice_as_settling(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    feedback, adoption = _pending_off_adoption(
        controller, audio_engine_mock, state="applied", execution_frame=123
    )
    controller.project.global_timing_policy = _policy(sync=True, realtime=False)
    adoption.update(target_mode="mapped", target_policy_sync=True)
    feedback.update(
        active=False,
        effective=False,
        intent_revision=None,
        policy_mode=None,
        policy_revision=None,
        policy_sync=None,
        policy_realtime_bpm=None,
        settled=None,
    )

    status = controller.transport.source_sync.status(0)

    assert status.state == "pending"
    assert status.guidance == "pending"
    assert status.active is False
    assert status.effective is False


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("target_mode", "fixed"),
        ("target_policy_revision", 1),
        ("target_policy_sync", True),
        ("target_policy_realtime_bpm", None),
        ("target_intent_revision", 1),
    ],
)
def test_stale_adoption_target_cannot_label_the_current_request_refused(
    controller: AppController, audio_engine_mock: Mock, field: str, value: object
) -> None:
    _, adoption = _pending_off_adoption(
        controller, audio_engine_mock, state="refused", reason="unsupported"
    )
    adoption[field] = value

    status = controller.transport.source_sync.status(0)

    assert status.state == "pending"
    assert status.guidance == "restart"
    assert status.effective is True
    assert "unsupported" not in (status.detail or "")


def test_equal_choices_after_away_back_require_the_new_raw_adoption_revision(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _, adoption = _pending_off_adoption(
        controller, audio_engine_mock, state="refused", reason="capacity"
    )
    controller.project.global_timing_policy = _policy(sync=False, realtime=False, revision=4)

    assert controller.transport.source_sync.status(0).guidance == "restart"
    adoption["target_policy_revision"] = 4
    assert controller.transport.source_sync.status(0).guidance == "refused"


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("snapshot_revision", True),
        ("attempt", -1),
        ("state", "queued"),
        ("state", "applied"),
        ("reason", "private-path/secret.wav"),
        ("reason", "busy"),
        ("execution_frame", True),
        ("execution_frame", 123),
        ("target_mode", ["legacy"]),
        ("target_policy_revision", True),
        ("target_policy_sync", 0),
        ("target_policy_realtime_bpm", 0),
        ("target_intent_revision", 1 << 64),
    ],
)
def test_malformed_adoption_never_becomes_audio_or_refusal_confirmation(
    controller: AppController, audio_engine_mock: Mock, field: str, value: object
) -> None:
    _, adoption = _pending_off_adoption(controller, audio_engine_mock)
    adoption[field] = value

    status = controller.transport.source_sync.status(0)

    assert status.state == "pending"
    assert status.guidance == "restart"
    assert status.effective is True


@pytest.mark.parametrize(
    "field",
    [
        "snapshot_revision",
        "attempt",
        "state",
        "reason",
        "execution_frame",
        "target_mode",
        "target_policy_revision",
        "target_policy_sync",
        "target_policy_realtime_bpm",
        "target_intent_revision",
    ],
)
def test_missing_adoption_field_cannot_default_into_a_matching_target(
    controller: AppController, audio_engine_mock: Mock, field: str
) -> None:
    _, adoption = _pending_off_adoption(
        controller, audio_engine_mock, state="refused", reason="unsupported"
    )
    del adoption[field]

    assert controller.transport.source_sync.status(0).guidance == "restart"


@pytest.mark.parametrize("missing", [False, True])
def test_absent_native_adoption_preserves_existing_start_guidance(
    controller: AppController, audio_engine_mock: Mock, *, missing: bool
) -> None:
    feedback, _ = _pending_off_adoption(controller, audio_engine_mock)
    if missing:
        del feedback["adoption"]
    else:
        feedback["adoption"] = None

    status = controller.transport.source_sync.status(0)

    assert status.state == "pending"
    assert status.guidance == "restart"
    assert status.effective is True


def test_global_choices_publish_one_revision_without_touching_pad_or_other_controls(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    before = controller.project.model_dump(exclude={"global_timing_policy"})
    controller.transport.timing_policy.set_enabled("sync", enabled=True)
    first = controller.project.global_timing_policy
    assert first.sync is True
    assert first.realtime_bpm is None
    assert first.revision == 1
    assert controller.project.model_dump(exclude={"global_timing_policy"}) == before
    kwargs = audio_engine_mock.set_input_runtime_state.call_args.kwargs
    assert json.loads(kwargs["timing_policy_json"]) == first.model_dump() | {"realtime_bpm": True}
    assert kwargs["source_map_rows"][0]["sync_requested"] is True
    assert kwargs["source_map_rows"][0]["intent_revision"] == 0
    controller.transport.timing_policy.set_enabled("realtime_bpm", enabled=False)
    assert controller.project.global_timing_policy == _policy(sync=False, realtime=None)
    assert controller.project.model_dump(exclude={"global_timing_policy"}) == before


def test_inherited_pad_off_creates_explicit_preference_and_global_mask_does_not_change_toggle(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.transport.timing_policy.set_enabled("sync", enabled=True)
    controller.transport.source_sync.toggle(0)
    assert controller.project.pad_source_sync_intent[0] == PadSourceSyncIntent(
        requested=False, revision=1
    )
    controller.transport.timing_policy.set_enabled("sync", enabled=False)
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=4)
    with pytest.raises(ValueError, match="global SYNC"):
        controller.transport.source_sync.toggle(0)
    assert controller.project.pad_source_sync_intent[0] == PadSourceSyncIntent(
        requested=True, revision=4
    )
    assert controller.project.global_timing_policy.sync is False


@pytest.mark.parametrize(
    ("sync", "realtime", "mode"),
    [
        (False, False, "legacy"),
        (False, True, "legacy"),
        (True, False, "mapped"),
        (True, True, "mapped"),
    ],
)
def test_all_explicit_choices_use_same_resolved_runtime_mode_without_control_mutation(
    controller: AppController,
    audio_engine_mock: Mock,
    *,
    sync: bool,
    realtime: bool,
    mode: str,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.global_timing_policy = _policy(sync=sync, realtime=realtime)
    before = controller.project.model_dump()
    assert controller.transport.source_sync.requested_mode(0) == mode
    assert controller.input_mapping._refresh_rust_runtime_state()
    row = audio_engine_mock.set_input_runtime_state.call_args.kwargs["source_map_rows"][0]
    assert row["sync_requested"] is sync
    assert row["intent_revision"] == 0
    assert row["delivery"] is None
    assert controller.project.model_dump() == before


def test_realtime_choice_alone_keeps_unsynchronized_legacy_preference(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.global_timing_policy = _policy(sync=None, realtime=False)
    assert controller.transport.source_sync.requested_mode(0) == "legacy"
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=0)
    assert controller.transport.source_sync.requested_mode(0) == "mapped"


def _settled_mode_feedback(
    controller: AppController,
    audio: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    realtime: bool,
) -> dict[str, object]:
    _assigned(controller, audio)
    controller.project.global_timing_policy = _policy(sync=True, realtime=realtime)
    geometry = _geometry()
    monkeypatch.setattr(controller.loader.source_beatmaps, "effective", Mock(return_value=geometry))
    return _feedback(
        audio,
        active=True,
        effective=True,
        intent_revision=0,
        map_revision=geometry.revision(),
        policy_mode="mapped",
        policy_revision=2,
        policy_sync=True,
        policy_realtime_bpm=True,
        settled=True,
        target_current=True,
    )


@pytest.mark.parametrize("realtime", [False, True])
def test_settled_matching_rt_does_not_inherit_sync_phase_only_pending(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    realtime: bool,
) -> None:
    _settled_mode_feedback(controller, audio_engine_mock, monkeypatch, realtime=realtime)
    state = controller.transport.source_sync.status(0)
    assert state.state == "pending"
    assert state.guidance == "phase"
    assert state.actual_realtime_bpm is True
    assert state.grid_phase_confirmed is None
    sync = controller.transport.timing_policy.status("sync")
    assert sync.pending is True
    assert sync.detail == "Timing mode applied; grid phase is unconfirmed."
    assert sync.grid_phase_confirmed is None
    rt = controller.transport.timing_policy.status("realtime_bpm")
    assert rt.requested is True
    assert rt.effective is True
    assert rt.pending is True
    assert rt.detail == sync.detail
    assert rt.grid_phase_confirmed is None


@pytest.mark.parametrize("realtime", [False, True])
@pytest.mark.parametrize("change", ["policy_revision", "settled", "source_generation", "refused"])
def test_rt_preserves_actual_pending_refusal_and_source_policy_mismatches(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    change: str,
    *,
    realtime: bool,
) -> None:
    feedback = _settled_mode_feedback(controller, audio_engine_mock, monkeypatch, realtime=realtime)
    if change == "refused":
        feedback.update(
            intent_revision=1,
            adoption=_adoption(
                state="refused",
                reason="capacity",
                target_mode="mapped",
                target_policy_revision=2,
                target_policy_sync=True,
                target_policy_realtime_bpm=True,
            ),
        )
    elif change == "settled":
        feedback.update(settled=False, effective=False)
    else:
        feedback[change] = 2 if change == "source_generation" else 1
    state = controller.transport.source_sync.status(0)
    assert state.state in {"pending", "refused", "unconfirmed"}
    assert state.guidance != "phase"
    rt = controller.transport.timing_policy.status("realtime_bpm")
    assert rt.pending is True
    assert rt.detail == state.guidance_text
    if change == "policy_revision":
        assert state.guidance == "restart"
        assert rt.effective is True
    elif change == "refused":
        assert state.guidance == "refused"
        assert rt.effective is True
    else:
        assert rt.effective is (False if change == "settled" else None)


def test_phase_guidance_with_unknown_actual_rt_keeps_rt_unconfirmed_pending(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=1)
    geometry = _geometry()
    monkeypatch.setattr(controller.loader.source_beatmaps, "effective", Mock(return_value=geometry))
    _feedback(
        audio_engine_mock,
        active=True,
        effective=True,
        intent_revision=1,
        map_revision=geometry.revision(),
    )
    state = controller.transport.source_sync.status(0)
    assert state.guidance == "phase"
    assert state.actual_realtime_bpm is True
    rt = controller.transport.timing_policy.status("realtime_bpm")
    assert rt.effective is True
    assert rt.pending is True
    assert rt.detail == state.guidance_text


def test_fixed_actual_settlement_needs_no_map_and_cannot_borrow_old_global_ack(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.global_timing_policy = _policy(sync=True, realtime=False)
    monkeypatch.setattr(controller.loader.source_beatmaps, "effective", lambda _id: None)
    feedback = _feedback(
        audio_engine_mock,
        active=True,
        effective=True,
        intent_revision=0,
        map_revision=None,
        policy_mode="fixed",
        policy_revision=2,
        settled=True,
        policy_sync=True,
        policy_realtime_bpm=False,
    )
    assert controller.transport.source_sync.status(0).state == "pending"
    assert controller.transport.source_sync.status(0).grid_phase_confirmed is None
    feedback.update(policy_revision=1)
    assert controller.transport.source_sync.status(0).state == "pending"
    feedback.update(policy_revision=2, policy_sync=False)
    assert controller.transport.source_sync.status(0).state == "pending"
    feedback.update(policy_sync=True, effective=False, settled=False)
    assert controller.transport.source_sync.status(0).state == "pending"


def test_global_off_ack_uses_full_actual_policy_while_retaining_old_active_audio(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.global_timing_policy = _policy(sync=False, realtime=False, revision=0)
    feedback = _feedback(
        audio_engine_mock,
        active=True,
        effective=False,
        intent_revision=0,
        map_revision=None,
        policy_mode="legacy",
        policy_revision=0,
        settled=True,
        policy_sync=None,
        policy_realtime_bpm=None,
    )
    assert controller.transport.source_sync.status(0).state == "pending"
    feedback.update(policy_sync=False, policy_realtime_bpm=True)
    assert controller.transport.source_sync.status(0).state == "off"
    feedback.update(policy_mode="mapped", effective=True, map_revision="a" * 64)
    state = controller.transport.source_sync.status(0)
    assert state.state == "pending"
    assert state.effective is True


@pytest.mark.parametrize("sync", [True, False])
@pytest.mark.parametrize("active", [True, False])
def test_fixed_or_legacy_preparation_refusal_is_diagnosed_without_changing_actual_audio(
    controller: AppController,
    audio_engine_mock: Mock,
    *,
    sync: bool,
    active: bool,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.global_timing_policy = _policy(sync=sync, realtime=False)
    diagnostics = Mock()
    controller.transport.source_sync.bind_diagnostics(diagnostics)
    feedback = _feedback(
        audio_engine_mock,
        active=active,
        effective=active,
        intent_revision=0,
        map_revision="a" * 64 if active else None,
        policy_mode="mapped" if active else None,
        policy_revision=0,
        policy_sync=None,
        policy_realtime_bpm=None,
        settled=active,
    )
    reason = (
        "source preparation requires an exact finite supported tempo ratio"
        if sync
        else "captured pad start preparation unavailable"
    )
    controller.transport.residency.report_launch_error(0, reason)

    status = controller.transport.source_sync.status(0)

    assert status.state == "refused"
    assert reason in status.detail
    assert status.requested is sync
    assert status.active is active
    assert status.effective is active
    assert status.policy_mode == ("mapped" if active else None)
    assert status.actual_realtime_bpm is (True if active else None)
    diagnostics.observe.assert_called_once_with("source_sync", status.detail, pad_id=0, request=0)

    # Historical Fixed audio cannot acknowledge the requested Mapped settlement.
    feedback.update(
        active=True,
        effective=sync,
        map_revision=None,
        policy_mode="fixed" if sync else "legacy",
        policy_revision=2,
        policy_sync=sync,
        policy_realtime_bpm=True,
        settled=True,
    )
    assert controller.transport.source_sync.status(0).state == ("refused" if sync else "off")


def test_changed_global_policy_waits_for_whole_reserved_cohort_then_retries(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    assert controller.input_mapping._refresh_rust_runtime_state()
    previous = controller.input_mapping._last_input_runtime_state_signature
    controller.transport._pair_deferred_sample_ids = frozenset({0})
    audio_engine_mock.reset_mock()
    with pytest.raises(RuntimeError, match="not ready"):
        controller.transport.timing_policy.set_enabled("sync", enabled=False)
    assert controller.project.global_timing_policy.sync is False
    assert controller.input_mapping._last_input_runtime_state_signature is previous
    audio_engine_mock.set_input_runtime_state.assert_not_called()
    controller.transport._pair_deferred_sample_ids = frozenset()
    assert controller.input_mapping._refresh_rust_runtime_state()
    assert (
        json.loads(
            audio_engine_mock.set_input_runtime_state.call_args.kwargs["timing_policy_json"]
        )["sync"]
        is False
    )


@pytest.mark.parametrize(
    ("method", "action", "control"),
    [
        ("toggle_source_sync", global_source_sync_toggle_action(), "sync"),
        ("toggle_realtime_bpm", global_realtime_bpm_toggle_action(), "realtime_bpm"),
    ],
)
def test_gui_learn_and_midi_share_each_global_action_without_early_mutation(
    controller: AppController,
    audio_engine_mock: Mock,
    method: str,
    action: LooperAction,
    control: str,
) -> None:
    _assigned(controller, audio_engine_mock)
    if control == "realtime_bpm":
        controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(
            requested=True, revision=0
        )
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)
    request = Mock(return_value=None)
    audio_engine_mock.request_input_runtime_policy_adoption = request
    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
    })
    getattr(ctx.audio.global_, method)()
    assert controller.project.global_timing_policy.is_neutral
    assert load_midi_mapping_file().mappings[0].action == action
    request.assert_not_called()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": action.key,
    })
    assert controller.project.global_timing_policy.sync is (control == "sync")
    assert controller.project.global_timing_policy.realtime_bpm is None
    request.assert_called_once_with(service=True)
    getattr(ctx.audio.global_, method)()
    assert controller.project.global_timing_policy.revision == 2
    assert request.call_count == 2


def test_fixed_auto_loop_uses_initial_period_and_retains_map_without_variable_marker_actions(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.manual_bpm[0] = 120.0
    controller.project.sample_durations[0] = 10.0
    controller.project.pad_loop_start_s[0] = 1.0
    controller.project.pad_loop_bars[0] = 1.0
    controller.project.pad_loop_auto[0] = True
    selection = _bind_map(controller, _geometry())
    controller.project.global_timing_policy = _policy(sync=False, realtime=False)
    assert controller.transport.loop.requested_region(0) == (1.0, 3.0)
    with pytest.raises(ValueError, match="Variable source timing is unavailable"):
        controller.transport.loop.apply_source_map_bars(0)
    assert selection.is_selected(0)
    assert controller.project.pad_loop_auto[0] is True
    controller.project.global_timing_policy = _policy(sync=False, realtime=True, revision=3)
    assert controller.transport.loop.requested_region(0) == (1.0, 3.0)
    selection.effective.return_value = None
    controller.project.pad_loop_end_s[0] = 3.5
    assert controller.transport.loop.requested_region(0) == (1.0, 3.0)
    assert controller.project.pad_loop_auto[0] is True
    controller.project.global_timing_policy = _policy(sync=True, realtime=True, revision=4)
    with pytest.raises(ValueError, match="qualified source map"):
        controller.transport.loop.requested_region(0)
