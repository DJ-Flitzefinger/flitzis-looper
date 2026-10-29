"""Productive controller transactions use actual native current ownership."""

import math
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, call

import pytest

from flitzis_looper.models import BeatGrid, SampleAnalysis
from flitzis_looper_audio import AudioMessage
from tests.flitzis_looper.conftest import (
    FakeCapturedGlobalStart,
    FakeGlobalPlaybackBatchTicket,
    FakeInputRuntimePadBinding,
    current_timing_metadata,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController
    from flitzis_looper.models import TimingIntent


PRECISE_PERIOD = 0.2245048325556449


def test_unload_last_remembered_pad_normalizes_stop_indicator(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0}
    controller.loader.unload_sample(0)

    assert controller.session.global_stop_restore_sample_ids == set()
    assert controller.session.global_stop_engaged is False
    audio_engine_mock.unload_sample.assert_called_once_with(0)


def _accepted_sources(
    controller: AppController, audio: Mock, ids: tuple[int, ...] = (0, 2)
) -> tuple[dict[int, dict[str, object]], dict[int, FakeInputRuntimePadBinding]]:
    controller.project.multi_loop = True
    metadata = {
        sample_id: current_timing_metadata(
            sample_id=sample_id, period=PRECISE_PERIOD, origin=-0.125
        )
        for sample_id in ids
    }
    bindings = {
        sample_id: FakeInputRuntimePadBinding(sample_id, accepted_timing=value, intent="automatic")
        for sample_id, value in metadata.items()
    }
    for sample_id in ids:
        controller.project.sample_paths[sample_id] = f"saved/stale-{sample_id}.wav"
        controller.project.sample_durations[sample_id] = 1.0
        controller.project.sample_analysis[sample_id] = SampleAnalysis(
            bpm=135.0, key="C", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[2.0])
        )
        controller.project.pad_loop_auto[sample_id] = True
        controller.project.pad_loop_bars[sample_id] = 1.0
    audio.current_constant_timing.side_effect = metadata.get
    audio.pad_timing_intent.side_effect = lambda _sample_id: "automatic"
    audio.current_input_runtime_pad_binding.side_effect = bindings.get
    # This fixture starts after whole-runtime admission. Separate input-mapping
    # tests cover publication; these cases observe the batch's one source capture.
    controller.transport.bind_input_runtime_refresh(lambda: True)
    return metadata, bindings


def _assert_no_unguarded_batch_effect(audio: Mock) -> None:
    audio.start_global_playback_batch.assert_not_called()
    audio.set_pad_loop_region.assert_not_called()
    audio.play_sample.assert_not_called()
    audio.play_sample_exclusive.assert_not_called()
    audio.stop_all.assert_not_called()
    audio.set_pad_bpm.assert_not_called()
    audio.set_pad_timing_metadata.assert_not_called()


def _set_start_ticket(audio: Mock, ticket: FakeGlobalPlaybackBatchTicket) -> None:
    audio.publish_captured_global_start.side_effect = lambda capture: capture.publish(ticket)


def _published_capture(audio: Mock) -> FakeCapturedGlobalStart:
    capture = audio.publish_captured_global_start.call_args.args[0]
    assert isinstance(capture, FakeCapturedGlobalStart)
    audio.publish_captured_global_start.assert_called_once_with(capture)
    assert capture.entries == tuple(audio.capture_global_start.call_args.args[0])
    return capture


class _ResidentWindow:
    """Explicit controller dependency; actual Native ACK ownership is tested in Rust."""

    def __init__(self) -> None:
        self.status = "preparing"
        self.current = True
        self.failure: str | None = None
        self.cancelled = False
        self.previous_window_revision = 1
        self.window_revision = 2
        self.key_lock_request_id: int | None = None

    def publication_status(self) -> str:
        return self.status

    def is_current(self) -> bool:
        return self.current

    def error(self) -> str | None:
        return self.failure

    def cancel(self) -> bool:
        self.cancelled = True
        return True


