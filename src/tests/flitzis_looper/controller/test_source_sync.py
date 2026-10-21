"""Control publication and callback-feedback routing; Native supplies separate audio proof."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.input_mapping.actions import source_sync_toggle_action
from flitzis_looper.input_mapping.storage import load_midi_mapping_file
from flitzis_looper.models import PadContentIdentity
from flitzis_looper.source_sync_intent import PadSourceSyncIntent, neutral_source_sync_intent
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.test_source_beatmap_selection import _assigned, _job
from tests.flitzis_looper.controller.transport.test_source_map_loop import _geometry

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


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
    assert controller.transport.source_sync.status(0).state == "effective"
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
