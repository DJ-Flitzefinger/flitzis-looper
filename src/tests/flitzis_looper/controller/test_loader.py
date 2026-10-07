from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.stem_cache import (
    cache_dir_for_sample_id,
    expected_stem_files,
    source_version_for_sample_path,
)
from flitzis_looper.models import (
    STEM_COMPONENT_MASK,
    STEM_KINDS,
    STEM_MASK_VOCALS,
    BeatGrid,
    ProjectState,
    SampleAnalysis,
    SessionState,
    StemCacheEntry,
)
from flitzis_looper_audio import AudioEngine
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import write_test_stem_marker

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController


def _running_audio_engine_or_skip() -> AudioEngine:
    audio = AudioEngine()
    try:
        audio.run()
    except RuntimeError as exc:
        audio.shut_down()
        pytest.skip(f"AudioEngine unavailable: {exc}")
    return audio


def test_load_sample_async(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test scheduling a sample load updates state and calls audio engine."""
    sample_id = 0
    path = "/path/to/sample.wav"

    controller.loader.load_sample_async(sample_id, path)

    audio_engine_mock.load_sample_async.assert_called_with(
        sample_id, path, run_analysis=True, replace_assignment=True
    )
    assert controller.session.pending_sample_paths[sample_id] == path
    assert sample_id in controller.session.loading_sample_ids
    assert controller.project.sample_paths[sample_id] is None


def test_load_sample_async_preserves_existing_until_success(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Admission retains the previous complete assignment and native playback."""
    sample_id = 0
    old_path = "/path/to/old.wav"
    new_path = "/path/to/new.wav"

    controller.project.sample_paths[sample_id] = old_path
    controller.project.stem_cache[sample_id] = StemCacheEntry(
        source_version="old",
        cache_dir="samples/stems/old",
    )

    controller.loader.load_sample_async(sample_id, new_path)

    audio_engine_mock.unload_sample.assert_not_called()
    audio_engine_mock.load_sample_async.assert_called_with(
        sample_id, new_path, run_analysis=True, replace_assignment=True
    )
    assert controller.project.sample_paths[sample_id] == old_path
    assert controller.project.stem_cache[sample_id] is not None
    assert controller.session.pending_sample_paths[sample_id] == new_path


@pytest.mark.parametrize(
    "failure",
    [
        RuntimeError("cold queue full"),
        RuntimeError(LoaderController._COLD_QUEUE_FULL),
        ValueError("invalid source"),
    ],
)
def test_load_admission_failure_preserves_project_session_and_pending_request(
    controller: AppController, audio_engine_mock: Mock, failure: Exception
) -> None:
    project = controller.project
    session = controller.session
    project.sample_paths[0] = "samples/old.wav"
    project.sample_durations[0] = 600.0
    project.manual_bpm[0] = 123.5
    project.pad_timing_intent[0] = "automatic"
    project.pad_key_lock[0] = True
    project.pad_loop_start_s[0] = 321.25
    project.pad_loop_end_s[0] = 325.25
    project.stem_cache[0] = StemCacheEntry(source_version="old", cache_dir="samples/stems/#1")
    session.active_sample_ids.add(0)
    session.pending_sample_paths[0] = "samples/already-pending.wav"
    session.loading_sample_ids.add(0)
    session.sample_load_progress[0] = 0.5
    session.sample_load_stage[0] = "Decoding"
    session.waveform_editor_open = True
    session.waveform_editor_pad_id = 0
    controller.loader._load_request_ids[0] = 4
    previous_project = project.model_dump()
    previous_session = session.model_dump(exclude={"sample_load_errors"})
    audio_engine_mock.reset_mock()
    audio_engine_mock.load_sample_async.side_effect = failure

    controller.loader.load_sample_async(0, "missing-or-over-budget.wav")

    assert project.model_dump() == previous_project
    assert session.model_dump(exclude={"sample_load_errors"}) == previous_session
    assert controller.loader._load_request_ids[0] == 4
    assert not controller.loader._deferred_restores
    assert session.sample_load_errors[0] == str(failure)
    audio_engine_mock.unload_sample.assert_not_called()
    audio_engine_mock.set_pad_timing_intent.assert_not_called()
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.set_stem_mix_mode.assert_not_called()


@pytest.mark.parametrize(
    "failure_stage",
    ["snapshot", "decode", "PCM budget", "manifest", "commit", "native queue", "stale adoption"],
)
def test_async_cold_failure_preserves_complete_old_source_state(
    controller: AppController, audio_engine_mock: Mock, failure_stage: str
) -> None:
    project = controller.project
    session = controller.session
    project.sample_paths[0] = "samples/old.wav"
    project.sample_durations[0] = 600.0
    project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0, key="C", beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0], bars=[0.0])
    )
    project.pad_timing_intent[0] = "automatic"
    project.manual_key[0] = "Gm"
    project.pad_key_lock[0] = True
    project.pad_gain_db[0] = -6.0
    project.pad_loop_start_s[0] = 321.25
    project.pad_loop_end_s[0] = 325.25
    project.stem_cache[0] = StemCacheEntry(source_version="old", cache_dir="samples/stems/#1")
    session.active_sample_ids.add(0)
    session.paused_sample_ids.add(0)
    session.global_stop_restore_sample_ids.add(0)
    session.pad_playhead_s[0] = 323.75
    session.waveform_editor_open = True
    session.waveform_editor_pad_id = 0
    session.analyzing_sample_ids.add(0)
    session.stem_generating_sample_ids.add(0)
    session.sample_analysis_progress[0] = 0.25
    session.stem_generation_progress[0] = 0.75
    controller.loader._analysis_request_ids[0] = 3
    previous_project = project.model_dump()
    transient_load_fields = {
        "sample_load_errors",
        "sample_load_stage",
        "sample_load_progress",
        "pending_sample_paths",
        "loading_sample_ids",
    }
    previous_session = session.model_dump(exclude=transient_load_fields)
    audio_engine_mock.reset_mock()
    audio_engine_mock.load_sample_async.return_value = 7
    controller.loader.load_sample_async(0, "new.wav")
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "started", "id": 0, "request_id": 7},
        {"type": "progress", "id": 0, "request_id": 7, "stage": failure_stage, "percent": 0.5},
        {"type": "error", "id": 0, "request_id": 7, "msg": failure_stage},
        None,
    ]

    controller.loader.poll_loader_events()

    assert project.model_dump() == previous_project
    assert session.model_dump(exclude=transient_load_fields) == previous_session
    assert controller.loader._analysis_request_ids[0] == 3
    assert session.sample_load_errors[0] == failure_stage
    assert 0 not in session.loading_sample_ids
    audio_engine_mock.unload_sample.assert_not_called()
    audio_engine_mock.set_pad_timing_intent.assert_not_called()
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()