def _pending_loop_edit(controller: AppController, audio: Mock) -> _ResidentWindow:
    _accepted_sources(controller, audio, (0,))
    controller.session.active_sample_ids = {0}
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 0.125
    controller.project.pad_loop_end_s[0] = 0.25
    audio.loaded_residency.return_value = {"source_identity": 123, "window_revision": 1}
    controller.transport.residency.remember(0)
    window = _ResidentWindow()
    audio.prepare_resident_control = Mock(return_value=window)
    controller.project.pad_loop_start_s[0] = 0.5
    controller.project.pad_loop_end_s[0] = 0.75
    controller.transport.residency.publish_loop(0, 0.5, 0.75)
    assert controller.transport.residency.acknowledged_region(0) == (0.125, 0.25)
    return window


@pytest.mark.parametrize("received_at_ns", [None, 0, 123_456_789])
def test_global_restart_carries_one_exact_current_snapshot_per_actual_source(
    controller: AppController, audio_engine_mock: Mock, received_at_ns: int | None
) -> None:
    _, bindings = _accepted_sources(controller, audio_engine_mock)
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 2}
    controller.project.sample_paths[2] = None
    audio_engine_mock.reset_mock()

    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=received_at_ns)

    entries = audio_engine_mock.capture_global_start.call_args.args[0]
    assert [binding for binding, _, _, _ in entries] == [bindings[0], bindings[2]]
    end_s = round(4.0 * PRECISE_PERIOD * 48_000) / 48_000
    assert [(start, end) for _, start, end, _ in entries] == [(0.0, end_s), (0.0, end_s)]
    assert [dependency for _, _, _, dependency in entries] == [None, None]
    capture = _published_capture(audio_engine_mock)
    assert capture.received_at_ns == received_at_ns
    assert audio_engine_mock.capture_global_start.call_args.kwargs == {
        "received_at_ns": received_at_ns
    }
    assert audio_engine_mock.current_constant_timing.call_args_list == [call(0), call(2)]
    assert audio_engine_mock.current_input_runtime_pad_binding.call_args_list == [call(0), call(2)]
    assert controller.session.global_stop_restore_sample_ids == {0, 2}
    assert controller.session.active_sample_ids == set()
    controller.transport.on_frame_render()
    assert controller.session.global_stop_restore_sample_ids == set()
    assert controller.session.active_sample_ids == set()
    _assert_no_unguarded_batch_effect(audio_engine_mock)


@pytest.mark.parametrize("operation", ["restart", "right_stop", "stop_all"])
def test_one_unavailable_automatic_member_rejects_entire_productive_batch(
    controller: AppController, audio_engine_mock: Mock, operation: str
) -> None:
    metadata, _ = _accepted_sources(controller, audio_engine_mock)
    metadata.pop(2)
    controller.session.active_sample_ids = {0, 2}
    controller.session.paused_sample_ids = {2}
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 2}
    before = controller.session.model_dump()
    project_before = controller.project.model_dump()
    audio_engine_mock.reset_mock()
    methods: dict[str, Callable[[], None]] = {
        "restart": controller.transport.playback.start_or_restart_global_start_stop,
        "right_stop": controller.transport.playback.stop_global_start_stop,
        "stop_all": controller.transport.playback.stop_all_pads,
    }

    methods[operation]()

    assert controller.session.model_dump() == before
    assert controller.project.model_dump() == project_before
    audio_engine_mock.capture_global_start.assert_not_called()
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    _assert_no_unguarded_batch_effect(audio_engine_mock)


