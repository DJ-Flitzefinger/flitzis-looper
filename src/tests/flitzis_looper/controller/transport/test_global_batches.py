"""Productive controller transactions use actual native current ownership."""

import math
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, call

import pytest

from flitzis_looper.models import BeatGrid, SampleAnalysis
from flitzis_looper_audio import AudioMessage
from tests.flitzis_looper.conftest import (
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
    return metadata, bindings


def _assert_no_unguarded_batch_effect(audio: Mock) -> None:
    audio.set_pad_loop_region.assert_not_called()
    audio.play_sample.assert_not_called()
    audio.play_sample_exclusive.assert_not_called()
    audio.stop_all.assert_not_called()
    audio.set_pad_bpm.assert_not_called()
    audio.set_pad_timing_metadata.assert_not_called()


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

    entries = audio_engine_mock.start_global_playback_batch.call_args.args[0]
    assert [binding for binding, _, _ in entries] == [bindings[0], bindings[2]]
    end_s = round(4.0 * PRECISE_PERIOD * 48_000) / 48_000
    assert [(start, end) for _, start, end in entries] == [(0.0, end_s), (0.0, end_s)]
    assert audio_engine_mock.start_global_playback_batch.call_args.kwargs == {
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
    audio_engine_mock.start_global_playback_batch.assert_not_called()
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
    audio_engine_mock.start_global_playback_batch.assert_not_called()
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

    audio_engine_mock.start_global_playback_batch.assert_not_called()


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
    audio_engine_mock.start_global_playback_batch.assert_not_called()
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    _assert_no_unguarded_batch_effect(audio_engine_mock)


@pytest.mark.parametrize("intent", ["manual", "tap", "legacy"])
def test_explicit_nonaccepted_authority_uses_native_binding_without_promoting_analysis(
    controller: AppController, audio_engine_mock: Mock, intent: TimingIntent
) -> None:
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

    entries = audio_engine_mock.start_global_playback_batch.call_args.args[0]
    assert entries[0][0] is binding
    assert binding.metadata()["accepted_timing"] is None
    assert entries[0][1:] == (0.0, round((4 * 60 / 135) * 44_100) / 44_100)
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
    audio_engine_mock.start_global_playback_batch.side_effect = RuntimeError("full command ring")
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
    audio_engine_mock.start_global_playback_batch.return_value = ticket
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
    audio_engine_mock.start_global_playback_batch.return_value = start
    audio_engine_mock.stop_global_playback_batch.return_value = stop
    before = controller.session.model_dump()
    controller.transport.playback.start_or_restart_global_start_stop()

    controller.transport.playback.stop_global_start_stop()
    controller.transport.playback.stop_all_pads()
    controller.transport.playback.start_or_restart_global_start_stop()

    audio_engine_mock.start_global_playback_batch.assert_called_once()
    audio_engine_mock.stop_global_playback_batch.assert_called_once_with(
        [bindings[0], bindings[2]], received_at_ns=None
    )
    assert controller.session.model_dump() == before
    _assert_no_unguarded_batch_effect(audio_engine_mock)

    stop.status = "accepted"
    controller.transport.playback.start_or_restart_global_start_stop()

    assert audio_engine_mock.start_global_playback_batch.call_count == 2
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
    audio_engine_mock.start_global_playback_batch.return_value = ticket
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
        audio_engine_mock.load_sample_async.return_value = 7
        controller.loader.load_sample_async(2, "replacement/source.wav")
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
            },
            None,
        ]
        controller.loader.poll_loader_events()
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
    audio_engine_mock.load_sample_async.return_value = 7
    controller.loader.load_sample_async(2, "replacement/source.wav")
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "error", "id": 2, "request_id": 7, "msg": "cold preparation failed"},
        None,
    ]
    controller.loader.poll_loader_events()
    ticket.status = "accepted"

    controller.transport.on_frame_render()

    assert controller.project.sample_paths[2] == "saved/stale-2.wav"
    assert controller.session.global_stop_restore_sample_ids == {0, 2}
    audio_engine_mock.unload_sample.assert_not_called()


def test_actual_app_drain_keeps_unloaded_pad_inactive_after_delayed_started_feedback(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    metadata, bindings = _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0}
    ticket = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.start_global_playback_batch.return_value = ticket
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
    audio_engine_mock.start_global_playback_batch.assert_called_once()
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