def test_successful_replacement_resets_retired_state_without_deleting_owned_files(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    old_original = tmp_path / "samples" / "old.wav"
    old_original.parent.mkdir(parents=True)
    old_original.write_bytes(b"byte-exact old original")
    old_stem = tmp_path / "samples" / "stems" / "#1" / "vocals.wav"
    old_stem.parent.mkdir(parents=True)
    old_stem.write_bytes(b"old complete stem owner")
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.sample_durations[0] = 600.0
    controller.project.manual_key[0] = "Gm"
    controller.project.pad_timing_intent[0] = "manual"
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version="old", cache_dir="samples/stems/#1", available=True
    )
    controller.session.active_sample_ids.add(0)
    controller.session.waveform_editor_open = True
    controller.session.waveform_editor_pad_id = 0
    audio_engine_mock.reset_mock()
    audio_engine_mock.load_sample_async.return_value = 7
    controller.loader.load_sample_async(0, "new.wav")
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 7,
            "cached_path": "samples/new.wav",
            "duration_s": 32.0,
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_paths[0] == "samples/new.wav"
    assert controller.project.sample_durations[0] == 32.0
    assert controller.project.manual_key[0] is None
    assert controller.project.stem_cache[0] is None
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert 0 not in controller.session.active_sample_ids
    assert controller.session.waveform_editor_open is False
    assert old_original.read_bytes() == b"byte-exact old original"
    assert old_stem.read_bytes() == b"old complete stem owner"
    audio_engine_mock.unload_sample.assert_not_called()
    audio_engine_mock.set_stem_mix_mode.assert_not_called()


