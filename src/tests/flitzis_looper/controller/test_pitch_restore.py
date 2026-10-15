"""Saved pitch is passed to atomic source initialization, without a delayed SET event."""

from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import ProjectState

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.mark.parametrize(("base", "extra"), [(-5, -18), (6, 18), (0, 0)])
@pytest.mark.parametrize("finite", [False, True])
@pytest.mark.parametrize("key_lock", [False, True])
def test_saved_numeric_tuple_and_actual_lock_are_initialized_before_first_possible_trigger(
    controller: AppController,
    audio_engine_mock: Mock,
    base: int,
    extra: int,
    *,
    finite: bool,
    key_lock: bool,
) -> None:
    project = controller.project
    project.sample_paths[0] = "samples/restore-pitch.wav"
    project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(raw_key="unrecognized source label"),
        correction="retained unknown correction",
        base_shift=base,
        extra_shift=extra,
        retrigger=True,
    )
    project.key_lock = True
    project.pad_key_lock[0] = key_lock
    project.pad_loop_auto[0] = False
    project.pad_loop_start_s[0] = 3.0 if finite else 0.0
    project.pad_loop_end_s[0] = 5.0 if finite else None
    audio_engine_mock.load_sample_async.return_value = 9
    audio_engine_mock.capture_pad_pitch_action = Mock()
    audio_engine_mock.publish_pitch_action = Mock()
    before = project.pad_key_intent[0]

    assert controller.loader._schedule_restored_load(
        0, Path("samples/restore-pitch.wav"), run_analysis=False
    )

    options = audio_engine_mock.load_sample_async.call_args.kwargs
    assert options["source_intent"] == "restore"
    assert options["initial_base_shift"] == base
    assert options["initial_extra_shift"] == extra
    assert options["initial_key_lock"] is key_lock
    assert "retrigger" not in options
    if finite:
        assert options["resident_loop_start_s"] == 3.0
        assert options["resident_loop_end_s"] == 5.0
        assert options["resident_key_lock"] is key_lock
    else:
        assert "resident_loop_start_s" not in options
        assert "resident_loop_end_s" not in options
        assert "resident_key_lock" not in options
    assert project.pad_key_intent[0] == before
    assert not controller.transport.pitch._owners
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.publish_pitch_action.assert_not_called()


def test_legacy_unknown_manual_key_restores_neutral_tuple_without_retrigger(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    legacy = ProjectState.model_validate({"manual_key": ["legacy arbitrary key", *([None] * 215)]})
    controller.project.pad_key_intent[0] = legacy.pad_key_intent[0]
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_end_s[0] = None
    audio_engine_mock.load_sample_async.return_value = 9
    assert controller.loader._schedule_restored_load(
        0, Path("samples/legacy-pitch.wav"), run_analysis=False
    )
    options = audio_engine_mock.load_sample_async.call_args.kwargs
    assert options["initial_base_shift"] == 0
    assert options["initial_extra_shift"] == 0
    assert controller.project.pad_key_intent[0].correction == "legacy arbitrary key"
    assert controller.project.pad_key_intent[0].retrigger is False
    assert not controller.transport.pitch._owners