@pytest.mark.parametrize(
    ("key", "replacement"),
    [
        ("source_id", "replaced-source"),
        ("source_generation", 8),
        ("accepted_request_id", 10),
        ("publication_epoch", 12),
        ("source_sha256", "c" * 64),
        ("source_provenance", "different association"),
        ("pcm_sha256", "d" * 64),
        ("sample_rate_hz", 44_100),
        ("frame_count", 48_000 * 600 + 1),
        ("source_zero_seconds", math.nextafter(0.0, 1.0)),
        ("mono_revision", "different-mono"),
        ("raw_revision", "different-raw"),
        ("revision", "same-period-different-evidence"),
        ("period_seconds_per_quarter", math.nextafter(PRECISE_PERIOD, 1.0)),
        ("origin_seconds", math.nextafter(-0.125, 1.0)),
        ("origin_provenance", "different-independent-origin"),
        ("acceptance_policy_version", "different-policy"),
        ("acceptance_provenance", "different-assessment"),
    ],
)
def test_current_binding_mismatch_cannot_admit_or_partially_clear_restore(
    controller: AppController, audio_engine_mock: Mock, key: str, replacement: object
) -> None:
    metadata, bindings = _accepted_sources(controller, audio_engine_mock)
    bindings[2] = FakeInputRuntimePadBinding(
        2, accepted_timing=dict(metadata[2], **{key: replacement}), intent="automatic"
    )
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 2}
    before = controller.session.model_dump()
    audio_engine_mock.reset_mock()

    controller.transport.playback.start_or_restart_global_start_stop()

    assert controller.session.model_dump() == before
    audio_engine_mock.capture_global_start.assert_not_called()
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    _assert_no_unguarded_batch_effect(audio_engine_mock)


@pytest.mark.parametrize("field", ["origin_seconds", "source_zero_seconds"])
def test_current_binding_signed_zero_is_not_interchangeable(
    controller: AppController, audio_engine_mock: Mock, field: str
) -> None:
    metadata, bindings = _accepted_sources(controller, audio_engine_mock, (0,))
    metadata[0][field] = 0.0
    bindings[0] = FakeInputRuntimePadBinding(
        0, accepted_timing=dict(metadata[0], **{field: -0.0}), intent="automatic"
    )
    controller.session.active_sample_ids = {0}

    controller.transport.playback.start_or_restart_global_start_stop()

    audio_engine_mock.capture_global_start.assert_not_called()
    audio_engine_mock.publish_captured_global_start.assert_not_called()


@pytest.mark.parametrize("operation", ["restart", "right_stop", "stop_all"])
@pytest.mark.parametrize("failure", ["missing_source", "capture_failed"])
def test_native_capture_failure_never_falls_back_to_saved_paths_or_partial_batch(
    controller: AppController, audio_engine_mock: Mock, operation: str, failure: str
) -> None:
    _, bindings = _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 2}
    before = controller.session.model_dump()
    project_before = controller.project.model_dump()
    methods: dict[str, Callable[[], None]] = {
        "restart": controller.transport.playback.start_or_restart_global_start_stop,
        "right_stop": controller.transport.playback.stop_global_start_stop,
        "stop_all": controller.transport.playback.stop_all_pads,
    }
    if failure == "missing_source":
        bindings.pop(2)
        methods[operation]()
    else:
        audio_engine_mock.current_input_runtime_pad_binding.side_effect = [
            bindings[0],
            RuntimeError("native source unavailable"),
        ]
        with pytest.raises(RuntimeError, match="native source unavailable"):
            methods[operation]()

    assert controller.session.model_dump() == before
    assert controller.project.model_dump() == project_before
    audio_engine_mock.capture_global_start.assert_not_called()
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    _assert_no_unguarded_batch_effect(audio_engine_mock)


@pytest.mark.parametrize("intent", ["manual", "tap", "legacy"])
def test_explicit_nonaccepted_authority_uses_native_binding_without_promoting_analysis(
    controller: AppController, audio_engine_mock: Mock, intent: TimingIntent
) -> None:
    controller.project.multi_loop = True
    controller.project.sample_paths[0] = "saved/ordinary.wav"
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=135.0, key="C", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[])
    )
    controller.project.pad_timing_intent[0] = intent
    controller.project.pad_loop_auto[0] = True
    controller.project.pad_loop_bars[0] = 1.0
    controller.session.active_sample_ids = {0}
    binding = FakeInputRuntimePadBinding(0, intent=intent)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = lambda _sample_id: binding
    audio_engine_mock.pad_timing_intent.return_value = intent
    audio_engine_mock.reset_mock()

    controller.transport.playback.start_or_restart_global_start_stop()

    entries = audio_engine_mock.capture_global_start.call_args.args[0]
    assert entries[0][0] is binding
    assert binding.metadata()["accepted_timing"] is None
    assert entries[0][1:] == (0.0, round((4 * 60 / 135) * 44_100) / 44_100, None)
    _published_capture(audio_engine_mock)
    _assert_no_unguarded_batch_effect(audio_engine_mock)


