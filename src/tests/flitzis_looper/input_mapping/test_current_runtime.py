"""Current native source and timing ownership in productive MIDI publication."""

from typing import TYPE_CHECKING

import pytest

from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding, current_timing_metadata

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _configure_accepted_pad(controller: AppController, audio: Mock) -> dict[str, object]:
    controller.project.sample_paths[0] = "samples/unchanged.wav"
    controller.project.sample_durations[0] = 12.0
    controller.project.pad_loop_auto[0] = True
    metadata = current_timing_metadata(period=0.500000000123, origin=-0.125)
    audio.current_constant_timing.return_value = metadata
    audio.pad_timing_intent.return_value = "automatic"
    return metadata


def test_runtime_region_and_accepted_identity_share_one_current_snapshot(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    metadata = _configure_accepted_pad(controller, audio_engine_mock)
    newer = dict(metadata, revision="later-revision", period_seconds_per_quarter=0.75)
    audio_engine_mock.current_constant_timing.side_effect = [metadata, newer]
    audio_engine_mock.reset_mock()

    controller.input_mapping._sync_rust_runtime_state()

    audio_engine_mock.current_constant_timing.assert_called_once_with(0)
    audio_engine_mock.current_input_runtime_pad_binding.assert_called_once_with(0)
    _, loaded, starts, ends, bindings = audio_engine_mock.set_input_runtime_state.call_args.args
    assert loaded[0] is True
    assert starts[0] == 0.0
    assert ends[0] == round(0.500000000123 * 32 * 48_000) / 48_000
    assert bindings[0].metadata()["accepted_timing"] == metadata
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


@pytest.mark.parametrize(
    ("changed_field", "new_value"),
    [
        ("revision", "same-period-new-accepted-revision"),
        ("accepted_request_id", 10),
        ("publication_epoch", 12),
        ("raw_revision", "another-raw-evidence"),
        ("pcm_sha256", "c" * 64),
        ("source_provenance", "another-original-association"),
        ("mono_revision", "another-mono-policy"),
        ("origin_provenance", "another-independent-origin-assertion"),
        ("acceptance_policy_version", "another-policy"),
        ("acceptance_provenance", "another-explicit-acceptance"),
    ],
)
def test_equal_numeric_runtime_republishes_complete_accepted_identity_changes(
    controller: AppController, audio_engine_mock: Mock, changed_field: str, new_value: object
) -> None:
    metadata = _configure_accepted_pad(controller, audio_engine_mock)
    controller.input_mapping._sync_rust_runtime_state()
    old_end = audio_engine_mock.set_input_runtime_state.call_args.args[3][0]
    audio_engine_mock.reset_mock()
    audio_engine_mock.current_constant_timing.return_value = dict(
        metadata, **{changed_field: new_value}
    )

    controller.input_mapping._sync_rust_runtime_state()

    audio_engine_mock.set_input_runtime_state.assert_called_once()
    _, loaded, _, ends, bindings = audio_engine_mock.set_input_runtime_state.call_args.args
    assert loaded[0] is True
    assert ends[0] == old_end
    assert bindings[0].metadata()["accepted_timing"][changed_field] == new_value
    audio_engine_mock.reset_mock()
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_not_called()


@pytest.mark.parametrize(
    ("changed_field", "new_value"),
    [
        ("source_id", "another-native-source"),
        ("source_generation", 2),
        ("source_sha256", "c" * 64),
        ("sample_rate_hz", 48_000),
        ("frame_count", 44_100 * 600 + 1),
        ("channels", 2),
        ("authority_revision", 2),
    ],
)
def test_unchanged_path_endpoints_and_bpm_do_not_hide_native_source_changes(
    controller: AppController, audio_engine_mock: Mock, changed_field: str, new_value: object
) -> None:
    controller.project.sample_paths[0] = "samples/unchanged.wav"
    controller.project.manual_bpm[0] = 120.0
    binding = FakeInputRuntimePadBinding()
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = binding
    controller.input_mapping._sync_rust_runtime_state()
    old_region = audio_engine_mock.set_input_runtime_state.call_args.args[2:4]
    replacement = FakeInputRuntimePadBinding()
    replacement._metadata[changed_field] = new_value
    audio_engine_mock.current_input_runtime_pad_binding.return_value = replacement
    audio_engine_mock.reset_mock()

    controller.input_mapping._sync_rust_runtime_state()

    audio_engine_mock.set_input_runtime_state.assert_called_once()
    assert audio_engine_mock.set_input_runtime_state.call_args.args[2:4] == old_region
    bindings = audio_engine_mock.set_input_runtime_state.call_args.args[4]
    assert bindings[0] is replacement


def test_capture_race_disables_direct_runtime_until_both_current_snapshots_agree(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    metadata = _configure_accepted_pad(controller, audio_engine_mock)
    newer = dict(metadata, revision="same-period-new-revision", publication_epoch=12)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = FakeInputRuntimePadBinding(
        accepted_timing=newer, intent="automatic"
    )

    controller.input_mapping._sync_rust_runtime_state()

    _, loaded, _, _, bindings = audio_engine_mock.set_input_runtime_state.call_args.args
    assert loaded[0] is False
    assert bindings[0] is None
    audio_engine_mock.reset_mock()
    audio_engine_mock.current_constant_timing.return_value = newer
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_called_once()
    assert audio_engine_mock.set_input_runtime_state.call_args.args[1][0] is True


def test_unavailable_automatic_authority_clears_runtime_without_legacy_replay(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _configure_accepted_pad(controller, audio_engine_mock)
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.reset_mock()
    audio_engine_mock.current_constant_timing.return_value = None

    controller.input_mapping._sync_rust_runtime_state()

    _, loaded, _, _, bindings = audio_engine_mock.set_input_runtime_state.call_args.args
    assert loaded[0] is False
    assert bindings[0] is None
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.reset_mock()
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_not_called()


@pytest.mark.parametrize("intent", ["manual", "tap", "legacy"])
def test_equal_value_manual_tap_legacy_intent_republishes_without_accepted_evidence(
    controller: AppController, audio_engine_mock: Mock, intent: str
) -> None:
    controller.project.sample_paths[0] = "samples/unchanged.wav"
    controller.project.manual_bpm[0] = 120.0
    controller.input_mapping._sync_rust_runtime_state()
    old_region = audio_engine_mock.set_input_runtime_state.call_args.args[2:4]
    audio_engine_mock.pad_timing_intent.return_value = intent
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = FakeInputRuntimePadBinding(
        intent=intent, authority_revision=2
    )
    audio_engine_mock.reset_mock()

    controller.input_mapping._sync_rust_runtime_state()

    audio_engine_mock.set_input_runtime_state.assert_called_once()
    assert audio_engine_mock.set_input_runtime_state.call_args.args[2:4] == old_region
    binding = audio_engine_mock.set_input_runtime_state.call_args.args[4][0]
    assert binding.metadata()["intent"] == intent
    assert binding.metadata()["accepted_timing"] is None
    audio_engine_mock.current_constant_timing.assert_not_called()


def test_missing_native_source_never_uses_the_saved_path_as_runtime_ownership(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/unchanged.wav"
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = None

    controller.input_mapping._sync_rust_runtime_state()

    assert audio_engine_mock.set_input_runtime_state.call_args.args[1][0] is False
    audio_engine_mock.reset_mock()
    audio_engine_mock.current_input_runtime_pad_binding.return_value = FakeInputRuntimePadBinding()
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_called_once()
    assert audio_engine_mock.set_input_runtime_state.call_args.args[1][0] is True


def test_rejected_publication_is_retried_and_never_launches_from_previous_runtime(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.sample_paths[0] = "samples/unchanged.wav"
    controller.input_mapping._sync_rust_runtime_state()
    controller.project.pad_loop_start_s[0] = 1.0
    audio_engine_mock.set_input_runtime_state.side_effect = RuntimeError("runtime source retired")
    audio_engine_mock.reset_mock()

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "received_at_ns": 123,
        "direct": True,
        "dispatched": False,
    })

    audio_engine_mock.set_input_runtime_state.assert_called_once()
    audio_engine_mock.trigger_input_runtime_pad.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.reset_mock()
    audio_engine_mock.set_input_runtime_state.side_effect = None
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_called_once()


@pytest.mark.parametrize("unavailable", [False, True])
def test_failed_guarded_trigger_never_falls_back_to_unbound_loop_and_play(
    controller: AppController, audio_engine_mock: Mock, *, unavailable: bool
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    _configure_accepted_pad(controller, audio_engine_mock)
    if unavailable:
        audio_engine_mock.current_constant_timing.return_value = None
    audio_engine_mock.trigger_input_runtime_pad.return_value = False
    audio_engine_mock.reset_mock()

    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "received_at_ns": 456,
        "direct": True,
        "dispatched": False,
    })

    audio_engine_mock.trigger_input_runtime_pad.assert_called_once_with(0, received_at_ns=456)
    assert audio_engine_mock.set_input_runtime_state.call_args.args[1][0] is not unavailable
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.play_sample.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()
