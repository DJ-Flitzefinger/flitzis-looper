"""Approximate scalars preserve refusal, source, performer and phase authority."""

import json
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis.app_tempo_approximate import from_saved
from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.controller import loader as loader_module
from flitzis_looper.controller.current_timing import CurrentPadTiming
from flitzis_looper.controller.timing_metadata import timing_anchor_sec_from_analysis
from flitzis_looper.copy_snapshot import CopySnapshotData, capture_copy_snapshot
from flitzis_looper.models import BeatGrid, PadContentIdentity, SampleAnalysis
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import sidebar_left
from tests.flitzis_looper.controller.test_adaptive_tempo_probe import _v4_method
from tests.flitzis_looper.controller.test_incoming_load_intent import _admit, _success
from tests.flitzis_looper.controller.test_key_metadata import _poll, _seed
from tests.flitzis_looper.controller.test_mean_tempo_fallback import _conflict_data

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def approximate() -> AppTempoMethod:
    method = from_saved(AppTempoMethod.model_validate(_conflict_data(), strict=True))
    assert method is not None
    return method


def analysis(method: AppTempoMethod) -> SampleAnalysis:
    return SampleAnalysis(
        bpm=method.bpm or 0.0,
        key="Em",
        bpm_method=method,
        beat_grid=BeatGrid(beats=method.raw_beats, downbeats=method.raw_downbeats, bars=[]),
    )


def test_scalar_refusal_round_trip_copy_warning_and_no_phase(controller: AppController) -> None:
    method = approximate()
    assert method.bpm == 120.0
    assert method.status == "approximate_estimate"
    assert method.ordinal_fit.outcome == "conflicting_sections"
    restored = SampleAnalysis.model_validate_json(analysis(method).model_dump_json())
    assert restored.beat_grid == BeatGrid(beats=[], downbeats=[], bars=[])
    assert restored.accepted_timing is None
    assert timing_anchor_sec_from_analysis(restored) == 0.0
    controller.project.sample_analysis[0] = restored
    controller.project.sample_paths[0] = "samples/fixture.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    snapshot = capture_copy_snapshot(controller.project, 0)
    assert CopySnapshotData.model_validate_json(snapshot.payload_json).analysis == restored
    ctx = UiContext(controller)
    assert ctx.state.pads.bpm_drift_warning(0) == "Approximate BPM"
    assert "may be inaccurate" in sidebar_left.bpm_estimate_guidance(method)
    controller.project.manual_bpm[0] = 60.0
    assert ctx.state.pads.bpm_drift_warning(0) is None


@pytest.mark.parametrize(
    "damage", ["scalar", "period", "original", "range", "source_kind", "selection", "extra"]
)
def test_strict_approximate_replay_rejects_tampering(damage: str) -> None:
    data = approximate().model_dump(mode="python")
    evidence = data["approximate_tempo"]
    if damage == "scalar":
        data["bpm"] = 119.0
    elif damage == "period":
        evidence["period_seconds"] = 0.6
    elif damage == "original":
        evidence["original"]["status"] = "estimated"
    elif damage == "range":
        evidence["range_constraint"] = {"minimum_bpm": 130.0, "maximum_bpm": 150.0}
    elif damage == "source_kind":
        evidence["kind"] = "source_energy"
    elif damage == "selection":
        evidence["kind"] = "neural_intervals"
    else:
        evidence["precise_pass"] = True
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("tempo_only", [False, True])
def test_same_source_precise_precedes_approximate(
    controller: AppController, audio_engine_mock: Mock, *, tempo_only: bool
) -> None:
    precise = analysis(_v4_method())
    controller.project.sample_analysis[0] = precise
    audio_engine_mock.pad_timing_intent.return_value = "legacy"
    audio_engine_mock.set_pad_bpm.reset_mock()
    candidate = approximate()
    if tempo_only:
        controller.loader._apply_tempo_only_analysis(0, {"bpm_method": candidate.model_dump_json()})
    else:
        controller.loader._store_analysis_completion(
            0, analysis(candidate).model_dump(mode="python"), key_completed=False, preserve_key=True
        )
    assert controller.project.sample_analysis[0] is precise
    audio_engine_mock.set_pad_bpm.assert_not_called()


@pytest.mark.parametrize("state", ["current", "stale", "manual", "tap"])
def test_saved_failure_reuses_only_verified_current_source_without_inference(
    controller: AppController, audio_engine_mock: Mock, state: str
) -> None:
    failed = AppTempoMethod.model_validate(_conflict_data(), strict=True)
    previous = analysis(failed)
    controller.project.sample_analysis[0] = previous
    controller.project.pad_timing_intent[0] = "legacy"
    audio_engine_mock.loaded_source_digest.side_effect = lambda _: (
        failed.source_sha256 if state != "stale" else "f" * 64
    )
    if state in {"manual", "tap"}:
        controller.project.manual_bpm[0] = 94.0
        controller.project.pad_timing_intent[0] = state
    audio_engine_mock.analyze_sample_async.reset_mock()
    controller.loader._apply_loaded_analysis(0, None)
    selected = controller.project.sample_analysis[0]
    if state == "current":
        assert selected.bpm == 120.0
        assert selected.bpm_method.status == "approximate_estimate"
        assert selected.key == "Em"
    else:
        assert selected is previous
    audio_engine_mock.analyze_sample_async.assert_not_called()