@pytest.mark.parametrize("operation", ["restart", "right_stop", "stop_all"])
def test_failed_native_batch_admission_preserves_all_saved_and_session_intent(
    controller: AppController, audio_engine_mock: Mock, operation: str
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    controller.session.paused_sample_ids = {2}
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 2}
    before = controller.session.model_dump()
    project_before = controller.project.model_dump()
    audio_engine_mock.publish_captured_global_start.side_effect = RuntimeError("full command ring")
    audio_engine_mock.stop_global_playback_batch.side_effect = RuntimeError("full command ring")
    audio_engine_mock.reset_mock()
    methods: dict[str, Callable[[], None]] = {
        "restart": controller.transport.playback.start_or_restart_global_start_stop,
        "right_stop": controller.transport.playback.stop_global_start_stop,
        "stop_all": controller.transport.playback.stop_all_pads,
    }

    with pytest.raises(RuntimeError, match="full command ring"):
        methods[operation]()

    assert controller.session.model_dump() == before
    assert controller.project.model_dump() == project_before
    _assert_no_unguarded_batch_effect(audio_engine_mock)


@pytest.mark.parametrize("operation", ["restart", "right_stop", "stop_all"])
@pytest.mark.parametrize("final_status", ["accepted", "rejected"])
def test_pending_batch_and_native_feedback_preserve_telemetry_owned_voice_sets(
    controller: AppController, audio_engine_mock: Mock, operation: str, final_status: str
) -> None:
    _, bindings = _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    controller.session.paused_sample_ids = {2}
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {2}
    ticket = FakeGlobalPlaybackBatchTicket("pending")
    _set_start_ticket(audio_engine_mock, ticket)
    audio_engine_mock.stop_global_playback_batch.return_value = ticket
    methods: dict[str, Callable[[], None]] = {
        "restart": controller.transport.playback.start_or_restart_global_start_stop,
        "right_stop": controller.transport.playback.stop_global_start_stop,
        "stop_all": controller.transport.playback.stop_all_pads,
    }
    before = controller.session.model_dump()

    methods[operation]()
    controller.transport.on_frame_render()
    assert controller.session.model_dump() == before
    if operation != "restart":
        audio_engine_mock.stop_global_playback_batch.assert_called_once_with(
            [bindings[0], bindings[2]], received_at_ns=None
        )
    else:
        _published_capture(audio_engine_mock)
    controller.session.active_sample_ids.add(5)
    ticket.status = final_status
    controller.transport.on_frame_render()

    assert controller.session.active_sample_ids == {0, 2, 5}
    assert controller.session.paused_sample_ids == {2}
    if final_status == "rejected":
        assert controller.session.global_stop_engaged is True
        assert controller.session.global_stop_restore_sample_ids == {2}
    elif operation == "right_stop":
        assert controller.session.global_stop_engaged is True
        assert controller.session.global_stop_restore_sample_ids == {0}
    else:
        assert controller.session.global_stop_engaged is False
        assert controller.session.global_stop_restore_sample_ids == set()


@pytest.mark.parametrize("start_status", ["pending", "accepted"])
def test_stop_supersedes_start_but_pending_stop_blocks_further_global_operations(
    controller: AppController, audio_engine_mock: Mock, start_status: str
) -> None:
    _, bindings = _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    start = FakeGlobalPlaybackBatchTicket(start_status)
    stop = FakeGlobalPlaybackBatchTicket("pending")
    _set_start_ticket(audio_engine_mock, start)
    audio_engine_mock.stop_global_playback_batch.return_value = stop
    before = controller.session.model_dump()
    controller.transport.playback.start_or_restart_global_start_stop()

    controller.transport.playback.stop_global_start_stop()
    controller.transport.playback.stop_all_pads()
    controller.transport.playback.start_or_restart_global_start_stop()

    audio_engine_mock.capture_global_start.assert_called_once()
    _published_capture(audio_engine_mock)
    audio_engine_mock.stop_global_playback_batch.assert_called_once_with(
        [bindings[0], bindings[2]], received_at_ns=None
    )
    assert controller.session.model_dump() == before
    _assert_no_unguarded_batch_effect(audio_engine_mock)

    stop.status = "accepted"
    controller.transport.playback.start_or_restart_global_start_stop()

    assert audio_engine_mock.capture_global_start.call_count == 2
    assert audio_engine_mock.publish_captured_global_start.call_count == 2
    audio_engine_mock.stop_global_playback_batch.assert_called_once()
    assert controller.session.active_sample_ids == {0, 2}
    assert controller.session.paused_sample_ids == set()
    _assert_no_unguarded_batch_effect(audio_engine_mock)


