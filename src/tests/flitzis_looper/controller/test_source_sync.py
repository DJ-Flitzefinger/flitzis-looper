"""Control publication and callback-feedback routing; Native supplies separate audio proof."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.control_diagnostics import ConsoleDiagnostics
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.input_mapping.actions import source_sync_toggle_action
from flitzis_looper.input_mapping.storage import load_midi_mapping_file
from flitzis_looper.models import PadContentIdentity
from flitzis_looper.source_sync_intent import PadSourceSyncIntent, neutral_source_sync_intent
from flitzis_looper.timing_policy import GlobalTimingPolicy
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.test_source_beatmap_selection import _assigned, _event, _job
from tests.flitzis_looper.controller.transport.test_source_map_loop import _geometry

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def _birth(**changes: object) -> dict[str, object]:
    value: dict[str, object] = {
        "requested": "first_sound_at_grid",
        "outcome": "complete_beginning_at_e",
        "alignment": "earliest_bound_met",
        "reason": None,
        "original_target_frame": 64,
        "execution_frame": 8192,
        "reference_qualified_at_birth": True,
        "emitted": True,
    }
    value.update(changes)
    return value


@pytest.mark.parametrize("attack", [False, True])
def test_birth_feedback_keeps_requested_alignment_separate_from_grid_phase(
    controller: AppController, audio_engine_mock: Mock, *, attack: bool
) -> None:
    audio = audio_engine_mock
    _assigned(controller, audio)
    value = _birth(
        **(
            {
                "requested": "attack_at_grid",
                "alignment": "pending",
                "reason": "no_qualified_anchor",
            }
            if attack
            else {}
        )
    )
    _feedback(audio, active=True, birth=value)
    preferences = controller.project.model_dump()

    status = controller.transport.source_sync.status(0)

    assert status.birth is not None
    assert status.birth.original_target_frame == 64
    assert status.birth.execution_frame == 8192
    assert status.birth.emitted is True
    assert status.birth.reference_qualified_at_birth is True
    assert status.birth.requested == ("attack_at_grid" if attack else "first_sound_at_grid")
    assert status.birth.alignment == ("pending" if attack else "earliest_bound_met")
    assert status.grid_phase_confirmed is None
    if attack:
        assert status.guidance_text == (
            "Attack alignment is pending; playback preserves the complete beginning."
        )
    assert controller.project.model_dump() == preferences


@pytest.mark.parametrize(
    "change",
    [
        {"original_target_frame": True},
        {"execution_frame": 1 << 64},
        {"execution_frame": 63},
        {"emitted": 1},
        {"emitted": False},
        {"reference_qualified_at_birth": 1},
        {"requested": "attack_at_grid"},
        {"outcome": "mixed"},
        {"requested": []},
        {"outcome": {}},
        {"alignment": []},
        {"reason": {}},
        {"extra": "unknown"},
    ],
)
def test_malformed_birth_feedback_cannot_retain_previous_positive(
    controller: AppController, audio_engine_mock: Mock, change: dict[str, object]
) -> None:
    audio = audio_engine_mock
    _assigned(controller, audio)
    feedback = _feedback(audio, active=True, birth=_birth())
    assert controller.transport.source_sync.status(0).birth is not None
    feedback["birth"] = _birth(**change)

    status = controller.transport.source_sync.status(0)

    assert status.state == "unconfirmed"
    assert status.birth is None
    assert status.grid_phase_confirmed is None


def test_birth_feedback_old_native_missing_mixed_stop_and_source_change_are_conservative(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _assigned(controller, audio)
    feedback = _feedback(audio, active=True)
    assert controller.transport.source_sync.status(0).birth is None
    feedback["birth"] = _birth(
        requested="mixed",
        outcome="mixed",
        alignment="mixed",
        reason="mixed",
        original_target_frame=None,
        execution_frame=None,
        reference_qualified_at_birth=False,
        emitted=False,
    )
    status = controller.transport.source_sync.status(0)
    assert status.birth is not None
    assert status.birth.emitted is False
    assert status.birth.execution_frame is None
    assert status.guidance_text == "Start alignment differs between active voices."
    feedback["birth"] = _birth()
    feedback["source_generation"] = 2
    assert controller.transport.source_sync.status(0).birth is None
    feedback["source_generation"] = 1
    feedback["active"] = False
    assert controller.transport.source_sync.status(0).birth is None
    feedback["birth"] = None
    assert controller.transport.source_sync.status(0).birth is None


@pytest.fixture(autouse=True)
def available_timing_parents(controller: AppController) -> None:
    """These mode/adoption tests explicitly configure their requested parents."""
    controller.project.multi_loop = True
    controller.project.bpm_lock = True


def _feedback(audio: Mock, **changes: object) -> dict[str, object]:
    binding = audio.current_input_runtime_pad_binding(0)
    binding._metadata["window_revision"] = 7
    audio.current_input_runtime_pad_binding.side_effect = None
    audio.current_input_runtime_pad_binding.return_value = binding
    feedback: dict[str, object] = {
        "source_generation": 1,
        # Actual source_id metadata is a string; this is a separate Native address.
        "source_identity": 1234,
        "window_revision": 7,
        "active": False,
        "effective": False,
        "intent_revision": None,
        "map_revision": None,
    }
    feedback.update(changes)
    audio.pad_source_sync_status = Mock(return_value=feedback)
    return feedback


def _adoption(**changes: object) -> dict[str, object]:
    """One Native-qualified observation; these tests do not manufacture an audio ACK."""
    observation: dict[str, object] = {
        "snapshot_revision": 11,
        "attempt": 3,
        "state": "pending",
        "reason": None,
        "execution_frame": None,
        "target_mode": "legacy",
        "target_policy_revision": None,
        "target_policy_sync": None,
        "target_policy_realtime_bpm": None,
        "target_intent_revision": 0,
    }
    observation.update(changes)
    return observation


def _mapped_request_with_old_audio(
    controller: AppController, audio: Mock, *, actual_mode: str = "legacy", state: str = "refused"
) -> dict[str, object]:
    """Retain qualified preceding audio independently of a newer mapped request."""
    _assigned(controller, audio)
    controller.project.global_timing_policy = GlobalTimingPolicy(
        schema_version=1, sync=True, realtime_bpm=True, revision=2
    )
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=5)
    return _feedback(
        audio,
        active=True,
        effective=actual_mode != "legacy",
        intent_revision=4,
        map_revision="f" * 64 if actual_mode == "mapped" else None,
        policy_mode=actual_mode,
        policy_revision=1,
        policy_sync=actual_mode != "legacy",
        policy_realtime_bpm=actual_mode == "mapped",
        settled=True,
        adoption=_adoption(
            state=state,
            reason="unsupported" if state == "refused" else None,
            execution_frame=123 if state == "applied" else None,
            target_mode="mapped",
            target_policy_revision=2,
            target_policy_sync=True,
            target_policy_realtime_bpm=True,
            target_intent_revision=5,
        ),
    )


@pytest.mark.parametrize("actual_mode", ["legacy", "fixed", "mapped"])
@pytest.mark.parametrize("state", ["pending", "applied", "refused"])
@pytest.mark.parametrize("map_state", ["absent", "preparing", "failed", "changed", "unreadable"])
def test_current_map_guidance_precedes_generic_adoption_without_hiding_actual_audio(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    actual_mode: str,
    state: str,
    map_state: str,
) -> None:
    audio = audio_engine_mock
    _mapped_request_with_old_audio(controller, audio, actual_mode=actual_mode, state=state)
    maps = controller.loader.source_beatmaps
    if map_state != "absent":
        job = _job(audio)
        maps.select(0, "current original record")
        if map_state in {"changed", "unreadable"}:
            maps.completed(0, _event(job))
            audio._resolve_loader_event.side_effect = (
                RuntimeError("placement unreadable") if map_state == "unreadable" else None
            )
            audio._resolve_loader_event.return_value = None
        elif map_state == "failed":
            job.source_beatmap_async_status.return_value = {
                "terminal": True,
                "stage": "failed",
                "detail": "Original map coverage rejected.",
            }
            maps.poll(frozenset())
    effective = Mock(wraps=maps.effective)
    monkeypatch.setattr(maps, "effective", effective)
    monkeypatch.setattr(
        maps, "status", Mock(side_effect=AssertionError("A second readiness query is forbidden."))
    )
    before = controller.project.model_dump(), controller.session.model_dump()
    audio.reset_mock()

    status = controller.transport.source_sync.status(0)

    readiness = (
        "preparing"
        if map_state == "preparing"
        else "failed"
        if map_state == "failed"
        else "unavailable"
    )
    refused = state == "refused" or map_state == "failed"
    assert status.state == ("refused" if refused else "pending")
    assert status.requested is True
    assert status.active is True
    assert status.effective is (actual_mode != "legacy")
    assert status.policy_mode == actual_mode
    assert status.actual_realtime_bpm is (actual_mode == "mapped")
    assert status.grid_phase_confirmed is None
    assert status.map_readiness == readiness
    assert status.guidance == (
        "refused" if refused else "settling" if state == "applied" else "pending"
    )
    assert status.guidance_text is not None
    assert "map" in status.guidance_text
    assert "Live timing change refused" not in status.guidance_text
    assert status.detail is not None
    if state == "refused":
        assert "unsupported" in status.detail
        assert "snapshot 11, attempt 3" in status.detail
    if map_state == "failed":
        assert "Original map coverage rejected." in status.detail
    effective.assert_called_once_with(0)
    assert (controller.project.model_dump(), controller.session.model_dump()) == before
    audio.set_input_runtime_state.assert_not_called()
    audio.capture_pad_start.assert_not_called()
    audio.set_pad_loop_region.assert_not_called()
    global_status = controller.transport.timing_policy.status("realtime_bpm")
    assert global_status.effective is (actual_mode == "mapped")
    assert global_status.pending is True
    assert global_status.detail == status.guidance_text


@pytest.mark.parametrize(
    "failure",
    [
        "assignment",
        "binding",
        "superseded",
        "unreadable",
        "generation",
        "window",
        "schema",
        "policy",
    ],
)
def test_map_observation_follows_complete_original_source_and_feedback_qualification(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    failure: str,
) -> None:
    audio = audio_engine_mock
    feedback = _mapped_request_with_old_audio(controller, audio)
    observe_map = Mock(side_effect=AssertionError("Unqualified feedback must not observe a map."))
    monkeypatch.setattr(controller.loader.source_beatmaps, "_observe_state", observe_map)
    if failure == "assignment":
        monkeypatch.setattr(
            controller.transport,
            "ensure_current_source_assignment_ready",
            Mock(side_effect=ValueError("assignment pending")),
        )
    elif failure == "binding":
        audio.current_input_runtime_pad_binding.return_value = None
    elif failure == "superseded":
        binding = audio.current_input_runtime_pad_binding.return_value
        monkeypatch.setattr(binding, "is_current", Mock(side_effect=[True, False]))
    elif failure == "unreadable":
        audio.pad_source_sync_status.side_effect = RuntimeError("feedback unreadable")
    elif failure == "generation":
        feedback["source_generation"] = 2
    elif failure == "window":
        feedback["window_revision"] = 8
    elif failure == "schema":
        feedback["active"] = 1
    else:
        feedback["policy_revision"] = -1

    status = controller.transport.source_sync.status(0)

    assert status.state == "unconfirmed"
    assert status.active is status.effective is status.actual_realtime_bpm is None
    assert status.map_readiness is None
    assert status.guidance == "unconfirmed"
    observe_map.assert_not_called()


def test_ready_unapplied_map_status_reuses_the_first_geometry_for_original_loop_guards(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    audio = audio_engine_mock
    feedback = _mapped_request_with_old_audio(controller, audio)
    del feedback["adoption"]
    project = controller.project
    project.manual_bpm[0] = 120.0
    project.pad_loop_start_s[0] = 1.0
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 1.0
    geometry = _geometry()
    effective = Mock(side_effect=[geometry, AssertionError("Unexpected second geometry capture.")])
    monkeypatch.setattr(controller.loader.source_beatmaps, "effective", effective)
    loop = controller.transport.loop
    mapped_region = Mock(wraps=loop._mapped_region)
    monkeypatch.setattr(loop, "_mapped_region", mapped_region)
    before = project.model_dump(), controller.session.model_dump()
    audio.reset_mock()

    status = controller.transport.source_sync.status(0)

    assert status.state == "pending"
    assert status.active is True
    assert status.effective is False
    assert status.actual_realtime_bpm is False
    assert status.guidance == "restart"
    assert status.map_readiness is None
    assert status.grid_phase_confirmed is None
    effective.assert_called_once_with(0)
    mapped_region.assert_called_once_with(geometry, start=1.0, bars=1.0)
    assert loop._source_sync_requested_region(0, geometry) == (1.0, 3.5)
    assert (project.model_dump(), controller.session.model_dump()) == before
    audio.set_input_runtime_state.assert_not_called()
    audio.capture_pad_start.assert_not_called()
    audio.set_pad_loop_region.assert_not_called()


@pytest.mark.parametrize("invalid", ["uncertain", "gap_unknown_count", "meter", "extent"])
def test_ready_map_status_retains_loop_coverage_refusal_without_recapturing_geometry(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    invalid: str,
) -> None:
    audio = audio_engine_mock
    feedback = _mapped_request_with_old_audio(controller, audio)
    del feedback["adoption"]
    project = controller.project
    project.pad_loop_start_s[0] = 1.0
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 3.0 if invalid == "extent" else 1.0 if invalid == "meter" else 2.0
    geometry = _geometry(
        middle_coverage=invalid if invalid in {"uncertain", "gap_unknown_count"} else "trusted",
        meter="unknown" if invalid == "meter" else "quarter_note_4_4_verified",
    )
    effective = Mock(side_effect=[geometry, AssertionError("Unexpected second geometry capture.")])
    monkeypatch.setattr(controller.loader.source_beatmaps, "effective", effective)
    before = project.model_dump(), controller.session.model_dump()
    audio.reset_mock()

    status = controller.transport.source_sync.status(0)

    assert status.state == "refused"
    assert status.active is True
    assert status.effective is False
    assert status.actual_realtime_bpm is False
    assert status.map_readiness is None
    assert status.grid_phase_confirmed is None
    assert status.detail is not None
    assert "supported" in status.detail
    effective.assert_called_once_with(0)
    assert (project.model_dump(), controller.session.model_dump()) == before
    audio.set_input_runtime_state.assert_not_called()
    audio.capture_pad_start.assert_not_called()
    audio.set_pad_loop_region.assert_not_called()


def test_missing_map_allows_pending_ordinary_play_and_current_delivery_allows_mapped_retrigger(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _mapped_request_with_old_audio(controller, audio)
    project = controller.project
    project.manual_bpm[0] = 120.0
    project.sample_durations[0] = 10.0
    project.pad_loop_start_s[0] = 1.0
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 1.0
    status = controller.transport.source_sync.status(0)
    assert status.map_readiness == "unavailable"
    audio.capture_pad_start.reset_mock()

    controller.transport.playback.trigger_pad(0)
    audio.capture_pad_start.assert_called_once_with(
        0, 1.0, 3.0, 1.0, exclusive=False, received_at_ns=None, expected_source_generation=1
    )
    row = audio.set_input_runtime_state.call_args.kwargs["source_map_rows"][0]
    assert row["ordinary_pending"] is True
    assert row["delivery"] is None
    assert row["sync_requested"] is True
    with pytest.raises(ValueError, match="Variable SYNC requires a qualified source map"):
        controller.transport.loop.requested_region(0)
    audio.capture_pad_start.reset_mock()
    maps = controller.loader.source_beatmaps
    job = _job(audio)
    maps.select(0, "current original record")
    event = _event(job)
    job.resolve_source_beatmap_artifact_completion.return_value.map.return_value = _geometry()
    maps.completed(0, event)
    assert maps.playback_delivery(0) is event["_native_event_owner"]

    ready = controller.transport.source_sync.status(0)

    assert ready.map_readiness is None
    assert ready.state == "refused"
    assert ready.effective is False
    assert ready.grid_phase_confirmed is None
    assert "unsupported" in ready.detail
    controller.transport.playback.trigger_pad(0)

    audio.capture_pad_start.assert_called_once_with(
        0, 1.0, 3.5, 1.0, exclusive=False, received_at_ns=None, expected_source_generation=1
    )
    assert controller.transport.source_sync.status(0).grid_phase_confirmed is None


_IDLE_POLICIES = (
    pytest.param(
        GlobalTimingPolicy(schema_version=1, sync=None, realtime_bpm=None, revision=0),
        id="neutral-default",
    ),
    pytest.param(
        GlobalTimingPolicy(schema_version=1, sync=False, realtime_bpm=True, revision=2),
        id="sync-off-realtime-on",
    ),
)


@pytest.mark.parametrize("action", ["pad", "global"])
def test_no_map_launch_uses_scalar_region_without_changing_saved_request_or_markers(
    controller: AppController, audio_engine_mock: Mock, action: str
) -> None:
    audio = audio_engine_mock
    _assigned(controller, audio)
    project = controller.project
    project.manual_bpm[0] = 120.0
    project.sample_durations[0] = 10.0
    project.pad_loop_start_s[0] = 1.0
    project.pad_loop_bars[0] = 1.0
    project.pad_loop_auto[0] = True
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=3)
    before = project.model_dump()
    if action == "pad":
        controller.transport.playback.trigger_pad(0)
        assert audio.capture_pad_start.call_args.args[:3] == (0, 1.0, 3.0)
    else:
        controller.session.active_sample_ids = {0}
        controller.transport.playback.start_or_restart_global_start_stop()
        audio.capture_global_start.assert_called_once()
        entry = controller.transport.capture_global_start_entry(0)
        assert entry is not None
        assert entry[1:3] == (1.0, 3.0)
    row = audio.set_input_runtime_state.call_args.kwargs["source_map_rows"][0]
    assert row["ordinary_pending"] is True
    assert row["sync_requested"] is True
    assert row["delivery"] is None
    assert project.model_dump() == before


@pytest.mark.parametrize("condition", ["withheld", "invalid", "unready"])
def test_pending_launch_permission_requires_absent_geometry_valid_region_and_current_binding(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    condition: str,
) -> None:
    audio = audio_engine_mock
    _assigned(controller, audio)
    project = controller.project
    project.manual_bpm[0] = 120.0
    project.pad_loop_start_s[0] = 1.0
    project.pad_loop_bars[0] = 1.0
    project.pad_loop_auto[0] = True
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=3)
    geometry = _geometry(middle_coverage="uncertain") if condition == "invalid" else _geometry()
    projection = Mock(return_value=(None, None) if condition == "unready" else (geometry, None))
    monkeypatch.setattr(controller.loader.source_beatmaps, "_playback_projection", projection)
    if condition == "unready":
        audio.current_input_runtime_pad_binding.return_value = None
        audio.current_input_runtime_pad_binding.side_effect = None
    assert controller.input_mapping._refresh_rust_runtime_state()
    row = audio.set_input_runtime_state.call_args.kwargs["source_map_rows"][0]
    assert row["ordinary_pending"] is False
    assert row["delivery"] is None
    assert row["sync_requested"] is True
    projection.assert_called_once_with(0)


def test_launch_region_captures_selector_geometry_and_delivery_once(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=1)
    maps = controller.loader.source_beatmaps
    effective = Mock(return_value=None)
    monkeypatch.setattr(maps, "effective", effective)
    launch = controller.transport.loop._capture_launch_region(0)
    assert launch.ordinary_pending is True
    assert launch.delivery is None
    effective.assert_called_once_with(0)


def _inactive_policy_refusal(
    controller: AppController,
    audio: Mock,
    policy: GlobalTimingPolicy,
    *,
    reason: str = "no_active_voices",
) -> tuple[dict[str, object], dict[str, object]]:
    """Qualified stopped callback fields remain separate from the complete raw target."""
    _assigned(controller, audio)
    controller.project.global_timing_policy = policy
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=False, revision=0)
    adoption = _adoption(
        state="refused",
        reason=reason,
        target_mode=controller.transport.source_sync.requested_mode(0),
        target_policy_revision=None if policy.is_neutral else policy.revision,
        target_policy_sync=policy.sync,
        target_policy_realtime_bpm=policy.realtime_bpm,
    )
    feedback = _feedback(
        audio,
        active=False,
        effective=False,
        intent_revision=None,
        map_revision=None,
        policy_mode=None,
        policy_revision=None,
        policy_sync=None,
        policy_realtime_bpm=None,
        settled=None,
        adoption=adoption,
    )
    return feedback, adoption


@pytest.mark.parametrize("policy", _IDLE_POLICIES)
def test_inactive_no_active_voice_attempt_stays_off_and_quiet_across_pad_and_global_polls(
    controller: AppController,
    audio_engine_mock: Mock,
    capsys: pytest.CaptureFixture[str],
    policy: GlobalTimingPolicy,
) -> None:
    feedback, adoption = _inactive_policy_refusal(controller, audio_engine_mock, policy)
    controller.transport.source_sync.bind_diagnostics(ConsoleDiagnostics())
    before = controller.project.model_dump()
    capsys.readouterr()

    assert adoption["target_mode"] == "legacy"
    assert adoption["target_policy_revision"] == (None if policy.is_neutral else 2)
    assert adoption["target_policy_sync"] is policy.sync
    assert adoption["target_policy_realtime_bpm"] is policy.realtime_bpm
    assert adoption["target_intent_revision"] == 0
    for _ in range(4):
        status = controller.transport.source_sync.status(0)
        assert status.state == "off"
        assert status.requested is False
        assert status.active is False
        assert status.effective is False
        assert status.policy_mode is None
        assert status.actual_realtime_bpm is None
        assert status.guidance is None
        assert status.detail is None
        sync = controller.transport.timing_policy.status("sync")
        realtime = controller.transport.timing_policy.status("realtime_bpm")
        assert sync.requested is policy.sync
        assert realtime.requested is policy.realtime_bpm
        assert sync.preference is False
        assert realtime.preference is True
        assert sync.effective is False
        assert realtime.effective is None
        assert sync.pending is realtime.pending is False
        assert sync.detail is realtime.detail is None

    assert not capsys.readouterr().err
    assert adoption["state"] == "refused"
    assert adoption["reason"] == "no_active_voices"
    assert adoption["attempt"] == 3
    assert adoption["execution_frame"] is None
    assert feedback["intent_revision"] is None
    assert feedback["settled"] is None
    assert controller.project.model_dump() == before


@pytest.mark.parametrize("policy", _IDLE_POLICIES)
@pytest.mark.parametrize("reason", ["missing_master", "unsupported"])
def test_inactive_real_live_timing_refusals_remain_visible(
    controller: AppController,
    audio_engine_mock: Mock,
    capsys: pytest.CaptureFixture[str],
    policy: GlobalTimingPolicy,
    reason: str,
) -> None:
    _inactive_policy_refusal(controller, audio_engine_mock, policy, reason=reason)
    controller.transport.source_sync.bind_diagnostics(ConsoleDiagnostics())
    capsys.readouterr()

    status = controller.transport.source_sync.status(0)

    assert status.state == "refused"
    assert status.guidance == "refused"
    assert status.active is False
    assert status.effective is False
    assert status.detail is not None
    assert reason in status.detail
    assert controller.transport.timing_policy.status("sync").pending is True
    assert controller.transport.timing_policy.status("realtime_bpm").pending is True
    diagnostic = capsys.readouterr().err
    assert "operation=source_sync" in diagnostic
    assert reason in diagnostic


@pytest.mark.parametrize("policy", _IDLE_POLICIES)
def test_active_no_active_voice_refusal_is_not_projected_as_idle(
    controller: AppController, audio_engine_mock: Mock, policy: GlobalTimingPolicy
) -> None:
    feedback, _ = _inactive_policy_refusal(controller, audio_engine_mock, policy)
    feedback["active"] = True

    status = controller.transport.source_sync.status(0)

    assert status.state == "refused"
    assert status.guidance == "refused"
    assert status.active is True
    assert status.effective is False
    assert status.detail is not None
    assert "no_active_voices" in status.detail


def test_requested_fixed_idle_attempt_is_stopped_without_an_applied_acknowledgement(
    controller: AppController, audio_engine_mock: Mock, capsys: pytest.CaptureFixture[str]
) -> None:
    policy = GlobalTimingPolicy(schema_version=1, sync=True, realtime_bpm=False, revision=2)
    feedback, adoption = _inactive_policy_refusal(controller, audio_engine_mock, policy)
    controller.transport.source_sync.bind_diagnostics(ConsoleDiagnostics())
    capsys.readouterr()

    status = controller.transport.source_sync.status(0)

    assert adoption["target_mode"] == "fixed"
    assert status.state == "stopped"
    assert status.requested is True
    assert status.active is False
    assert status.effective is False
    assert status.policy_mode is None
    assert status.actual_realtime_bpm is None
    assert status.guidance == "restart"
    assert controller.transport.timing_policy.status("sync").pending is False
    assert controller.transport.timing_policy.status("realtime_bpm").pending is False
    assert adoption["state"] == "refused"
    assert adoption["execution_frame"] is None
    assert feedback["settled"] is None
    assert not capsys.readouterr().err


@pytest.mark.parametrize("policy", _IDLE_POLICIES)
@pytest.mark.parametrize(("field", "value"), [("source_generation", 2), ("window_revision", 8)])
def test_idle_attempt_cannot_bypass_current_source_generation_or_window_fences(
    controller: AppController,
    audio_engine_mock: Mock,
    policy: GlobalTimingPolicy,
    field: str,
    value: int,
) -> None:
    feedback, adoption = _inactive_policy_refusal(controller, audio_engine_mock, policy)
    feedback[field] = value

    status = controller.transport.source_sync.status(0)

    assert status.state == "unconfirmed"
    assert status.guidance == "unconfirmed"
    assert status.active is None
    assert status.effective is None
    assert controller.transport.timing_policy.status("sync").pending is True
    assert controller.transport.timing_policy.status("realtime_bpm").pending is True
    assert adoption["state"] == "refused"


def _current_scalar_glide(
    controller: AppController, audio: Mock, *, sync: bool | None
) -> dict[str, object]:
    _assigned(controller, audio)
    neutral = sync is None
    mode = "fixed" if sync else "legacy"
    policy = GlobalTimingPolicy(
        schema_version=1,
        sync=sync,
        realtime_bpm=None if neutral else False,
        revision=0 if neutral else 2,
    )
    controller.project.global_timing_policy = policy
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(
        requested=mode == "fixed", revision=5
    )
    revision = None if neutral else policy.revision
    return _feedback(
        audio,
        active=True,
        effective=False,
        intent_revision=5,
        map_revision=None,
        policy_mode=mode,
        policy_revision=revision,
        policy_sync=policy.sync,
        policy_realtime_bpm=policy.realtime_bpm,
        settled=False,
        target_current=True,
        adoption=_adoption(
            state="refused",
            reason="unsupported",
            target_mode=mode,
            target_policy_revision=revision,
            target_policy_sync=policy.sync,
            target_policy_realtime_bpm=policy.realtime_bpm,
            target_intent_revision=5,
        ),
    )


@pytest.mark.parametrize("sync", [None, False, True])
def test_current_scalar_target_settling_precedes_redundant_adoption_refusal(
    controller: AppController, audio_engine_mock: Mock, *, sync: bool | None
) -> None:
    feedback = _current_scalar_glide(controller, audio_engine_mock, sync=sync)

    status = controller.transport.source_sync.status(0)

    assert status.state == "pending"
    assert status.guidance == "settling"
    assert status.detail is None
    assert status.active is True
    assert status.effective is False
    assert status.actual_realtime_bpm is None
    assert feedback["settled"] is False
    adoption = feedback["adoption"]
    assert isinstance(adoption, dict)
    assert adoption["state"] == "refused"

    feedback.update(settled=True, effective=sync is True)
    status = controller.transport.source_sync.status(0)
    assert status.state == ("pending" if sync else "off")
    assert status.guidance == ("phase" if sync else None)
    assert status.detail == ("Grid phase is unconfirmed." if sync else None)


def _confirmed_phase_feedback(controller: AppController, audio: Mock) -> dict[str, object]:
    feedback = _current_scalar_glide(controller, audio, sync=True)
    feedback.update(
        effective=True,
        settled=True,
        phase_program_current=True,
        phase_program_revision=11,
        phase_read_confirmed=True,
        adoption=None,
    )
    return feedback


def test_actual_current_phase_read_confirms_pad_and_global_sync(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _confirmed_phase_feedback(controller, audio_engine_mock)
    status = controller.transport.source_sync.status(0)
    assert status.state == "effective"
    assert status.grid_phase_confirmed is True
    assert status.guidance is None
    assert status.detail is None
    global_status = controller.transport.timing_policy.status("sync")
    assert global_status.grid_phase_confirmed is True
    assert global_status.pending is False


def test_phase_read_needs_the_current_original_variable_map_delivery(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    feedback = _mapped_request_with_old_audio(controller, audio, actual_mode="mapped")
    maps = controller.loader.source_beatmaps
    job = _job(audio)
    maps.select(0, "current original variable map")
    event = _event(job)
    job.resolve_source_beatmap_artifact_completion.return_value.map.return_value = _geometry()
    maps.completed(0, event)
    geometry = maps.effective(0)
    assert geometry is not None
    feedback.update(
        intent_revision=5,
        policy_revision=2,
        policy_sync=True,
        policy_realtime_bpm=True,
        map_revision=geometry.revision(),
        target_current=True,
        settled=True,
        phase_program_current=True,
        phase_program_revision=11,
        phase_read_confirmed=True,
        adoption=None,
    )
    assert controller.transport.source_sync.status(0).grid_phase_confirmed is True
    audio._resolve_loader_event.return_value = None
    assert controller.transport.source_sync.status(0).grid_phase_confirmed is None


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("phase_read_confirmed", None),
        ("phase_read_confirmed", False),
        ("phase_program_current", False),
        ("phase_program_revision", None),
        ("target_current", False),
        ("settled", False),
        ("active", False),
        ("policy_revision", 0),
        ("intent_revision", 6),
        ("source_generation", 2),
        ("window_revision", 8),
    ],
)
def test_phase_read_cannot_override_stale_or_unconfirmed_audio(
    controller: AppController, audio_engine_mock: Mock, field: str, value: object
) -> None:
    feedback = _confirmed_phase_feedback(controller, audio_engine_mock)
    feedback[field] = value
    status = controller.transport.source_sync.status(0)
    assert status.state != "effective"
    assert status.grid_phase_confirmed is None
    assert controller.transport.timing_policy.status("sync").grid_phase_confirmed is None


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("phase_read_confirmed", 1),
        ("phase_program_current", "true"),
        ("phase_program_revision", True),
        ("phase_program_revision", 0),
        ("phase_program_revision", -1),
        ("phase_program_revision", 1 << 64),
    ],
)
def test_malformed_phase_read_fields_remain_unconfirmed(
    controller: AppController, audio_engine_mock: Mock, field: str, value: object
) -> None:
    feedback = _confirmed_phase_feedback(controller, audio_engine_mock)
    feedback[field] = value
    status = controller.transport.source_sync.status(0)
    assert status.state == "unconfirmed"
    assert status.grid_phase_confirmed is None


@pytest.mark.parametrize("sync", [None, False, True])
@pytest.mark.parametrize("target_current", [None, False, "missing"])
def test_missing_or_false_target_current_cannot_override_adoption_refusal(
    controller: AppController, audio_engine_mock: Mock, target_current: object, *, sync: bool | None
) -> None:
    feedback = _current_scalar_glide(controller, audio_engine_mock, sync=sync)
    if target_current == "missing":
        del feedback["target_current"]
    else:
        feedback["target_current"] = target_current

    status = controller.transport.source_sync.status(0)

    assert status.state == "refused"
    assert status.guidance == "refused"
    assert status.effective is False


@pytest.mark.parametrize("sync", [None, False, True])
@pytest.mark.parametrize("target_current", [0, 1, "true", []])
def test_malformed_target_current_is_unconfirmed_including_neutral_policy(
    controller: AppController, audio_engine_mock: Mock, target_current: object, *, sync: bool | None
) -> None:
    feedback = _current_scalar_glide(controller, audio_engine_mock, sync=sync)
    feedback["target_current"] = target_current

    status = controller.transport.source_sync.status(0)

    assert status.state == "unconfirmed"
    assert status.guidance == "unconfirmed"
    assert status.effective is None


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("policy_mode", "off"),
        ("policy_revision", 0),
        ("policy_sync", False),
        ("policy_realtime_bpm", False),
        ("intent_revision", 4),
    ],
)
def test_target_current_cannot_override_a_mismatched_raw_policy_request(
    controller: AppController, audio_engine_mock: Mock, field: str, value: object
) -> None:
    feedback = _current_scalar_glide(controller, audio_engine_mock, sync=None)
    feedback[field] = value

    status = controller.transport.source_sync.status(0)

    assert status.state == "refused"
    assert status.guidance == "refused"


def test_target_current_cannot_bypass_source_fences_or_confirm_a_stopped_glide(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    feedback = _current_scalar_glide(controller, audio_engine_mock, sync=None)
    feedback["source_generation"] = 2
    assert controller.transport.source_sync.status(0).state == "unconfirmed"
    feedback.update(source_generation=1, active=False)
    assert controller.transport.source_sync.status(0).guidance == "refused"


def test_mapped_target_current_does_not_enter_scalar_settling_precedence(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    feedback = _current_scalar_glide(controller, audio_engine_mock, sync=True)
    geometry = _geometry()
    monkeypatch.setattr(controller.loader.source_beatmaps, "effective", lambda _id: geometry)
    controller.project.global_timing_policy = GlobalTimingPolicy(
        schema_version=1, sync=True, realtime_bpm=True, revision=2
    )
    feedback.update(
        effective=True,
        map_revision=geometry.revision(),
        policy_mode="mapped",
        policy_realtime_bpm=True,
        adoption=_adoption(
            state="refused",
            reason="unsupported",
            target_mode="mapped",
            target_policy_revision=2,
            target_policy_sync=True,
            target_policy_realtime_bpm=True,
            target_intent_revision=5,
        ),
    )

    status = controller.transport.source_sync.status(0)

    assert status.state == "refused"
    assert status.guidance == "refused"
    assert status.effective is True


@pytest.mark.parametrize("revision", [0, 7])
def test_adoption_preserves_nullable_default_and_later_raw_policy_revisions(
    controller: AppController, audio_engine_mock: Mock, revision: int
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.global_timing_policy = GlobalTimingPolicy(
        schema_version=1, sync=None, realtime_bpm=None, revision=revision
    )
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=False, revision=5)
    target_revision = None if revision == 0 else revision
    adoption = _adoption(target_policy_revision=target_revision, target_intent_revision=5)
    _feedback(
        audio_engine_mock,
        active=True,
        intent_revision=4,
        policy_mode="legacy",
        policy_revision=target_revision,
        policy_sync=None,
        policy_realtime_bpm=None,
        settled=True,
        adoption=adoption,
    )

    status = controller.transport.source_sync.status(0)
    assert status.state == "pending"
    assert status.guidance == "pending"
    assert status.effective is False

    adoption["target_policy_revision"] = 0 if revision == 0 else None
    status = controller.transport.source_sync.status(0)
    assert status.state == "pending"
    assert status.guidance == "restart"


def test_current_adoption_cannot_bypass_source_feedback_fences(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _assigned(controller, audio_engine_mock)
    _feedback(
        audio_engine_mock,
        source_generation=2,
        adoption=_adoption(state="refused", reason="unsupported"),
    )

    status = controller.transport.source_sync.status(0)

    assert status.state == "unconfirmed"
    assert status.guidance == "unconfirmed"
    assert status.effective is None


def test_sync_during_map_preparation_persists_without_cancelling_map_or_other_controls(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _assigned(controller, audio_engine_mock)
    maps = controller.loader.source_beatmaps
    job = _job(audio_engine_mock)
    maps.select(0, "Native record")
    map_intent = controller.project.pad_source_beatmap_intent[0]
    controls = (controller.project.bpm_lock, controller.project.key_lock)

    controller.transport.source_sync.set_requested(0, requested=True)
    controller._persistence.flush()

    assert controller.project.pad_source_beatmap_intent[0] is map_intent
    assert maps.status(0) == "pending"
    job.cancel.assert_not_called()
    assert (controller.project.bpm_lock, controller.project.key_lock) == controls
    saved = ProjectPersistence.from_config_path(controller._persistence.config_path)
    assert saved.project.pad_source_sync_intent[0] == PadSourceSyncIntent(
        requested=True, revision=1
    )
    row = audio_engine_mock.set_input_runtime_state.call_args.kwargs["source_map_rows"][0]
    assert row == {
        "sync_requested": True,
        "intent_revision": 1,
        "delivery": None,
        "exact_quarters": None,
        "ordinary_pending": True,
    }


def test_gui_learn_and_midi_dispatch_use_same_independent_fixed_target_action(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _assigned(controller, audio_engine_mock)
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)
    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
    })
    ctx.audio.pads.toggle_source_sync(0)
    assert controller.project.pad_source_sync_intent[0] == neutral_source_sync_intent()
    learned = load_midi_mapping_file().mappings[0].action
    assert learned == source_sync_toggle_action(0)
    assert controller.session.input_learn_active is False
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": learned.key,
    })
    assert controller.project.pad_source_sync_intent[0].requested
    ctx.audio.pads.toggle_source_sync(0)
    assert controller.project.pad_source_sync_intent[0] == PadSourceSyncIntent(
        requested=False, revision=2
    )


def test_ready_map_never_implies_effective_and_old_mapped_audio_cannot_ack_new_request(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _assigned(controller, audio_engine_mock)
    geometry = _geometry()
    monkeypatch.setattr(controller.loader.source_beatmaps, "effective", lambda _id: geometry)
    feedback = _feedback(audio_engine_mock)
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=4)
    status = controller.transport.source_sync.status(0)
    assert status.state == "stopped"
    assert status.effective is False
    feedback.update(
        active=True, effective=True, intent_revision=3, map_revision=geometry.revision()
    )
    status = controller.transport.source_sync.status(0)
    assert status.state == "pending"
    assert status.effective is True
    feedback["intent_revision"] = 4
    assert controller.transport.source_sync.status(0).state == "pending"
    assert controller.transport.source_sync.status(0).grid_phase_confirmed is None
    feedback["map_revision"] = "f" * 64
    assert controller.transport.source_sync.status(0).state == "pending"
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=False, revision=5)
    status = controller.transport.source_sync.status(0)
    assert status.state == "pending"
    assert status.effective is True
    assert not status.requested
    feedback.update(active=False, effective=False, intent_revision=None, map_revision=None)
    assert controller.transport.source_sync.status(0).state == "off"


@pytest.mark.parametrize(
    "field", ["source_generation", "window_revision", "active", "intent_revision"]
)
def test_stale_or_malformed_feedback_never_confirms_current_adoption(
    controller: AppController, audio_engine_mock: Mock, field: str
) -> None:
    _assigned(controller, audio_engine_mock)
    feedback = _feedback(audio_engine_mock)
    feedback[field] = -1 if field != "active" else 1
    assert controller.transport.source_sync.status(0).state == "unconfirmed"


def test_same_frame_gui_and_global_capture_refresh_complete_runtime_first(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _assigned(controller, audio_engine_mock)
    events: list[str] = []
    publish = audio_engine_mock.set_input_runtime_state.side_effect

    def runtime(*args: object, **kwargs: object) -> object:
        events.append("runtime")
        return publish(*args, **kwargs)

    audio_engine_mock.set_input_runtime_state.side_effect = runtime
    original_capture = audio_engine_mock.capture_pad_start.side_effect

    def capture(*args: object, **kwargs: object) -> object:
        events.append("capture")
        return original_capture(*args, **kwargs)

    audio_engine_mock.capture_pad_start.side_effect = capture
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=1)
    controller.transport.playback.trigger_pad(0)
    assert events[:2] == ["runtime", "capture"]
    events.clear()
    controller.session.active_sample_ids.add(0)
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=False, revision=2)
    original_global = audio_engine_mock.capture_global_start.side_effect

    def global_capture(*args: object, **kwargs: object) -> object:
        events.append("global")
        return original_global(*args, **kwargs)

    audio_engine_mock.capture_global_start.side_effect = global_capture
    controller.transport.playback.start_or_restart_global_start_stop()
    assert events[:2] == ["runtime", "global"]


@pytest.mark.parametrize("action", ["pad", "global"])
def test_untouched_scalar_capture_keeps_original_path_despite_unrelated_sync_intent(
    controller: AppController, audio_engine_mock: Mock, action: str
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.pad_source_sync_intent[1] = PadSourceSyncIntent(requested=True, revision=1)
    refresh = Mock(
        side_effect=AssertionError("unrelated SYNC must not recapture this scalar source")
    )
    controller.transport.bind_input_runtime_refresh(refresh)
    if action == "pad":
        controller.transport.playback.trigger_pad(0)
        audio_engine_mock.capture_pad_start.assert_called_once()
    else:
        controller.session.active_sample_ids = {0}
        controller.transport.playback.start_or_restart_global_start_stop()
        audio_engine_mock.capture_global_start.assert_called_once()
    refresh.assert_not_called()


def test_failed_runtime_publication_retains_requested_pending_intent(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _assigned(controller, audio_engine_mock)
    audio_engine_mock.set_input_runtime_state.side_effect = ValueError("publication rejected")
    with pytest.raises(RuntimeError, match="control state"):
        controller.transport.source_sync.set_requested(0, requested=True)
    assert controller.project.pad_source_sync_intent[0] == PadSourceSyncIntent(
        requested=True, revision=1
    )
    assert controller.transport.source_sync.status(0).state == "unconfirmed"


def test_unload_defaults_clear_only_replaced_content_sync(controller: AppController) -> None:
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="e" * 32)
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=3)
    controller.project.pad_source_sync_intent[1] = PadSourceSyncIntent(requested=True, revision=8)
    controller.loader._reset_unloaded_pad_project_defaults(0, type(controller.project)())
    assert controller.project.pad_source_sync_intent[0] == neutral_source_sync_intent()
    assert controller.project.pad_source_sync_intent[1].revision == 8


def test_runtime_publishes_requested_mapped_geometry_instead_of_old_effective_window(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _assigned(controller, audio_engine_mock)
    project = controller.project
    project.manual_bpm[0] = 120.0
    project.pad_loop_start_s[0] = 1.0
    project.pad_loop_bars[0] = 1.0
    project.pad_loop_auto[0] = True
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=1)
    geometry, delivery = _geometry(), object()
    maps = controller.loader.source_beatmaps
    monkeypatch.setattr(maps, "effective", lambda _id: geometry)
    monkeypatch.setattr(maps, "_playback_projection", lambda _id: (geometry, delivery))
    monkeypatch.setattr(
        controller.transport.residency, "acknowledged_region", lambda _id: (1.0, 3.0)
    )

    assert controller.transport.loop.effective_region(0) == (1.0, 3.0)
    assert controller.input_mapping._refresh_rust_runtime_state()

    publication = audio_engine_mock.set_input_runtime_state.call_args
    assert publication.args[2][0] == 1.0
    assert publication.args[3][0] == 3.5
    assert controller.transport.loop.requested_region(0) == (1.0, 3.5)
    row = publication.kwargs["source_map_rows"][0]
    assert row["delivery"] is delivery
    assert row["exact_quarters"] == 4.0
    # OFF adoption must capture the new scalar auto extent, not the retained
    # old mapped window, through both GUI and the same device-runtime row.
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=False, revision=2)
    monkeypatch.setattr(
        controller.transport.residency, "acknowledged_region", lambda _id: (1.0, 3.5)
    )
    assert controller.input_mapping._refresh_rust_runtime_state()
    assert controller.transport.loop.requested_region(0) == (1.0, 3.0)
    assert audio_engine_mock.set_input_runtime_state.call_args.args[3][0] == 3.0
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=3)
    project.pad_loop_auto[0] = False
    project.pad_loop_end_s[0] = 4.0
    assert controller.input_mapping._refresh_rust_runtime_state()
    assert (
        audio_engine_mock.set_input_runtime_state.call_args.kwargs["source_map_rows"][0][
            "exact_quarters"
        ]
        is None
    )


def test_deferred_arrangement_keeps_the_whole_original_map_request_row(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _assigned(controller, audio_engine_mock)
    intent = PadSourceSyncIntent(requested=True, revision=6)
    controller.project.pad_source_sync_intent[0] = intent
    assert controller.input_mapping._refresh_rust_runtime_state()
    original = controller.input_mapping._last_input_runtime_state_signature[2][0]
    controller.transport._pair_deferred_sample_ids = frozenset({0})
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=False, revision=7)
    lookup = Mock(side_effect=AssertionError("reserved row must not be recaptured"))
    monkeypatch.setattr(controller.loader.source_beatmaps, "_playback_projection", lookup)

    assert controller.input_mapping._refresh_rust_runtime_state()

    assert controller.input_mapping._last_input_runtime_state_signature[2][0] is original
    lookup.assert_not_called()
