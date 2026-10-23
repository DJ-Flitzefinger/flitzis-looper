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
from tests.flitzis_looper.controller.test_source_beatmap_selection import _assigned, _job
from tests.flitzis_looper.controller.transport.test_source_map_loop import _geometry

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


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
    monkeypatch.setattr(maps, "playback_delivery", lambda _id: delivery)
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
    monkeypatch.setattr(controller.loader.source_beatmaps, "playback_delivery", lookup)

    assert controller.input_mapping._refresh_rust_runtime_state()

    assert controller.input_mapping._last_input_runtime_state_signature[2][0] is original
    lookup.assert_not_called()