def test_derived_refresh_queue_failure_keeps_matching_loaded_metadata(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.sample_durations[0] = 600.0
    audio_engine_mock.load_sample_async.return_value = 7
    controller.loader.load_sample_async(0, "new.wav")
    audio_engine_mock.set_pad_gain.side_effect = RuntimeError("parameter queue full")
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 7,
            "cached_path": "samples/new.wav",
            "duration_s": 32.0,
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_paths[0] == "samples/new.wav"
    assert controller.project.sample_durations[0] == 32.0
    assert (
        "control refresh failed: parameter queue full" in controller.session.sample_load_errors[0]
    )
    assert 0 not in controller.session.loading_sample_ids
    audio_engine_mock.unload_sample.assert_not_called()


def test_selected_same_project_path_is_new_assignment_after_native_success(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/same.wav"
    controller.project.manual_bpm[0] = 90.0
    controller.project.manual_key[0] = "Gm"
    controller.project.pad_loop_start_s[0] = 20.0
    audio_engine_mock.load_sample_async.return_value = 7
    controller.loader.load_sample_async(0, "samples/same.wav")
    assert controller.project.manual_bpm[0] == 90.0
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 7,
            "cached_path": "samples/same.wav",
            "duration_s": 32.0,
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_paths[0] == "samples/same.wav"
    assert controller.project.manual_bpm[0] is None
    assert controller.project.manual_key[0] is None
    assert controller.project.pad_loop_start_s[0] == 0.0
    assert controller.project.pad_loop_auto[0] is True
    audio_engine_mock.unload_sample.assert_not_called()


def test_restored_admission_failure_retains_assignment_and_timing_authority(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    original = tmp_path / "samples" / "old.wav"
    original.parent.mkdir(parents=True)
    original.write_bytes(b"existing original")
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.pad_timing_intent[0] = "automatic"
    previous = controller.project.model_dump()
    audio_engine_mock.reset_mock()
    audio_engine_mock.load_sample_async.side_effect = RuntimeError("source queue full")

    controller.loader.restore_samples_from_project_state()

    assert controller.project.model_dump() == previous
    assert controller.session.sample_load_errors[0] == "source queue full"
    assert 0 not in controller.session.loading_sample_ids
    audio_engine_mock.load_sample_async.assert_called_once_with(
        0,
        "samples/old.wav",
        run_analysis=False,
        restore_automatic=True,
        replace_assignment=True,
    )
    audio_engine_mock.set_pad_timing_intent.assert_not_called()
    audio_engine_mock.set_pad_bpm.assert_not_called()


def _finish_deferred_restore_polling(
    loader: LoaderController,
    audio: Mock,
    session: SessionState,
    inflight: dict[int, int],
    admitted_ids: list[int],
) -> None:
    for _ in range(NUM_SAMPLES):
        completed = dict(inflight)
        inflight.clear()
        audio.poll_loader_events.side_effect = [
            *(
                {
                    "type": "success",
                    "id": sample_id,
                    "request_id": request_id,
                    "cached_path": "samples/old.wav",
                    "duration_s": 600.0,
                }
                for sample_id, request_id in completed.items()
            ),
            None,
        ]
        previous_admissions = len(admitted_ids)
        loader.poll_loader_events()
        assert len(admitted_ids) - previous_admissions <= 8
        if not session.loading_sample_ids:
            return
    pytest.fail("Deferred startup admission did not finish")


def _populate_saved_automatic_sources(project: ProjectState, separator: str) -> None:
    for sample_id in range(NUM_SAMPLES):
        project.sample_paths[sample_id] = f"samples{separator}old.wav"
        project.sample_durations[sample_id] = 600.5
        project.sample_analysis[sample_id] = SampleAnalysis(
            bpm=120.0, key="C", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[])
        )
        project.pad_timing_intent[sample_id] = "automatic"
        project.pad_loop_start_s[sample_id] = 5.0
        project.pad_loop_end_s[sample_id] = 10.0
        project.pad_loop_auto[sample_id] = False
        project.pad_grid_anchor_s[sample_id] = 1.25
        project.pad_grid_offset_samples[sample_id] = -128
        project.pad_gain_db[sample_id] = -6.0
    project.bpm_lock = True


@pytest.mark.parametrize("separator", ["/", "\\"])
def test_startup_restores_all_pads_through_bounded_deferred_admission(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path, separator: str
) -> None:
    original = tmp_path / "samples" / "old.wav"
    original.parent.mkdir(parents=True)
    original.write_bytes(b"existing original")
    project = controller.project
    _populate_saved_automatic_sources(project, separator)
    previous_project = project.model_dump()
    inflight: dict[int, int] = {}
    admitted_ids: list[int] = []

    def admit(sample_id: int, _path: str, **_kwargs: object) -> int:
        if len(inflight) >= 32:
            raise RuntimeError(LoaderController._COLD_QUEUE_FULL)
        request_id = len(admitted_ids) + 1
        admitted_ids.append(sample_id)
        inflight[sample_id] = request_id
        return request_id

    audio_engine_mock.reset_mock()
    audio_engine_mock.load_sample_async.side_effect = admit
    audio_engine_mock.poll_loader_events.return_value = None

    controller.loader.restore_samples_from_project_state()
    controller.transport.apply_project_state_to_audio()

    assert len(inflight) == 32
    assert len(controller.loader._deferred_restores) == NUM_SAMPLES - 32
    assert len(controller.session.loading_sample_ids) == NUM_SAMPLES
    assert audio_engine_mock.load_sample_async.call_count == 33
    assert project.model_dump() == previous_project
    assert not controller.session.sample_load_errors
    assert controller.session.sample_load_stage[32] == "Waiting for cold source admission"
    assert controller.transport.bpm.current_timing(32) is None
    assert controller.transport.bpm.effective_bpm(32) is None
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.set_master_period.assert_not_called()

    controller.loader.poll_loader_events()
    assert audio_engine_mock.load_sample_async.call_count == 34
    assert len(controller.loader._deferred_restores) == NUM_SAMPLES - 32

    _finish_deferred_restore_polling(
        controller.loader, audio_engine_mock, controller.session, inflight, admitted_ids
    )

    assert admitted_ids == list(range(NUM_SAMPLES))
    assert not controller.loader._deferred_restores
    assert not controller.loader._load_request_ids
    assert not controller.session.pending_sample_paths
    assert not controller.session.sample_load_errors
    previous_project["sample_durations"] = [600.0] * NUM_SAMPLES
    previous_project["sample_paths"] = ["samples/old.wav"] * NUM_SAMPLES
    assert project.model_dump() == previous_project
    audio_engine_mock.unload_sample.assert_not_called()


@pytest.mark.parametrize("action", ["new_selection", "unload", "shutdown", "path_change"])
def test_deferred_startup_restore_cannot_replace_later_user_assignment(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path, action: str
) -> None:
    original = tmp_path / "samples" / "old.wav"
    original.parent.mkdir(parents=True)
    original.write_bytes(b"existing original")
    controller.project.sample_paths[0] = "samples/old.wav"
    audio_engine_mock.load_sample_async.side_effect = RuntimeError(
        LoaderController._COLD_QUEUE_FULL
    )
    audio_engine_mock.poll_loader_events.return_value = None
    controller.loader.restore_samples_from_project_state()
    assert 0 in controller.loader._deferred_restores
    audio_engine_mock.load_sample_async.side_effect = None
    audio_engine_mock.load_sample_async.return_value = 9

    if action == "new_selection":
        controller.loader.load_sample_async(0, "new.wav")
    elif action == "unload":
        controller.loader.unload_sample(0)
    elif action == "shutdown":
        controller.loader.shut_down()
    else:
        controller.project.sample_paths[0] = "samples/later.wav"
    audio_engine_mock.load_sample_async.reset_mock()
    controller.loader.poll_loader_events()

    audio_engine_mock.load_sample_async.assert_not_called()
    assert not controller.loader._deferred_restores
    if action == "new_selection":
        assert controller.session.pending_sample_paths[0] == "new.wav"
        assert controller.loader._load_request_ids[0] == 9
        assert 0 in controller.session.loading_sample_ids
    else:
        assert 0 not in controller.session.loading_sample_ids
        assert 0 not in controller.session.pending_sample_paths


def test_deferred_startup_restore_treats_stopped_lane_as_terminal_failure(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    original = tmp_path / "samples" / "old.wav"
    original.parent.mkdir(parents=True)
    original.write_bytes(b"existing original")
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.pad_timing_intent[0] = "automatic"
    previous_project = controller.project.model_dump()
    audio_engine_mock.load_sample_async.side_effect = RuntimeError(
        LoaderController._COLD_QUEUE_FULL
    )
    audio_engine_mock.poll_loader_events.return_value = None
    controller.loader.restore_samples_from_project_state()
    audio_engine_mock.load_sample_async.side_effect = RuntimeError("cold source lane stopped")

    controller.loader.poll_loader_events()

    assert not controller.loader._deferred_restores
    assert 0 not in controller.session.loading_sample_ids
    assert 0 not in controller.session.pending_sample_paths
    assert controller.session.sample_load_errors[0] == "cold source lane stopped"
    assert controller.project.model_dump() == previous_project
    assert controller.transport.bpm.current_timing(0) is None
    audio_engine_mock.load_sample_async.reset_mock()
    controller.loader.poll_loader_events()
    audio_engine_mock.load_sample_async.assert_not_called()


def test_unload_admission_failure_preserves_source_session_and_deferred_restore(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    original = tmp_path / "samples" / "old.wav"
    original.parent.mkdir(parents=True)
    original.write_bytes(b"existing original")
    stem_file = original.parent / "stems" / "old" / "vocals.wav"
    stem_file.parent.mkdir(parents=True)
    stem_file.write_bytes(b"old stem")
    project = controller.project
    session = controller.session
    project.sample_paths[0] = "samples/old.wav"
    project.pad_timing_intent[0] = "automatic"
    project.pad_gain_db[0] = -6.0
    project.pad_loop_start_s[0] = 5.0
    project.pad_loop_end_s[0] = 10.0
    project.stem_cache[0] = StemCacheEntry(source_version="old", cache_dir="samples/stems/old")
    session.active_sample_ids.add(0)
    session.pad_playhead_s[0] = 7.5
    session.waveform_editor_open = True
    session.waveform_editor_pad_id = 0
    audio_engine_mock.load_sample_async.side_effect = RuntimeError(
        LoaderController._COLD_QUEUE_FULL
    )
    controller.loader.restore_samples_from_project_state()
    previous_project = project.model_dump()
    previous_session = session.model_dump()
    previous_deferred = dict(controller.loader._deferred_restores)
    cancelled = Mock()
    stems_deleted = Mock()
    unloaded = Mock()
    bpm_changed = Mock()
    monkeypatch.setattr(controller.loader._accepted_restore, "cancel", cancelled)
    monkeypatch.setattr(controller.loader, "_on_stems_deleted", stems_deleted)
    monkeypatch.setattr(controller.loader, "_on_sample_unloaded", unloaded)
    monkeypatch.setattr(controller.loader, "_on_pad_bpm_changed", bpm_changed)
    audio_engine_mock.unload_sample.side_effect = RuntimeError("native unload queue full")

    with pytest.raises(RuntimeError, match="native unload queue full"):
        controller.loader.unload_sample(0)

    assert project.model_dump() == previous_project
    assert session.model_dump() == previous_session
    assert controller.loader._deferred_restores == previous_deferred
    assert original.read_bytes() == b"existing original"
    assert stem_file.read_bytes() == b"old stem"
    cancelled.assert_not_called()
    stems_deleted.assert_not_called()
    unloaded.assert_not_called()
    bpm_changed.assert_not_called()


def test_load_success_resets_stale_empty_pad_settings(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """An acknowledged new source resets stale empty-pad settings at completion."""
    sample_id = 0
    defaults = ProjectState()
    controller.project.manual_bpm[sample_id] = 123.0
    controller.project.manual_key[sample_id] = "Gm"
    controller.project.pad_key_lock[sample_id] = True
    controller.project.pad_gain_db[sample_id] = -6.0
    controller.project.pad_eq_low_db[sample_id] = 1.0
    controller.project.pad_eq_mid_db[sample_id] = -2.0
    controller.project.pad_eq_high_db[sample_id] = 3.0
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_start_s[sample_id] = 5.0
    controller.project.pad_loop_end_s[sample_id] = 10.0
    controller.project.pad_loop_bars[sample_id] = 2.0
    controller.project.pad_grid_offset_samples[sample_id] = -240
    controller.project.pad_grid_anchor_s[sample_id] = 0.024

    old_project = controller.project.model_dump()
    audio_engine_mock.load_sample_async.return_value = 1
    controller.loader.load_sample_async(sample_id, "/path/to/new.wav")
    assert controller.project.model_dump() == old_project
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": sample_id,
            "request_id": 1,
            "cached_path": "samples/new.wav",
            "duration_s": 32.0,
        },
        None,
    ]
    controller.loader.poll_loader_events()

    assert controller.project.manual_bpm[sample_id] == defaults.manual_bpm[sample_id]
    assert controller.project.manual_key[sample_id] == defaults.manual_key[sample_id]
    assert controller.project.pad_key_lock[sample_id] == defaults.pad_key_lock[sample_id]
    assert controller.project.pad_gain_db[sample_id] == defaults.pad_gain_db[sample_id]
    assert controller.project.pad_eq_low_db[sample_id] == defaults.pad_eq_low_db[sample_id]
    assert controller.project.pad_eq_mid_db[sample_id] == defaults.pad_eq_mid_db[sample_id]
    assert controller.project.pad_eq_high_db[sample_id] == defaults.pad_eq_high_db[sample_id]
    assert controller.project.pad_loop_auto[sample_id] is True
    assert controller.project.pad_loop_start_s[sample_id] == defaults.pad_loop_start_s[sample_id]
    assert controller.project.pad_loop_end_s[sample_id] == defaults.pad_loop_end_s[sample_id]
    assert controller.project.pad_loop_bars[sample_id] == defaults.pad_loop_bars[sample_id]
    assert controller.project.pad_grid_anchor_s[sample_id] is None
    assert (
        controller.project.pad_grid_offset_samples[sample_id]
        == defaults.pad_grid_offset_samples[sample_id]
    )
    audio_engine_mock.set_pad_bpm.assert_called_with(sample_id, None)
    audio_engine_mock.set_pad_gain.assert_called_with(sample_id, defaults.pad_gain_db[sample_id])
    audio_engine_mock.set_pad_eq.assert_called_with(sample_id, 0.0, 0.0, 0.0)
    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, 0.0, None)
    disabled = False
    audio_engine_mock.set_pad_key_lock.assert_called_with(sample_id, disabled)
    audio_engine_mock.load_sample_async.assert_called_with(
        sample_id, "/path/to/new.wav", run_analysis=True, replace_assignment=True
    )


def test_loader_success_updates_project_sample_path(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.session.pending_sample_paths[0] = "/path/to/original.wav"

    analysis = {
        "bpm": 120.0,
        "key": "C#m",
        "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
    }

    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "duration_s": 1.0,
            "cached_path": "samples/foo.wav",
            "analysis": analysis,
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_paths[0] == "samples/foo.wav"
    assert controller.project.sample_durations[0] == 1.0
    assert controller.project.sample_analysis[0] is not None
    assert 0 not in controller.session.pending_sample_paths


def test_stale_loader_success_with_old_request_id_is_ignored(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.load_sample_async.return_value = 2
    controller.loader.load_sample_async(0, "/path/to/current.wav")

    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 1,
            "duration_s": 1.0,
            "cached_path": "samples/old.wav",
            "detected_loop_start_s": 0.75,
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_paths[0] is None
    assert controller.project.sample_durations[0] is None
    assert controller.project.pad_loop_start_s[0] == 0.0
    assert controller.session.pending_sample_paths[0] == "/path/to/current.wav"
    assert 0 in controller.session.loading_sample_ids


def test_stale_loader_progress_after_unload_is_ignored(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.load_sample_async.return_value = 7
    controller.loader.load_sample_async(0, "/path/to/current.wav")

    controller.loader.unload_sample(0)

    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "progress", "id": 0, "request_id": 7, "stage": "Decoding", "percent": 0.5},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.loading_sample_ids
    assert 0 not in controller.session.sample_load_progress
    assert 0 not in controller.session.sample_load_stage


def test_loader_success_initializes_new_sample_loop_defaults(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.session.pending_sample_paths[0] = "/path/to/original.wav"
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 5.0
    controller.project.pad_loop_end_s[0] = 10.0
    controller.project.pad_loop_bars[0] = 2.0

    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "duration_s": 32.0,
            "cached_path": "samples/foo.wav",
            "analysis": {
                "bpm": 120.0,
                "key": "C#m",
                "beat_grid": {"beats": [2.0, 2.5], "downbeats": [2.0], "bars": [2.0]},
            },
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.pad_loop_auto[0] is True
    assert controller.project.pad_loop_bars[0] == 8.0
    assert controller.project.pad_loop_start_s[0] == pytest.approx(0.0)
    assert controller.project.pad_loop_end_s[0] is None
    audio_engine_mock.set_pad_loop_region.assert_called_with(0, 0.0, 16.0)


@pytest.mark.parametrize("anchor_s", [None, 0.0, 1_151 / 48_000])
def test_restored_sample_success_preserves_existing_loop_settings(
    controller: AppController,
    audio_engine_mock: Mock,
    anchor_s: float | None,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.session.pending_sample_paths[0] = "samples/foo.wav"
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 5.0
    controller.project.pad_loop_end_s[0] = 10.0
    controller.project.pad_loop_bars[0] = 2.0
    controller.project.pad_grid_anchor_s[0] = anchor_s

    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "duration_s": 32.0,
            "cached_path": "samples/foo.wav",
            "detected_loop_start_s": 1.0,
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.pad_loop_auto[0] is False
    assert controller.project.pad_loop_bars[0] == 2.0
    assert controller.project.pad_loop_start_s[0] == pytest.approx(5.0)
    assert controller.project.pad_loop_end_s[0] == pytest.approx(10.0)
    assert controller.project.pad_grid_anchor_s[0] == anchor_s
    if anchor_s is not None:
        audio_engine_mock.set_pad_timing_metadata.assert_called_once_with(0, anchor_s)


@pytest.mark.parametrize("analysis_bpm", [None, 123.456])
def test_loader_success_initializes_activity_candidate_with_or_without_bpm(
    controller: AppController,
    audio_engine_mock: Mock,
    analysis_bpm: float | None,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    audio_engine_mock.load_sample_async.return_value = 3
    controller.loader.load_sample_async(0, "/path/to/original.wav")
    start_s = 1_151 / 48_000
    analysis = (
        {
            "bpm": analysis_bpm,
            "key": "C#m",
            "beat_grid": {"beats": [2.0, 2.5], "downbeats": [2.0], "bars": [2.0]},
        }
        if analysis_bpm is not None
        else None
    )
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 3,
            "duration_s": 3.0,
            "cached_path": "samples/foo.wav",
            "detected_loop_start_s": start_s,
            "analysis": analysis,
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.pad_loop_start_s[0] == start_s
    assert controller.project.pad_loop_auto[0] is True
    assert controller.project.pad_loop_bars[0] == 8.0
    assert controller.project.pad_grid_anchor_s[0] == start_s
    assert controller.transport.loop.grid_anchor_sec(0) == start_s
    assert controller.project.pad_grid_offset_samples[0] == 0
    audio_engine_mock.set_pad_timing_metadata.assert_called_with(0, start_s)
    expected_end_s = (
        round((start_s + 32 * 60 / analysis_bpm) * 48_000) / 48_000
        if analysis_bpm is not None
        else None
    )
    audio_engine_mock.set_pad_loop_region.assert_called_with(0, start_s, expected_end_s)


@pytest.mark.parametrize("candidate", [None, "invalid", True, float("nan"), -0.1, 3.0])
def test_loader_success_invalid_activity_candidate_keeps_track_start_fallback(
    controller: AppController,
    audio_engine_mock: Mock,
    candidate: object,
) -> None:
    controller.session.pending_sample_paths[0] = "/path/to/original.wav"
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "duration_s": 3.0,
            "cached_path": "samples/foo.wav",
            "detected_loop_start_s": candidate,
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.pad_loop_start_s[0] == 0.0
    assert controller.project.pad_loop_auto[0] is True
    assert controller.project.pad_loop_bars[0] == 8.0
    assert controller.project.pad_grid_anchor_s[0] is None


def test_manual_reanalysis_does_not_apply_activity_candidate_or_move_markers(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.project.sample_durations[0] = 32.0
    controller.project.manual_bpm[0] = 123.456
    controller.project.pad_grid_offset_samples[0] = -240
    controller.project.pad_grid_anchor_s[0] = 1_151 / 48_000
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 5.0
    controller.project.pad_loop_end_s[0] = 10.0
    audio_engine_mock.analyze_sample_async.return_value = 7
    controller.loader.analyze_sample_async(0)
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "task_success",
            "task": "analysis",
            "id": 0,
            "request_id": 7,
            "detected_loop_start_s": 1.0,
            "analysis": {
                "bpm": 120.0,
                "key": "C#m",
                "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
            },
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.pad_loop_start_s[0] == 5.0
    assert controller.project.pad_loop_end_s[0] == 10.0
    assert controller.project.pad_loop_auto[0] is False
    assert controller.project.manual_bpm[0] == 123.456
    assert controller.project.pad_grid_offset_samples[0] == -240
    assert controller.project.pad_grid_anchor_s[0] == 1_151 / 48_000
    audio_engine_mock.set_pad_timing_metadata.assert_called_with(0, 911 / 48_000)


def test_same_assignment_load_success_preserves_base_when_new_analysis_arrives(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    controller.project.sample_paths[0] = "samples/foo.wav"
    controller.project.pad_grid_anchor_s[0] = 1_151 / 48_000
    controller.project.pad_grid_offset_samples[0] = -240
    controller.project.pad_loop_start_s[0] = 4.0
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "duration_s": 32.0,
            "cached_path": "samples/foo.wav",
            "detected_loop_start_s": 1.0,
            "analysis": {
                "bpm": 120.0,
                "key": "C#m",
                "beat_grid": {"beats": [2.0], "downbeats": [2.0], "bars": [2.0]},
            },
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.pad_loop_start_s[0] == 4.0
    assert controller.project.pad_grid_anchor_s[0] == 1_151 / 48_000
    assert controller.project.pad_grid_offset_samples[0] == -240
    audio_engine_mock.set_pad_timing_metadata.assert_called_once_with(0, 911 / 48_000)


def test_restored_sample_success_publishes_available_stems(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
) -> None:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_path = samples_dir / "loop.wav"
    write_mono_pcm16_wav(sample_path, 44_100)
    controller.project.sample_paths[0] = "samples/loop.wav"
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    controller.session.pending_sample_paths[0] = "samples/loop.wav"
    controller.session.loading_sample_ids.add(0)

    source_version = source_version_for_sample_path("samples/loop.wav")
    assert source_version is not None
    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")
    write_test_stem_marker(stems_dir, source_version)
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=source_version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )

    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "duration_s": 32.0,
            "cached_path": "samples/loop.wav",
        },
        None,
    ]

    controller.loader.poll_loader_events()

    audio_engine_mock.publish_prepared_stems.assert_called_once_with(
        0, source_version, cache_dir, audio_engine_mock.capture_prepared_source.return_value
    )
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "all_stems", source_version)
    audio_engine_mock.set_stem_enabled_mask.assert_called_once_with(
        0, STEM_COMPONENT_MASK, source_version
    )
    assert controller.project.stem_cache[0] is not None
    assert controller.project.stem_cache[0].available is True


def test_restored_sample_success_marks_stems_unavailable_when_publication_fails(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
) -> None:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_path = samples_dir / "loop.wav"
    write_mono_pcm16_wav(sample_path, 44_100)
    controller.project.sample_paths[0] = "samples/loop.wav"
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    controller.session.pending_sample_paths[0] = "samples/loop.wav"
    controller.session.loading_sample_ids.add(0)

    source_version = source_version_for_sample_path("samples/loop.wav")
    assert source_version is not None
    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")
    write_test_stem_marker(stems_dir, source_version)
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=source_version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )
    audio_engine_mock.publish_prepared_stems.side_effect = RuntimeError("buffer may be full")
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "duration_s": 32.0,
            "cached_path": "samples/loop.wav",
        },
        None,
    ]

    controller.loader.poll_loader_events()

    audio_engine_mock.publish_prepared_stems.assert_called_once_with(
        0, source_version, cache_dir, audio_engine_mock.capture_prepared_source.return_value
    )
    audio_engine_mock.set_stem_mix_mode.assert_not_called()
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False
    assert "Restored stem publication failed" in controller.session.stem_generation_errors[0]


def test_unload_sample(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test unloading a sample stops playback and clears state."""
    sample_id = 0
    path = "/path/to/sample.wav"
    controller.project.sample_paths[sample_id] = path
    controller.project.sample_durations[sample_id] = 1.0
    controller.project.stem_cache[sample_id] = StemCacheEntry(
        source_version="old",
        cache_dir="samples/stems/old",
    )
    controller.session.active_sample_ids.add(sample_id)
    controller.session.stem_generating_sample_ids.add(sample_id)

    controller.loader.unload_sample(sample_id)

    audio_engine_mock.unload_sample.assert_called_with(sample_id)
    assert controller.project.sample_paths[sample_id] is None
    assert controller.project.sample_durations[sample_id] is None
    assert controller.project.stem_cache[sample_id] is None
    assert sample_id not in controller.session.active_sample_ids
    assert sample_id not in controller.session.stem_generating_sample_ids


def test_unload_sample_closes_waveform_editor_for_unloaded_pad(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Unloading the edited pad returns the center surface to the pad view."""
    sample_id = 0
    controller.project.sample_paths[sample_id] = "/path/to/sample.wav"
    controller.session.waveform_editor_open = True
    controller.session.waveform_editor_pad_id = sample_id

    controller.loader.unload_sample(sample_id)

    assert controller.session.waveform_editor_open is False
    assert controller.session.waveform_editor_pad_id is None
    audio_engine_mock.unload_sample.assert_called_once_with(sample_id)


def test_unload_sample_keeps_waveform_editor_for_different_pad(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Unloading another pad does not close an unrelated editor."""
    sample_id = 0
    edited_pad_id = 1
    controller.project.sample_paths[sample_id] = "/path/to/sample.wav"
    controller.project.sample_paths[edited_pad_id] = "/path/to/other.wav"
    controller.session.waveform_editor_open = True
    controller.session.waveform_editor_pad_id = edited_pad_id

    controller.loader.unload_sample(sample_id)

    assert controller.session.waveform_editor_open is True
    assert controller.session.waveform_editor_pad_id == edited_pad_id
    audio_engine_mock.unload_sample.assert_called_once_with(sample_id)


def test_unload_sample_resets_track_bound_pad_settings(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Unloading clears the persisted settings that belong to the previous track."""
    sample_id = 0
    defaults = ProjectState()
    controller.project.sample_paths[sample_id] = "/path/to/sample.wav"
    controller.project.sample_durations[sample_id] = 42.0
    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=123.0,
        key="C#m",
        beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0], bars=[0.0]),
    )
    controller.project.stem_cache[sample_id] = StemCacheEntry(
        source_version="old",
        cache_dir="samples/stems/old",
    )
    controller.project.pad_stem_mix_mode[sample_id] = "all_stems"
    controller.project.pad_key_lock[sample_id] = True
    controller.project.manual_bpm[sample_id] = 128.0
    controller.project.manual_key[sample_id] = "Gm"
    controller.project.pad_gain_db[sample_id] = -9.0
    controller.project.pad_eq_low_db[sample_id] = 1.5
    controller.project.pad_eq_mid_db[sample_id] = -3.0
    controller.project.pad_eq_high_db[sample_id] = 4.5
    controller.project.pad_loop_auto[sample_id] = True
    controller.project.pad_loop_start_s[sample_id] = 6.0
    controller.project.pad_loop_end_s[sample_id] = 14.0
    controller.project.pad_loop_bars[sample_id] = 2.0
    controller.project.pad_grid_offset_samples[sample_id] = 512
    controller.project.pad_grid_anchor_s[sample_id] = 0.024

    controller.loader.unload_sample(sample_id)

    assert controller.project.sample_paths[sample_id] == defaults.sample_paths[sample_id]
    assert controller.project.sample_durations[sample_id] == defaults.sample_durations[sample_id]
    assert controller.project.sample_analysis[sample_id] == defaults.sample_analysis[sample_id]
    assert controller.project.stem_cache[sample_id] == defaults.stem_cache[sample_id]
    assert controller.project.pad_stem_mix_mode[sample_id] == defaults.pad_stem_mix_mode[sample_id]
    assert controller.project.pad_key_lock[sample_id] == defaults.pad_key_lock[sample_id]
    assert controller.project.manual_bpm[sample_id] == defaults.manual_bpm[sample_id]
    assert controller.project.manual_key[sample_id] == defaults.manual_key[sample_id]
    assert controller.project.pad_gain_db[sample_id] == defaults.pad_gain_db[sample_id]
    assert controller.project.pad_eq_low_db[sample_id] == defaults.pad_eq_low_db[sample_id]
    assert controller.project.pad_eq_mid_db[sample_id] == defaults.pad_eq_mid_db[sample_id]
    assert controller.project.pad_eq_high_db[sample_id] == defaults.pad_eq_high_db[sample_id]
    assert controller.project.pad_loop_auto[sample_id] == defaults.pad_loop_auto[sample_id]
    assert controller.project.pad_loop_start_s[sample_id] == defaults.pad_loop_start_s[sample_id]
    assert controller.project.pad_loop_end_s[sample_id] == defaults.pad_loop_end_s[sample_id]
    assert controller.project.pad_loop_bars[sample_id] == defaults.pad_loop_bars[sample_id]
    assert controller.project.pad_grid_anchor_s[sample_id] is None
    assert (
        controller.project.pad_grid_offset_samples[sample_id]
        == defaults.pad_grid_offset_samples[sample_id]
    )
    audio_engine_mock.unload_sample.assert_called_once_with(sample_id)
    audio_engine_mock.set_pad_bpm.assert_called_with(sample_id, None)
    audio_engine_mock.set_pad_gain.assert_called_with(sample_id, defaults.pad_gain_db[sample_id])
    audio_engine_mock.set_pad_eq.assert_called_with(sample_id, 0.0, 0.0, 0.0)
    audio_engine_mock.set_pad_loop_region.assert_called_with(sample_id, 0.0, None)
    disabled = False
    audio_engine_mock.set_pad_key_lock.assert_called_with(sample_id, disabled)


def test_unload_sample_with_negative_grid_offset_does_not_publish_invalid_timing_metadata(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Regression: unloaded pads must not publish stale negative grid anchors to Rust."""
    audio_engine_mock.output_sample_rate.return_value = 44_100
    audio_engine_mock.set_pad_timing_metadata.side_effect = ValueError(
        "phase_anchor_s out of range"
    )
    sample_id = 0
    controller.project.sample_paths[sample_id] = "samples/loop.wav"
    controller.project.pad_grid_offset_samples[sample_id] = -1

    controller.loader.unload_sample(sample_id)

    audio_engine_mock.unload_sample.assert_called_once_with(sample_id)
    audio_engine_mock.set_pad_bpm.assert_called_with(sample_id, None)
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_is_sample_loaded_true(controller: AppController) -> None:
    """Test is_sample_loaded returns True when sample is loaded."""
    sample_id = 0
    path = "/path/to/sample.wav"

    controller.project.sample_paths[sample_id] = path

    assert controller.loader.is_sample_loaded(sample_id) is True


def test_is_sample_loaded_false(controller: AppController) -> None:
    """Test is_sample_loaded returns False when sample is not loaded."""
    sample_id = 0

    assert controller.loader.is_sample_loaded(sample_id) is False


@pytest.mark.audio_device
def test_restore_sample_does_not_copy_file(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(tmp_path)

    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_filepath = samples_dir / "test.wav"
    write_mono_pcm16_wav(sample_filepath, 44_100)

    project = ProjectState()
    session = SessionState()
    project.sample_paths[0] = "samples/test.wav"

    audio = _running_audio_engine_or_skip()

    try:
        loader = LoaderController(
            project=project,
            session=session,
            audio=audio,
            on_pad_bpm_changed=lambda _: None,
        )
        loader.restore_samples_from_project_state()
        while loader.is_sample_loading(0):
            loader.poll_loader_events()

        assert loader.is_sample_loaded(0)
        sample_files = [path for path in samples_dir.iterdir() if path.is_file()]
        assert len(sample_files) == 1
        assert sample_files[0].name == "test.wav"
        assert project.sample_paths[0] == "samples/test.wav"
        # Resampling quantizes the 128-frame fixture to whole output frames.
        assert project.sample_durations[0] == pytest.approx(
            128 / 44_100, abs=1 / audio.output_sample_rate()
        )

    finally:
        audio.shut_down()


@pytest.mark.audio_device
def test_load_new_sample_copies_file(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(tmp_path)

    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    source_dir = tmp_path / "source"
    source_dir.mkdir()

    source_filepath = source_dir / "test.wav"
    write_mono_pcm16_wav(source_filepath, 44_100)

    # Create project and session state
    project = ProjectState()
    session = SessionState()

    # Create audio engine and loader
    audio = _running_audio_engine_or_skip()

    try:
        loader = LoaderController(
            project=project,
            session=session,
            audio=audio,
            on_pad_bpm_changed=lambda _: None,
        )
        loader.restore_samples_from_project_state()

        loader.load_sample_async(0, source_filepath.as_posix())
        while loader.is_sample_loading(0):
            loader.poll_loader_events()

        assert loader.is_sample_loaded(0)
        sample_files = [path for path in samples_dir.iterdir() if path.is_file()]
        assert len(sample_files) == 1
        assert sample_files[0].name == "test.wav"
        assert project.sample_paths[0] == "samples/test.wav"
        # Resampling quantizes the 128-frame fixture to whole output frames.
        assert project.sample_durations[0] == pytest.approx(
            128 / 44_100, abs=1 / audio.output_sample_rate()
        )

    finally:
        audio.shut_down()


def test_invalid_sample_id_too_low(controller: AppController) -> None:
    """Test that invalid sample ID below 0 raises ValueError."""
    invalid_id = -1

    with pytest.raises(ValueError, match="sample_id must be"):
        controller.loader.load_sample_async(invalid_id, "/path/to/sample.wav")


def test_invalid_sample_id_too_high(controller: AppController) -> None:
    """Test that invalid sample ID above NUM_SAMPLES raises ValueError."""
    invalid_id = NUM_SAMPLES

    with pytest.raises(ValueError, match="sample_id must be"):
        controller.loader.load_sample_async(invalid_id, "/path/to/sample.wav")


def test_task_started_sets_analyzing_state(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 in controller.session.analyzing_sample_ids
    assert 0 not in controller.session.sample_analysis_progress
    assert 0 not in controller.session.sample_analysis_stage
    assert 0 not in controller.session.sample_analysis_errors


def test_task_progress_updates_stage_and_percent(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        {
            "type": "task_progress",
            "id": 0,
            "task": "analysis",
            "percent": 0.25,
            "stage": "Analyzing",
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.session.sample_analysis_progress[0] == 0.25
    assert controller.session.sample_analysis_stage[0] == "Analyzing"


def test_task_success_stores_analysis_and_clears_task_state(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    analysis = {
        "bpm": 120.0,
        "key": "C#m",
        "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
    }

    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        {"type": "task_success", "id": 0, "task": "analysis", "analysis": analysis},
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_analysis[0] is not None
    assert controller.project.sample_analysis[0].bpm == 120.0
    assert controller.project.sample_analysis[0].key == "C#m"
    assert 0 not in controller.session.analyzing_sample_ids
    assert 0 not in controller.session.sample_analysis_progress
    assert 0 not in controller.session.sample_analysis_stage
    assert 0 not in controller.session.sample_analysis_errors


def test_stale_analysis_success_after_unload_is_ignored(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    audio_engine_mock.analyze_sample_async.return_value = 5
    controller.loader.analyze_sample_async(0)

    controller.loader.unload_sample(0)

    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "task_success",
            "id": 0,
            "request_id": 5,
            "task": "analysis",
            "analysis": {
                "bpm": 120.0,
                "key": "C#m",
                "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
            },
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_paths[0] is None
    assert controller.project.sample_analysis[0] is None
    assert 0 not in controller.session.analyzing_sample_ids


def test_task_error_records_error_and_clears_progress(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        {"type": "task_progress", "id": 0, "task": "analysis", "percent": 0.5},
        {"type": "task_error", "id": 0, "task": "analysis", "msg": "bad audio"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.analyzing_sample_ids
    assert 0 not in controller.session.sample_analysis_progress
    assert 0 not in controller.session.sample_analysis_stage
    assert controller.session.sample_analysis_errors[0] == "bad audio"


def test_stem_task_events_update_generation_state(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "stem_generation"},
        {
            "type": "task_progress",
            "id": 0,
            "task": "stem_generation",
            "percent": 0.25,
            "stage": "Generating stems",
        },
        {"type": "task_error", "id": 0, "task": "stem_generation", "msg": "not implemented"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.stem_generating_sample_ids
    assert 0 not in controller.session.stem_generation_progress
    assert 0 not in controller.session.stem_generation_stage
    assert controller.session.stem_generation_errors[0] == "not implemented"


def test_legacy_stem_task_success_rejects_missing_admission_ticket(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_path = samples_dir / "loop.wav"
    write_mono_pcm16_wav(sample_path, 44_100)
    controller.project.sample_paths[0] = "samples/loop.wav"
    source_version = source_version_for_sample_path("samples/loop.wav")
    assert source_version is not None
    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=source_version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=False,
    )
    controller.session.stem_generating_sample_ids.add(0)
    controller.session.stem_generation_source_versions[0] = source_version
    controller.session.stem_generation_progress[0] = 0.5
    controller.session.stem_generation_stage[0] = "Writing stem cache"
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_success", "id": 0, "task": "stem_generation"},
        None,
    ]

    controller.loader.poll_loader_events()

    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False
    audio_engine_mock.publish_prepared_stems.assert_not_called()
    audio_engine_mock.capture_prepared_source.assert_not_called()
    assert "admission ticket is missing" in controller.session.stem_generation_errors[0]
    assert 0 not in controller.session.stem_generating_sample_ids
    assert 0 not in controller.session.stem_generation_source_versions
    assert 0 not in controller.session.stem_generation_progress
    assert 0 not in controller.session.stem_generation_stage


def test_standalone_loader_stem_success_fails_closed_without_ticket(
    audio_engine_mock: Mock,
) -> None:
    project = ProjectState()
    session = SessionState()
    loader = LoaderController(
        project, session, audio_engine_mock, on_pad_bpm_changed=lambda _: None
    )
    session.stem_generating_sample_ids.add(0)
    session.stem_generation_source_versions[0] = "legacy"
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_success", "id": 0, "task": "stem_generation"},
        None,
    ]

    loader.poll_loader_events()

    audio_engine_mock.publish_prepared_stems.assert_not_called()
    audio_engine_mock.capture_prepared_source.assert_not_called()
    assert "admission ticket is missing" in session.stem_generation_errors[0]
    assert session.stem_generating_sample_ids == set()


def test_stale_stem_task_error_is_ignored(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_error", "id": 0, "task": "stem_generation", "msg": "late"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.stem_generation_errors


def test_load_sample_async_already_loading(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test scheduling a load for a sample already loading clears existing state."""
    sample_id = 0
    path1 = "/path/to/first.wav"
    path2 = "/path/to/second.wav"

    controller.session.loading_sample_ids.add(sample_id)
    controller.session.pending_sample_paths[sample_id] = path1
    controller.session.sample_load_progress[sample_id] = 0.5
    controller.session.sample_load_stage[sample_id] = "Loading"

    controller.loader.load_sample_async(sample_id, path2)

    assert controller.session.pending_sample_paths[sample_id] == path2
    assert sample_id in controller.session.loading_sample_ids
    assert controller.session.sample_load_progress.get(sample_id) is None
    assert controller.session.sample_load_stage.get(sample_id) is None


def test_loader_started_event_handling(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test loader started event clears previous state and marks loading."""
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "started", "id": 0},
        None,
    ]

    controller.session.sample_load_errors[0] = "previous error"
    controller.session.sample_load_progress[0] = 0.5
    controller.session.sample_load_stage[0] = "Loading"
    controller.project.sample_paths[0] = None

    controller.loader.poll_loader_events()

    assert 0 in controller.session.loading_sample_ids
    assert controller.session.sample_load_errors.get(0) is None
    assert controller.session.sample_load_progress.get(0) is None
    assert controller.session.sample_load_stage.get(0) is None
    assert controller.project.sample_analysis[0] is None


def test_loader_progress_event_handling(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test loader progress event updates stage and percent."""
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "progress", "id": 0, "stage": "Decoding", "percent": 0.75},
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.session.sample_load_stage[0] == "Decoding"
    assert controller.session.sample_load_progress[0] == 0.75


def test_loader_error_event_preserves_assignment(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """A failed replacement settles loading while retaining the effective assignment."""
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "error",
            "id": 0,
            "msg": "File not found",
        },
        None,
    ]

    controller.session.loading_sample_ids.add(0)
    controller.session.pending_sample_paths[0] = "/path/to/sample.wav"
    controller.session.sample_load_progress[0] = 0.5
    controller.session.sample_load_stage[0] = "Loading"
    controller.project.sample_paths[0] = "/path/to/sample.wav"

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.loading_sample_ids
    assert 0 not in controller.session.pending_sample_paths
    assert controller.session.sample_load_progress.get(0) is None
    assert controller.session.sample_load_stage.get(0) is None
    assert controller.project.sample_paths[0] == "/path/to/sample.wav"
    assert controller.project.sample_durations[0] is None
    assert controller.project.sample_analysis[0] is None
    assert controller.session.sample_load_errors[0] == "File not found"


def test_analyze_sample_async_success(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test successful analysis schedules async analysis."""
    controller.project.sample_paths[0] = "/path/to/sample.wav"

    controller.loader.analyze_sample_async(0)

    audio_engine_mock.analyze_sample_async.assert_called_with(0)
    assert 0 in controller.session.analyzing_sample_ids


def test_analyze_sample_async_already_loading(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test analysis returns early if sample is already loading."""
    controller.session.loading_sample_ids.add(0)

    controller.loader.analyze_sample_async(0)

    audio_engine_mock.analyze_sample_async.assert_not_called()
    assert 0 not in controller.session.analyzing_sample_ids


def test_analyze_sample_async_unloaded_pad_rejected_before_audio(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test analysis on an unloaded pad is rejected without calling Rust."""
    controller.loader.analyze_sample_async(0)

    audio_engine_mock.analyze_sample_async.assert_not_called()
    assert 0 not in controller.session.analyzing_sample_ids
    assert controller.session.sample_analysis_errors[0] == "sample is not loaded"


def test_analyze_sample_async_runtime_error(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test analysis runtime error is handled gracefully."""
    controller.project.sample_paths[0] = "/path/to/sample.wav"
    audio_engine_mock.analyze_sample_async.side_effect = RuntimeError("Audio engine busy")

    controller.loader.analyze_sample_async(0)

    assert 0 not in controller.session.analyzing_sample_ids
    assert controller.session.sample_analysis_errors[0] == "Audio engine busy"


def test_invalid_analysis_data_ignored(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test invalid analysis data (not a dict) is ignored."""
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        {"type": "task_success", "id": 0, "task": "analysis", "analysis": "invalid"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_analysis[0] is None


def test_analysis_validation_error_ignored(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test analysis data failing validation is ignored."""
    invalid_analysis = {"bpm": 120.0, "key": "C#m", "beat_grid": "invalid"}
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "task_started", "id": 0, "task": "analysis"},
        {"type": "task_success", "id": 0, "task": "analysis", "analysis": invalid_analysis},
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_analysis[0] is None


def test_pending_sample_path(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test pending_sample_path returns pending path for loading sample."""
    path = "/path/to/sample.wav"
    controller.session.pending_sample_paths[0] = path

    assert controller.loader.pending_sample_path(0) == path


def test_sample_load_error(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test sample_load_error returns last error message."""
    controller.session.sample_load_errors[0] = "File not found"

    assert controller.loader.sample_load_error(0) == "File not found"


def test_sample_load_progress(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test sample_load_progress returns progress percentage."""
    controller.session.sample_load_progress[0] = 0.75

    assert controller.loader.sample_load_progress(0) == 0.75


def test_sample_load_stage(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test sample_load_stage returns load stage description."""
    controller.session.sample_load_stage[0] = "Decoding"

    assert controller.loader.sample_load_stage(0) == "Decoding"


def test_unload_sample_windows_path(controller: AppController, audio_engine_mock: Mock) -> None:
    """Test unloading a sample with Windows path skips file deletion."""
    sample_id = 0
    path = "C:\\Users\\test\\sample.wav"
    controller.project.sample_paths[sample_id] = path

    controller.loader.unload_sample(sample_id)

    assert controller.project.sample_paths[sample_id] is None


def test_unload_sample_defers_original_deletion_to_native_last_reader(
    tmp_path: Path, controller: AppController, audio_engine_mock: Mock
) -> None:
    """Native cleanup owns deletion after the UI assignment has been revoked."""
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    cached_file = samples_dir / "test.wav"
    cached_file.write_bytes(b"test data")

    controller.project.sample_paths[0] = "samples/test.wav"

    controller.loader.unload_sample(0)

    assert cached_file.exists()
    audio_engine_mock.retire_project_asset.assert_called_once_with(
        str(cached_file), recursive=False
    )


def test_unload_sample_retires_only_declared_legacy_stem_files(
    tmp_path: Path, controller: AppController, audio_engine_mock: Mock
) -> None:
    """Legacy cleanup leaves the pad container and unknown files untouched."""
    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")

    controller.project.sample_paths[0] = "samples/test.wav"
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version="samples/test.wav|10|20",
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )

    controller.loader.unload_sample(0)

    assert stems_dir.exists()
    for kind in STEM_KINDS:
        audio_engine_mock.retire_project_asset.assert_any_call(
            str(stems_dir / f"{kind}.wav"), recursive=False
        )
    assert all(
        not call.kwargs["recursive"]
        for call in audio_engine_mock.retire_project_asset.call_args_list
    )
    assert controller.project.stem_cache[0] is None


def test_unload_sample_clears_restored_stem_runtime_eligibility(
    tmp_path: Path, controller: AppController, audio_engine_mock: Mock
) -> None:
    """Unloading a restored stem pad removes all cache state used for later publication."""
    samples_dir = tmp_path / "samples"
    samples_dir.mkdir()
    sample_path = samples_dir / "test.wav"
    write_mono_pcm16_wav(sample_path, 44_100)

    cache_dir = cache_dir_for_sample_id(0)
    stems_dir = tmp_path / cache_dir
    stems_dir.mkdir(parents=True)
    for kind in STEM_KINDS:
        (stems_dir / f"{kind}.wav").write_bytes(b"stem")

    source_version = source_version_for_sample_path("samples/test.wav")
    assert source_version is not None
    controller.project.sample_paths[0] = "samples/test.wav"
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version=source_version,
        cache_dir=cache_dir,
        stems=expected_stem_files(cache_dir),
        available=True,
    )
    controller.session.pad_stem_enabled_mask[0] = STEM_MASK_VOCALS
    controller.session.pad_stem_last_custom_mask[0] = STEM_MASK_VOCALS
    controller.session.pad_stem_mask_display_mode[0] = "custom"

    controller.loader.unload_sample(0)
    restored_publish_result = controller.stems.publish_restored_stem_cache_if_available(0)

    assert restored_publish_result is True
    assert stems_dir.exists()
    assert sample_path.exists()
    audio_engine_mock.retire_project_asset.assert_any_call(str(sample_path), recursive=False)
    assert controller.project.sample_paths[0] is None
    assert controller.project.stem_cache[0] is None
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert controller.stems.stems_available(0) is False
    assert controller.stems.stem_mask_controls_enabled(0) is False
    assert controller.session.pad_stem_enabled_mask[0] == STEM_COMPONENT_MASK
    assert controller.session.pad_stem_last_custom_mask[0] == STEM_COMPONENT_MASK
    assert controller.session.pad_stem_mask_display_mode[0] == "all"
    audio_engine_mock.publish_prepared_stems.assert_not_called()
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "full_mix")
    audio_engine_mock.unload_sample.assert_called_once_with(0)


def test_unload_sample_outside_samples_dir(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test unloading a sample outside samples dir skips file deletion."""
    sample_id = 0
    path = "/other/path/sample.wav"
    controller.project.sample_paths[sample_id] = path

    controller.loader.unload_sample(sample_id)

    audio_engine_mock.unload_sample.assert_called_with(sample_id)
    assert controller.project.sample_paths[sample_id] is None


def test_poll_loader_events_with_malformed_events(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test poll_loader_events ignores malformed events."""
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "success", "id": 0, "duration_s": 1.0},
        {"type": 123, "id": 0},
        {"type": "success", "id": "not_a_number"},
        {"type": "unknown_type", "id": 0},
        {"type": "started", "id": 1},
        None,
    ]

    controller.loader.poll_loader_events()

    assert 0 not in controller.session.loading_sample_ids
    assert 1 in controller.session.loading_sample_ids
    assert controller.project.sample_durations[0] == 1.0