def test_manual_gesture_prevents_late_stop_ack_from_reviving_old_restore_set(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    ticket = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.stop_global_playback_batch.return_value = ticket
    controller.transport.playback.stop_global_start_stop()

    controller.transport.playback.stop_pad(0)
    ticket.status = "accepted"
    controller.transport.on_frame_render()

    assert controller.session.global_stop_engaged is False
    assert controller.session.global_stop_restore_sample_ids == set()
    audio_engine_mock.stop_sample.assert_called_once_with(0)


def test_restart_ack_preserves_paused_voice_until_actual_started_telemetry(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 2}
    controller.session.active_sample_ids = {2}
    controller.session.paused_sample_ids = {2}
    ticket = FakeGlobalPlaybackBatchTicket("pending")
    _set_start_ticket(audio_engine_mock, ticket)
    controller.transport.playback.start_or_restart_global_start_stop()
    unrelated_started = Mock()
    unrelated_started.sample_id.return_value = 5
    controller.transport.playback.handle_sample_started_message(unrelated_started)
    ticket.status = "accepted"
    controller.transport.on_frame_render()

    assert controller.session.active_sample_ids == {2, 5}
    assert controller.session.paused_sample_ids == {2}
    for sample_id in (0, 2):
        started = Mock()
        started.sample_id.return_value = sample_id
        controller.transport.playback.handle_sample_started_message(started)
    assert controller.session.active_sample_ids == {0, 2, 5}
    assert controller.session.paused_sample_ids == set()


@pytest.mark.parametrize("operation", ["unload", "replace"])
@pytest.mark.parametrize("final_status", ["accepted", "rejected"])
def test_productive_unload_or_replace_prunes_pad_from_late_global_stop_restore(
    controller: AppController, audio_engine_mock: Mock, operation: str, final_status: str
) -> None:
    metadata, bindings = _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {5}
    ticket = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.stop_global_playback_batch.return_value = ticket
    controller.transport.playback.stop_global_start_stop()
    if final_status == "accepted":
        ticket.status = final_status

    def retire_native_source(sample_id: int) -> None:
        metadata.pop(sample_id)
        bindings.pop(sample_id)

    audio_engine_mock.unload_sample.side_effect = retire_native_source
    if operation == "unload":
        controller.loader.unload_sample(2)
    else:
        audio_engine_mock.load_incoming_sample_async.return_value = 7
        controller.loader.load_sample_async(2, "replacement/source.wav")
        audio_engine_mock.load_incoming_sample_async.assert_called_once_with(
            2, "replacement/source.wav", controller.project.resolve_pad_key_lock(2)
        )
        audio_engine_mock.load_sample_async.assert_not_called()
        assert 2 in controller.session.active_sample_ids
        audio_engine_mock.unload_sample.assert_not_called()
        # The actual cold callback replaces source ownership before success metadata.
        retire_native_source(2)
        audio_engine_mock.poll_loader_events.side_effect = [
            {
                "type": "success",
                "id": 2,
                "request_id": 7,
                "cached_path": "samples/replacement.wav",
                "duration_s": 1.0,
                "incoming_defaults": {
                    "revision": 0,
                    "key_lock": controller.project.resolve_pad_key_lock(2),
                    "start_s": 0.0,
                    "end_s": None,
                    "period_seconds": None,
                    "grid_origin_s": 0.0,
                    "loop_auto": True,
                    "loop_bars": 8.0,
                },
            },
            None,
        ]
        controller.loader.poll_loader_events()
        assert controller.project.sample_paths[2] == "samples/replacement.wav"
        assert 2 not in controller.session.sample_load_errors
    ticket.status = final_status
    controller.transport.on_frame_render()

    if operation == "unload":
        audio_engine_mock.unload_sample.assert_called_once_with(2)
    else:
        audio_engine_mock.unload_sample.assert_not_called()
    assert 2 not in controller.session.active_sample_ids
    assert 2 not in controller.session.paused_sample_ids
    assert controller.session.global_stop_engaged is True
    assert controller.session.global_stop_restore_sample_ids == (
        {0} if final_status == "accepted" else {5}
    )


def test_failed_cold_replacement_keeps_pad_in_late_global_stop_restore(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    controller.session.global_stop_engaged = True
    ticket = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.stop_global_playback_batch.return_value = ticket
    controller.transport.playback.stop_global_start_stop()
    audio_engine_mock.load_incoming_sample_async.return_value = 7
    controller.loader.load_sample_async(2, "replacement/source.wav")
    audio_engine_mock.load_incoming_sample_async.assert_called_once_with(
        2, "replacement/source.wav", controller.project.resolve_pad_key_lock(2)
    )
    audio_engine_mock.load_sample_async.assert_not_called()
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "error", "id": 2, "request_id": 7, "msg": "cold preparation failed"},
        None,
    ]
    controller.loader.poll_loader_events()
    ticket.status = "accepted"

    controller.transport.on_frame_render()

    assert controller.project.sample_paths[2] == "saved/stale-2.wav"
    assert controller.session.sample_load_errors[2] == "cold preparation failed"
    assert 2 not in controller.session.loading_sample_ids
    assert controller.session.global_stop_restore_sample_ids == {0, 2}
    audio_engine_mock.unload_sample.assert_not_called()


