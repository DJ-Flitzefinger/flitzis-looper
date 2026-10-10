"""Actual controller integration for neutral key intent and source-bound metadata."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.key_intent import MAX_KEY_EPOCH, MusicalKey, PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import PadContentIdentity

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _seed(controller: AppController, audio: Mock) -> PadKeyIntent:
    controller.project.sample_paths[0] = "samples/shared.wav"
    controller.project.pad_content[0] = PadContentIdentity(
        instance_id="1" * 32, material_id="a" * 32
    )
    intent = PadKeyIntent(
        source=SourceKeyVersion(version=3, raw_key="Em"),
        correction="Am",
        analysis_epoch=4,
        correction_epoch=9,
        base_shift=-4,
        extra_shift=2,
        retrigger=True,
    )
    controller.project.pad_key_intent[0] = intent
    audio.waveform_source_identity.return_value = (7, "b" * 64, 128, 44_100)
    audio.analyze_sample_async.return_value = 11
    return intent


def _success(request_id: object = 11, *, timing_stale: bool = False) -> dict[str, object]:
    return {
        "type": "task_success",
        "task": "analysis",
        "id": 0,
        "request_id": request_id,
        "timing_stale": timing_stale,
        "analysis": {
            "bpm": 120.0,
            "key": "Bm",
            "beat_grid": {"beats": [], "downbeats": [], "bars": []},
        },
    }


def _poll(controller: AppController, audio: Mock, *events: dict[str, object]) -> None:
    audio.poll_loader_events.side_effect = [*events, None]
    controller.loader.poll_loader_events()


def test_metadata_edits_and_three_resets_issue_no_audio_commands(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _seed(controller, audio_engine_mock)
    controller.session.active_sample_ids.add(0)
    controller.session.pad_playhead_s[0] = 12.5
    controller.project.pad_key_lock[0] = True
    audio_engine_mock.reset_mock()
    pad = controller.transport.pad

    pad.clear_manual_key(0)
    current = controller.project.pad_key_intent[0]
    assert (current.correction, current.base_shift, current.extra_shift) == (None, -4, 2)
    pad.set_base_key_intent(0, MusicalKey(root=0, mode="minor"))
    assert controller.project.pad_key_intent[0].base_shift == -4
    pad.reset_base_key_intent(0)
    reset = controller.project.pad_key_intent[0]
    assert (reset.base_shift, reset.extra_shift) == (0, 2)
    pad.set_manual_key(0, "unknown performer label")
    pad.reset_extra_shift_intent(0)
    current = controller.project.pad_key_intent[0]
    assert (current.correction, current.base_shift, current.extra_shift) == (
        "unknown performer label",
        0,
        0,
    )
    pad.set_extra_shift_intent(0, 12)
    pad.set_pitch_retrigger_intent(0, enabled=False)
    assert controller.project.pad_key_intent[0].extra_shift == 12
    assert controller.project.pad_key_intent[0].retrigger is False
    assert controller.session.active_sample_ids == {0}
    assert controller.session.pad_playhead_s[0] == 12.5
    assert controller.project.pad_key_lock[0] is True
    assert controller.persistence._dirty is True
    assert audio_engine_mock.mock_calls == []


def test_deliberate_analysis_removes_old_correction_only_at_real_admission(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    before = _seed(controller, audio_engine_mock)
    audio_engine_mock.analyze_sample_async.side_effect = RuntimeError("busy")
    controller.loader.analyze_sample_async(0)
    assert controller.project.pad_key_intent[0] == before

    audio_engine_mock.analyze_sample_async.side_effect = None
    controller.loader.analyze_sample_async(0)
    admitted = controller.project.pad_key_intent[0]
    assert admitted.correction is None
    assert admitted.analysis_epoch == 5
    assert admitted.correction_epoch == 10
    assert (admitted.base_shift, admitted.extra_shift, admitted.retrigger) == (-4, 2, True)
    _poll(controller, audio_engine_mock, _success())
    result = controller.project.pad_key_intent[0]
    assert result.source == SourceKeyVersion(version=4, raw_key="Bm")
    assert result.correction is None


@pytest.mark.parametrize("timing_stale", [False, True])
def test_newer_correction_wins_even_when_analysis_timing_is_stale(
    controller: AppController, audio_engine_mock: Mock, *, timing_stale: bool
) -> None:
    _seed(controller, audio_engine_mock)
    controller.loader.analyze_sample_async(0)
    controller.transport.pad.set_manual_key(0, "C#m")
    corrected = controller.project.pad_key_intent[0]
    _poll(controller, audio_engine_mock, _success(timing_stale=timing_stale))
    current = controller.project.pad_key_intent[0]
    assert current.correction == "C#m"
    assert current.correction_epoch == corrected.correction_epoch
    assert current.source == SourceKeyVersion(version=4, raw_key="Bm")
    assert (current.base_shift, current.extra_shift, current.retrigger) == (-4, 2, True)


def test_failed_readmission_keeps_current_analysis_and_new_correction(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _seed(controller, audio_engine_mock)
    controller.loader.analyze_sample_async(0)
    controller.transport.pad.set_manual_key(0, "C#m")
    before = controller.project.pad_key_intent[0]
    audio_engine_mock.analyze_sample_async.side_effect = RuntimeError("queue full")
    controller.loader.analyze_sample_async(0)
    assert controller.project.pad_key_intent[0] == before
    assert controller.loader._analysis_request_ids[0] == 11
    assert 0 in controller.session.analyzing_sample_ids
    _poll(controller, audio_engine_mock, _success())
    assert controller.project.pad_key_intent[0].correction == "C#m"
    assert controller.project.pad_key_intent[0].source == SourceKeyVersion(version=4, raw_key="Bm")


@pytest.mark.parametrize("invalid_id", [None, True, False, 11.0, "11"])
def test_unbound_feedback_cannot_settle_current_key_request(
    controller: AppController, audio_engine_mock: Mock, invalid_id: object
) -> None:
    _seed(controller, audio_engine_mock)
    controller.loader.analyze_sample_async(0)
    before = controller.project.pad_key_intent[0]
    _poll(controller, audio_engine_mock, _success(invalid_id))
    assert controller.project.pad_key_intent[0] == before
    assert 0 in controller.session.analyzing_sample_ids
    assert controller.loader._analysis_request_ids[0] == 11
    _poll(controller, audio_engine_mock, _success())
    assert controller.project.pad_key_intent[0].source == SourceKeyVersion(version=4, raw_key="Bm")


def test_legacy_unbound_analysis_retains_display_without_new_key_version(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _seed(controller, audio_engine_mock)
    before = controller.project.pad_key_intent[0]
    _poll(controller, audio_engine_mock, _success(None))
    analysis = controller.project.sample_analysis[0]
    assert analysis is not None
    assert analysis.key == "Bm"
    assert controller.project.pad_key_intent[0] == before


@pytest.mark.parametrize(
    "identity",
    [
        None,
        [7, "b" * 64, 128, 44_100],
        (7, "b" * 64, 128),
        (True, "b" * 64, 128, 44_100),
        (7, None, 128, 44_100),
        (7, "b" * 64, True, 44_100),
        (7, "b" * 64, 0, 44_100),
        (7, "b" * 64, 128, False),
        (7, "b" * 64, 128, 0),
    ],
)
@pytest.mark.parametrize("stage", ["admission", "completion"])
def test_absent_or_malformed_source_identity_grants_no_key_metadata_authority(
    controller: AppController, audio_engine_mock: Mock, identity: object, stage: str
) -> None:
    before = _seed(controller, audio_engine_mock)
    if stage == "admission":
        audio_engine_mock.waveform_source_identity.return_value = identity
    controller.loader.analyze_sample_async(0)
    if stage == "completion":
        controller.transport.pad.set_manual_key(0, "C#m")
        before = controller.project.pad_key_intent[0]
        audio_engine_mock.waveform_source_identity.return_value = identity
    else:
        assert controller.project.pad_key_intent[0] == before
    _poll(controller, audio_engine_mock, _success())
    assert controller.project.pad_key_intent[0] == before
    assert 0 not in controller.session.analyzing_sample_ids
    assert 0 not in controller.loader._analysis_request_ids
    assert controller.loader._key_analysis.has_request(0) is False


@pytest.mark.parametrize("replaced", ["content", "native_source"])
def test_equal_byte_source_aba_cannot_update_captured_key_version(
    controller: AppController, audio_engine_mock: Mock, replaced: str
) -> None:
    _seed(controller, audio_engine_mock)
    controller.loader.analyze_sample_async(0)
    if replaced == "content":
        controller.project.pad_content[0] = PadContentIdentity(
            instance_id="2" * 32, material_id="a" * 32
        )
    else:
        audio_engine_mock.waveform_source_identity.return_value = (8, "b" * 64, 128, 44_100)
    replacement_intent = PadKeyIntent(correction="replacement correction", extra_shift=7)
    controller.project.pad_key_intent[0] = replacement_intent
    _poll(controller, audio_engine_mock, _success())
    assert controller.project.pad_key_intent[0] == replacement_intent


def test_out_of_order_and_duplicate_results_do_not_republish_versions(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _seed(controller, audio_engine_mock)
    controller.loader.analyze_sample_async(0)
    audio_engine_mock.analyze_sample_async.return_value = 12
    controller.loader.analyze_sample_async(0)
    before = controller.project.pad_key_intent[0]
    _poll(controller, audio_engine_mock, _success(11))
    assert controller.project.pad_key_intent[0] == before
    assert controller.loader._analysis_request_ids[0] == 12
    _poll(controller, audio_engine_mock, _success(12))
    current = controller.project.pad_key_intent[0]
    assert current.analysis_epoch == 6
    assert current.source == SourceKeyVersion(version=4, raw_key="Bm")
    _poll(controller, audio_engine_mock, _success(12))
    assert controller.project.pad_key_intent[0] == current


@pytest.mark.parametrize("exhausted", ["analysis_epoch", "correction_epoch", "source"])
def test_epoch_capacity_failure_occurs_before_native_admission(
    controller: AppController, audio_engine_mock: Mock, exhausted: str
) -> None:
    before = _seed(controller, audio_engine_mock)
    if exhausted == "source":
        before = before.changed(source=SourceKeyVersion(version=MAX_KEY_EPOCH, raw_key="Em"))
    else:
        before = before.changed(**{exhausted: MAX_KEY_EPOCH})
    controller.project.pad_key_intent[0] = before
    controller.loader.analyze_sample_async(0)
    assert controller.project.pad_key_intent[0] == before
    audio_engine_mock.analyze_sample_async.assert_not_called()
    assert "capacity exhausted" in controller.session.sample_analysis_errors[0]


@pytest.mark.parametrize("timing_stale", [False, True])
def test_true_equal_path_replacement_resets_only_after_matching_source_success(
    controller: AppController, audio_engine_mock: Mock, *, timing_stale: bool
) -> None:
    before = _seed(controller, audio_engine_mock)
    old_content = controller.project.pad_content[0]
    audio_engine_mock.load_sample_async.return_value = 21
    controller.loader.load_sample_async(0, "samples/shared.wav")
    assert controller.project.pad_key_intent[0] == before
    _poll(
        controller,
        audio_engine_mock,
        {
            "type": "success",
            "id": 0,
            "request_id": 20,
            "cached_path": "samples/shared.wav",
            "timing_stale": timing_stale,
        },
    )
    assert controller.project.pad_key_intent[0] == before
    _poll(
        controller,
        audio_engine_mock,
        {
            "type": "success",
            "id": 0,
            "request_id": 21,
            "cached_path": "samples/shared.wav",
            "timing_stale": timing_stale,
        },
    )
    assert controller.project.pad_key_intent[0] == PadKeyIntent()
    assert controller.project.pad_content[0] != old_content


def test_same_source_restore_preserves_key_intent_and_content(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    before = _seed(controller, audio_engine_mock)
    # Use a legacy original, whose material identity is correctly None.
    controller.project.pad_content[0] = PadContentIdentity(instance_id="1" * 32)
    content = controller.project.pad_content[0]
    controller.loader._load_request_ids[0] = 21
    controller.session.loading_sample_ids.add(0)
    controller.session.pending_sample_paths[0] = "samples/shared.wav"
    _poll(
        controller,
        audio_engine_mock,
        {
            "type": "success",
            "id": 0,
            "request_id": 21,
            "cached_path": "samples/shared.wav",
            "timing_stale": True,
        },
    )
    assert controller.project.pad_key_intent[0] == before
    assert controller.project.pad_content[0] == content


def test_failed_load_keeps_all_source_bound_key_intent(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    before = _seed(controller, audio_engine_mock)
    audio_engine_mock.load_sample_async.side_effect = RuntimeError("queue full")
    controller.loader.load_sample_async(0, "same byte replacement.wav")
    assert controller.project.pad_key_intent[0] == before


def test_prepare_getter_failure_preserves_correction_before_native_admission(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    before = _seed(controller, audio_engine_mock)
    audio_engine_mock.waveform_source_identity.side_effect = RuntimeError("source identity lock")
    controller.loader.analyze_sample_async(0)
    assert controller.project.pad_key_intent[0] == before
    audio_engine_mock.analyze_sample_async.assert_not_called()
    assert controller.session.sample_analysis_errors[0] == "source identity lock"


def test_post_admission_getter_failure_preserves_intent_and_settles_real_request(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    before = _seed(controller, audio_engine_mock)
    identity = (7, "b" * 64, 128, 44_100)
    audio_engine_mock.waveform_source_identity.side_effect = [
        identity,
        RuntimeError("post-admission identity lock"),
    ]
    controller.loader.analyze_sample_async(0)
    assert controller.project.pad_key_intent[0] == before
    assert controller.loader._analysis_request_ids[0] == 11
    assert 0 in controller.session.analyzing_sample_ids
    assert "post-admission identity lock" in controller.session.sample_analysis_errors[0]
    _poll(
        controller,
        audio_engine_mock,
        {"type": "task_started", "task": "analysis", "id": 0, "request_id": 11},
    )
    assert "post-admission identity lock" in controller.session.sample_analysis_errors[0]
    _poll(controller, audio_engine_mock, _success())
    assert controller.project.pad_key_intent[0] == before
    assert 0 not in controller.session.analyzing_sample_ids
    assert 0 not in controller.loader._analysis_request_ids
    assert "post-admission identity lock" in controller.session.sample_analysis_errors[0]


def test_completion_getter_failure_preserves_later_correction_and_finishes_poll(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _seed(controller, audio_engine_mock)
    controller.loader.analyze_sample_async(0)
    controller.transport.pad.set_manual_key(0, "later C2")
    before = controller.project.pad_key_intent[0]
    audio_engine_mock.waveform_source_identity.side_effect = RuntimeError(
        "completion identity lock"
    )
    _poll(controller, audio_engine_mock, _success())
    assert controller.project.pad_key_intent[0] == before
    assert 0 not in controller.session.analyzing_sample_ids
    assert 0 not in controller.loader._analysis_request_ids
    assert controller.loader._key_analysis.has_request(0) is False
    assert "completion identity lock" in controller.session.sample_analysis_errors[0]