def test_fresh_loaded_invalid_approximate_cannot_publish_unproven_scalar(
    controller: AppController,
) -> None:
    data = analysis(approximate()).model_dump(mode="python")
    data["bpm_method"]["approximate_tempo"]["period_seconds"] = 0.6
    with pytest.raises(ValueError, match="Fresh BPM provenance"):
        controller.loader._apply_loaded_analysis(0, data)
    assert controller.project.sample_analysis[0] is None


def test_saved_packet_does_not_mutate_original() -> None:
    original = AppTempoMethod.model_validate(_conflict_data(), strict=True)
    before = json.loads(original.model_dump_json())
    assert from_saved(original) is not None
    assert json.loads(original.model_dump_json()) == before


def test_current_accepted_native_timing_precedes_unavailable_cached_approximate(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.project.sample_analysis[0] = analysis(approximate())
    audio_engine_mock.loaded_source_digest.side_effect = lambda _: None
    current = CurrentPadTiming(0.5, 0.0, 44100, accepted_revision="fixture-current-native")
    monkeypatch.setattr(controller.transport.bpm, "current_timing", lambda _: current)
    refresh = Mock()
    monkeypatch.setattr(controller.transport.bpm, "_on_accepted_refresh", refresh)
    audio_engine_mock.set_pad_bpm.reset_mock()
    controller.transport.bpm.on_pad_bpm_changed(0)
    refresh.assert_called_once_with(0)
    audio_engine_mock.set_pad_bpm.assert_not_called()


@pytest.mark.parametrize("state", ["current", "stale", "native", "unavailable"])
def test_persisted_positive_approximate_restore_requires_current_source_and_authority(
    controller: AppController, audio_engine_mock: Mock, state: str
) -> None:
    previous = analysis(approximate())
    controller.project.sample_analysis[0] = previous
    digest = (
        None
        if state == "unavailable"
        else "f" * 64
        if state == "stale"
        else previous.bpm_method.source_sha256
    )
    audio_engine_mock.loaded_source_digest.side_effect = lambda _: digest
    if state == "native":
        audio_engine_mock.pad_timing_intent.return_value = "automatic"
    audio_engine_mock.set_pad_bpm.reset_mock()
    controller.loader._apply_loaded_analysis(0, None)
    if state == "current":
        assert controller.project.sample_analysis[0].bpm == 120.0
        audio_engine_mock.set_pad_bpm.assert_called_once_with(
            0, 120.0, expected_source_generation=1
        )
    else:
        audio_engine_mock.set_pad_bpm.assert_not_called()
    if state == "stale":
        assert controller.project.sample_analysis[0].bpm == 0.0
        assert controller.project.sample_analysis[0].bpm_method is None
    elif state == "unavailable":
        assert controller.project.sample_analysis[0] is previous


@pytest.mark.parametrize("race", ["source", "manual", "native", "publication"])
def test_saved_replay_losing_current_source_or_timing_never_publishes_stale_bpm(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch, race: str
) -> None:
    failed = AppTempoMethod.model_validate(_conflict_data(), strict=True)
    previous = analysis(failed)
    controller.project.sample_analysis[0] = previous
    audio_engine_mock.loaded_source_digest.side_effect = lambda _: failed.source_sha256
    binding = Mock(
        is_current=Mock(return_value=True), metadata=Mock(return_value={"source_generation": 17})
    )
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = lambda _: binding
    audio_engine_mock.set_pad_bpm.reset_mock()

    def replay(method: AppTempoMethod, bounds: object) -> AppTempoMethod | None:
        result = from_saved(method)
        if race == "source":
            binding.is_current.return_value = False
        elif race == "manual":
            controller.project.manual_bpm[0] = 94.0
        elif race == "native":
            audio_engine_mock.pad_timing_intent.return_value = "automatic"
        return result

    monkeypatch.setattr(loader_module, "from_saved", replay)
    if race == "publication":
        audio_engine_mock.set_pad_bpm.side_effect = RuntimeError(
            "source generation changed before admission"
        )
    controller.loader._apply_loaded_analysis(0, None)
    assert controller.project.sample_analysis[0] is previous
    if race == "publication":
        audio_engine_mock.set_pad_bpm.assert_called_once_with(
            0, 120.0, expected_source_generation=17
        )
    else:
        audio_engine_mock.set_pad_bpm.assert_not_called()


@pytest.mark.parametrize("persisted", [False, True])
def test_derived_refresh_failure_retains_admitted_scalar_and_visible_error(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    persisted: bool,
) -> None:
    previous = analysis(
        approximate() if persisted else AppTempoMethod.model_validate(_conflict_data(), strict=True)
    )
    controller.project.sample_analysis[0] = previous
    audio_engine_mock.loaded_source_digest.side_effect = lambda _: previous.bpm_method.source_sha256
    callback = Mock(side_effect=RuntimeError("derived loop publication failed"))
    monkeypatch.setattr(controller.loader, "_on_pad_bpm_changed", callback)
    audio_engine_mock.set_pad_bpm.reset_mock()
    controller.loader._apply_loaded_analysis(0, None)
    assert controller.project.sample_analysis[0].bpm == 120.0
    assert controller.project.sample_analysis[0].bpm_method.status == "approximate_estimate"
    audio_engine_mock.set_pad_bpm.assert_called_once_with(0, 120.0, expected_source_generation=1)
    callback.assert_called_once_with(0, publish_bpm=False, expected_source_generation=1)
    assert "derived control refresh failed" in controller.session.sample_analysis_errors[0]


def test_startup_and_changed_source_restore_never_publish_cached_approximate(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    previous = analysis(approximate())
    controller.project.sample_paths[0] = "samples/fixture.wav"
    controller.project.sample_analysis[0] = previous
    audio_engine_mock.loaded_source_digest.side_effect = lambda _: None
    audio_engine_mock.set_pad_bpm.reset_mock()
    controller.transport.apply_project_state_to_audio()
    assert controller.transport.bpm.effective_bpm(0) is None
    assert all(call.args[1] is None for call in audio_engine_mock.set_pad_bpm.call_args_list)
    audio_engine_mock.loaded_source_digest.side_effect = lambda _: "f" * 64
    controller.loader._apply_loaded_analysis(0, None)
    assert controller.project.sample_analysis[0].bpm == 0.0
    assert controller.transport.bpm.effective_bpm(0) is None
    assert all(call.args[1] is None for call in audio_engine_mock.set_pad_bpm.call_args_list)


@pytest.mark.parametrize("manual", [False, True])
def test_invalid_incoming_tempo_completes_source_and_keeps_performer_override(
    controller: AppController, audio_engine_mock: Mock, *, manual: bool
) -> None:
    _admit(controller, audio_engine_mock)
    if manual:
        controller.loader.set_incoming_timing_intent(0, 7, period_seconds=0.625, grid_origin_s=0.0)
    event = _success()
    event["analysis"] = analysis(approximate()).model_dump(mode="python")
    event["analysis"]["bpm_method"]["approximate_tempo"]["period_seconds"] = 0.6
    audio_engine_mock.poll_loader_events.side_effect = [event, None]
    audio_engine_mock.set_pad_bpm.reset_mock()
    controller.loader.poll_loader_events()
    assert controller.project.sample_paths[0] == "samples/incoming.wav"
    assert controller.project.pad_content[0] is not None
    assert controller.project.sample_durations[0] == 600.0
    assert controller.loader.incoming_load_request_id(0) is None
    assert controller.project.sample_analysis[0].bpm == 0.0
    assert "Fresh BPM provenance" in controller.session.sample_analysis_errors[0]
    assert controller.project.manual_bpm[0] == (96.0 if manual else None)
    if not manual:
        audio_engine_mock.set_pad_bpm.assert_called_with(0, None)


def test_loaded_approximate_retains_precise_and_merges_independent_key(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    precise = analysis(_v4_method())
    controller.project.sample_analysis[0] = precise
    monkeypatch.setattr(controller.loader._key_analysis, "loaded", lambda *_: True)
    candidate = analysis(approximate()).model_dump(mode="python")
    candidate.update(key="F#m", key_status="ready")
    controller.loader._apply_loaded_analysis(0, candidate, key_request_id=17)
    retained = controller.project.sample_analysis[0]
    assert retained.bpm_method is precise.bpm_method
    assert retained.beat_grid == precise.beat_grid
    assert retained.key == "F#m"


@pytest.mark.parametrize("current_request", [None, 12])
def test_superseded_or_cancelled_approximate_completion_keeps_previous_tuple(
    controller: AppController, audio_engine_mock: Mock, current_request: int | None
) -> None:
    _seed(controller, audio_engine_mock)
    previous = analysis(_v4_method())
    controller.project.sample_analysis[0] = previous
    if current_request is not None:
        controller.loader._analysis_request_ids[0] = current_request
    audio_engine_mock.set_pad_bpm.reset_mock()
    _poll(
        controller,
        audio_engine_mock,
        {
            "type": "task_success",
            "task": "analysis",
            "id": 0,
            "request_id": 11,
            "analysis": analysis(approximate()).model_dump(mode="python"),
        },
    )
    assert controller.project.sample_analysis[0] is previous
    audio_engine_mock.set_pad_bpm.assert_not_called()


def test_valid_incoming_approximate_is_usable_and_warns(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _admit(controller, audio_engine_mock)
    event = _success()
    event["analysis"] = analysis(approximate()).model_dump(mode="python")
    _poll(controller, audio_engine_mock, event)
    selected = controller.project.sample_analysis[0]
    assert selected.bpm == 120.0
    assert selected.beat_grid == BeatGrid(beats=[], downbeats=[], bars=[])
    assert UiContext(controller).state.pads.bpm_drift_warning(0) == "Approximate BPM"