def test_actual_app_drain_keeps_unloaded_pad_inactive_after_delayed_started_feedback(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    metadata, bindings = _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0}
    ticket = FakeGlobalPlaybackBatchTicket("pending")
    _set_start_ticket(audio_engine_mock, ticket)
    controller.transport.playback.start_or_restart_global_start_stop()
    assert controller.session.active_sample_ids == set()
    ticket.status = "accepted"

    def retire_native_source(sample_id: int) -> None:
        metadata.pop(sample_id)
        bindings.pop(sample_id)

    audio_engine_mock.unload_sample.side_effect = retire_native_source
    controller.loader.unload_sample(0)
    audio_engine_mock.receive_msg.side_effect = [
        AudioMessage.SampleStarted(0),
        AudioMessage.SampleStopped(0),
        None,
    ]
    audio_engine_mock.poll_loader_events.return_value = None

    controller.poll_runtime_events()
    controller.transport.on_frame_render()

    assert controller.project.sample_paths[0] is None
    assert controller.session.active_sample_ids == set()
    assert controller.session.paused_sample_ids == set()
    assert controller.session.global_stop_engaged is False
    assert controller.session.global_stop_restore_sample_ids == set()
    audio_engine_mock.capture_global_start.assert_called_once()
    audio_engine_mock.unload_sample.assert_called_once_with(0)


@pytest.mark.parametrize("timestamp", [-1, True, 1.5, "12", 1 << 64])
@pytest.mark.parametrize("operation", ["restart", "stop_all"])
def test_invalid_global_timestamp_rejects_before_capturing_or_publishing(
    controller: AppController, audio_engine_mock: Mock, timestamp: object, operation: str
) -> None:
    controller.session.active_sample_ids = {0, 2}
    audio_engine_mock.reset_mock()
    if operation == "restart":
        with pytest.raises(ValueError, match="received_at_ns"):
            controller.transport.playback.start_or_restart_global_start_stop(
                received_at_ns=cast("int", timestamp)
            )
    else:
        with pytest.raises(ValueError, match="received_at_ns"):
            controller.transport.playback.stop_all_pads(received_at_ns=cast("int", timestamp))

    assert audio_engine_mock.method_calls == []


def test_deferred_global_start_retains_original_source_loop_time_and_same_capture(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _, bindings = _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 0.25
    controller.project.pad_loop_end_s[0] = 0.75
    ready = False

    def publish(capture: FakeCapturedGlobalStart) -> FakeGlobalPlaybackBatchTicket | None:
        return capture.publish() if ready else None

    audio_engine_mock.publish_captured_global_start.side_effect = publish
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=123)
    capture = _published_capture(audio_engine_mock)
    assert capture.entries == ((bindings[0], 0.25, 0.75, None),)
    assert capture.received_at_ns == 123
    capture.frame = 24_000
    original_project = controller.project.model_dump()
    controller.project.pad_loop_start_s[0] = 0.5
    controller.project.pad_loop_end_s[0] = 1.0
    source_reads = audio_engine_mock.current_input_runtime_pad_binding.call_count
    timing_reads = audio_engine_mock.current_constant_timing.call_count
    for _ in range(3):
        controller.transport.playback._global_playback.poll()
    ready = True
    controller.transport.playback._global_playback.poll()

    audio_engine_mock.capture_global_start.assert_called_once()
    assert audio_engine_mock.publish_captured_global_start.call_args_list == [call(capture)] * 5
    assert audio_engine_mock.current_input_runtime_pad_binding.call_count == source_reads
    assert audio_engine_mock.current_constant_timing.call_count == timing_reads
    assert capture.entries == ((bindings[0], 0.25, 0.75, None),)
    assert capture.received_at_ns == 123
    assert capture.original_frame() == 24_000
    assert controller.project.model_dump() != original_project
    assert controller.project.pad_loop_start_s[0] == 0.5
    assert controller.project.pad_loop_end_s[0] == 1.0
    _assert_no_unguarded_batch_effect(audio_engine_mock)


def test_global_capture_retains_pending_window_and_requested_instead_of_effective_loop(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    window = _pending_loop_edit(controller, audio_engine_mock)

    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=456)
    capture = _published_capture(audio_engine_mock)
    original_binding, start, end, dependency = capture.entries[0]
    assert (start, end) == (0.5, 0.75)
    assert dependency is window
    assert capture.received_at_ns == 456
    source_reads = audio_engine_mock.current_input_runtime_pad_binding.call_count
    timing_reads = audio_engine_mock.current_constant_timing.call_count
    controller.transport.playback._global_playback.poll()
    assert capture.state() == "captured"
    window.status = "accepted"
    controller.transport.playback._global_playback.poll()

    assert capture.entries == ((original_binding, 0.5, 0.75, window),)
    audio_engine_mock.capture_global_start.assert_called_once()
    assert audio_engine_mock.publish_captured_global_start.call_args_list == [call(capture)] * 3
    assert audio_engine_mock.current_input_runtime_pad_binding.call_count == source_reads
    assert audio_engine_mock.current_constant_timing.call_count == timing_reads
    assert window.cancelled is False
    audio_engine_mock.play_resident_control.assert_not_called()
    _assert_no_unguarded_batch_effect(audio_engine_mock)


def test_global_start_rejects_python_pending_edit_without_native_ticket(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _pending_loop_edit(controller, audio_engine_mock)
    audio_engine_mock.prepare_resident_control.side_effect = RuntimeError(
        "resident command queue is full"
    )
    controller.project.pad_loop_start_s[0] = 0.625
    controller.transport.residency.publish_loop(0, 0.625, 0.75)
    before = controller.session.model_dump()
    audio_engine_mock.reset_mock()

    controller.transport.playback.start_or_restart_global_start_stop()

    audio_engine_mock.capture_global_start.assert_not_called()
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    assert controller.session.model_dump() == before
    _assert_no_unguarded_batch_effect(audio_engine_mock)


def test_forgetting_deferred_global_intent_cancels_attacks_and_preserves_own_edit(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    window = _pending_loop_edit(controller, audio_engine_mock)
    controller.transport.playback.start_or_restart_global_start_stop()
    capture = _published_capture(audio_engine_mock)

    controller.transport.playback._forget_global_start_stop_restore()
    window.status = "accepted"
    controller.transport.playback._global_playback.poll()

    assert capture.cancelled is True
    assert window.cancelled is False
    assert capture.entries[0][3] is window
    audio_engine_mock.capture_global_start.assert_called_once()
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(capture)
    _assert_no_unguarded_batch_effect(audio_engine_mock)


def test_global_queue_pressure_retries_same_capture_with_bounded_budget(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    audio_engine_mock.publish_captured_global_start.side_effect = RuntimeError(
        "ordinary start queue is full"
    )
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=789)
    capture = _published_capture(audio_engine_mock)
    for _ in range(10):
        controller.transport.playback._global_playback.poll()

    audio_engine_mock.capture_global_start.assert_called_once()
    assert audio_engine_mock.publish_captured_global_start.call_args_list == [call(capture)] * 8
    assert capture.cancelled is True
    assert capture.received_at_ns == 789
    assert controller.transport.residency.status(0)[1] == "ordinary start queue is full"
    _assert_no_unguarded_batch_effect(audio_engine_mock)


def test_stop_cancels_deferred_original_group_and_stops_its_unobserved_targets(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _, bindings = _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0}
    audio_engine_mock.publish_captured_global_start.side_effect = lambda _capture: None
    controller.transport.playback.start_or_restart_global_start_stop()
    capture = _published_capture(audio_engine_mock)
    assert controller.session.active_sample_ids == set()

    controller.transport.playback.stop_all_pads()
    controller.transport.playback._global_playback.poll()

    assert capture.cancelled is True
    audio_engine_mock.capture_global_start.assert_called_once()
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(capture)
    audio_engine_mock.stop_global_playback_batch.assert_called_once_with(
        [bindings[0]], received_at_ns=None
    )
    assert controller.session.active_sample_ids == set()
    assert controller.session.global_stop_restore_sample_ids == set()
    _assert_no_unguarded_batch_effect(audio_engine_mock)


def test_deferred_global_failure_reports_current_native_diagnostic_slot_without_recapture(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    window = _pending_loop_edit(controller, audio_engine_mock)
    controller.transport.playback.start_or_restart_global_start_stop()
    capture = _published_capture(audio_engine_mock)
    capture.slots = [5]
    controller.transport._pair_deferred_sample_ids = frozenset({5})
    controller.transport.playback._global_playback.poll()
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(capture)
    controller.transport._pair_deferred_sample_ids = frozenset()
    window.status = "failed"
    window.failure = "original resident timing changed"

    controller.transport.playback._global_playback.poll()

    audio_engine_mock.capture_global_start.assert_called_once()
    assert audio_engine_mock.publish_captured_global_start.call_args_list == [call(capture)] * 2
    assert capture.state() == "rejected"
    assert controller.transport.residency.status(5)[1] == "original resident timing changed"
    assert window.cancelled is False


@pytest.mark.parametrize("reason", [None, "source_changed"])
def test_published_global_rejection_reports_actual_native_owner_after_pair_deferral(
    controller: AppController, audio_engine_mock: Mock, reason: str | None
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0}
    ticket = FakeGlobalPlaybackBatchTicket("pending")
    _set_start_ticket(audio_engine_mock, ticket)
    controller.transport.playback.start_or_restart_global_start_stop()
    capture = _published_capture(audio_engine_mock)
    capture.slots = [5]
    capture.status = "rejected"
    capture.failure = reason
    ticket.status = "rejected"
    controller.transport._pair_deferred_sample_ids = frozenset({5})

    controller.transport.playback._global_playback.poll()

    assert controller.transport.playback._global_playback._pending is not None
    assert controller.transport.residency.status(5)[1] is None
    controller.transport._pair_deferred_sample_ids = frozenset()
    controller.transport.playback._global_playback.poll()

    assert controller.transport.playback._global_playback._pending is None
    assert controller.transport.residency.status(5)[1] == (
        reason or "global playback start rejected"
    )
    assert controller.session.global_stop_engaged is True
    assert controller.session.global_stop_restore_sample_ids == {0}
    audio_engine_mock.capture_global_start.assert_called_once()
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(capture)
