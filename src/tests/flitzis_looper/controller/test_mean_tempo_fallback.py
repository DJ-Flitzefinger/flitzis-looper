"""An interior mean is approximate metadata with ordinary scalar publication owners."""

import hashlib
import json
from copy import deepcopy
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis.app_tempo_fallback import mean_period_ratio
from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.copy_snapshot import CopySnapshotData, capture_copy_snapshot
from flitzis_looper.models import BeatGrid, PadContentIdentity, SampleAnalysis
from tests.flitzis_looper.controller.test_adaptive_tempo_probe import _v4_method
from tests.flitzis_looper.controller.test_corroborated_tempo import _v6_data
from tests.flitzis_looper.controller.test_inferred_tempo_probe import _candidate_data, _v5_data
from tests.flitzis_looper.controller.test_key_metadata import _poll, _seed
from tests.flitzis_looper.controller.test_period_first_tempo import _perfect_checks, _upgrade

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


def _known_probe_windows(
    points: list[tuple[int, float]], period: float, anchor: float
) -> list[list[float]]:
    """Count independently specified grid members in this fixture's five windows."""
    supported = []
    for bounds in ([16.0, 48.0], [80.0, 112.0], [144.0, 176.0], [208.0, 240.0], [272.0, 304.0]):
        start, end = bounds
        observed = [t for k, t in points if start <= t < end and start <= anchor + period * k < end]
        if len(observed) >= 4 and observed[-1] - observed[0] >= 10.0:
            supported.append(bounds)
    pairs = [
        [left, right]
        for i, left in enumerate(supported)
        for right in supported[i + 1 :]
        if left[1] <= right[0]
    ]
    assert pairs
    return max(pairs, key=lambda pair: pair[1][0] - pair[0][1])


def _conflict_data(scale: float = 1.0, *, raw_seconds: list[float] | None = None) -> dict:
    """Known affine grids compete at the ends; the entire interior is 120 BPM."""
    anchor = 0.0625
    primary = [
        anchor + 0.5 * scale * k
        for k in range(int(640 / scale))
        if 64.0 <= anchor + 0.5 * scale * k < 256.0 or k % 5 == 0
    ]
    ends = [
        anchor + 0.625 * scale * k
        for k in range(int(512 / scale))
        if not 64.0 <= anchor + 0.625 * scale * k < 256.0
    ]
    known_raw = sorted(set(primary) | set(ends))
    raw = known_raw if raw_seconds is None else raw_seconds
    assert len(raw) == len(known_raw)
    periods = (0.5 * scale, 0.25 * scale, 1.0 * scale, 0.625 * scale)
    # Membership follows these explicitly constructed exact binary grids.
    members = [
        [
            (int((t - anchor) / period), raw[index])
            for index, t in enumerate(known_raw)
            if ((t - anchor) / period).is_integer()
        ]
        for period in periods
    ]
    summaries = [
        _candidate_data(raw, points, period, anchor, 320.0)
        for points, period in zip(members, periods, strict=True)
    ]
    windows = _known_probe_windows(members[0], periods[0], anchor)
    trials = [
        {
            "candidate_index": index,
            "outcome": "conflicting_sections",
            "evidence_tier": "two_ten_second",
            "candidate_limit_reached": False,
            "conflicting_sections": [0, 4],
            "critical_indices": [i for i in range(4) if i != index],
            **_perfect_checks(points, period, _known_probe_windows(points, period, anchor)),
        }
        for index, (points, period) in enumerate(zip(members[:3], periods[:3], strict=True))
    ]
    data = _v5_data(extra_peaks=())
    data.update(raw_beats=raw, raw_downbeats=[], bpm=None, status="uncorroborated")
    data["ordinal_fit"].update(
        outcome="conflicting_sections",
        winner=summaries[0],
        alternatives=summaries[1:],
        assignments=[[raw.index(t), k] for k, t in members[0]],
        evaluated_candidate_count=4,
        conflicting_sections=[0, 4],
        **_perfect_checks(members[0], periods[0], windows),
    )
    for index, harmonic in enumerate(data["ordinal_fit"]["harmonics"]):
        harmonic.update(
            seed_anchor_seconds=anchor,
            seed_period_seconds=periods[0] * harmonic["factor"],
            alternative_index=index,
        )
    return _upgrade(data, trials)


def _mean_data(*, precise: bool = False, qm_factor: float = 1.0) -> dict:
    data = _conflict_data()
    old = _v6_data()["qm_corroboration"]
    source, qm = old["source"], old["qm"]
    source["loaded_frame_count"] = data["frame_count"]
    descriptor = json.loads(source["decoder_descriptor_json"])
    descriptor["pcm"].update(full_frames=320 * 44100, full_bytes=320 * 44100 * 8)
    source["decoder_descriptor_json"] = json.dumps(
        descriptor, sort_keys=True, separators=(",", ":")
    )
    source["decoder_identity"] = hashlib.sha256(
        source["decoder_descriptor_json"].encode()
    ).hexdigest()
    qm["analyzer"]["frame_count"] = 320 * 44100
    if precise:
        qm["raw_beat_frames"] = [(0.0625 + 0.5 * k) / (512 / 44100) for k in range(640)]
        core = _v6_data()["qm_corroboration"]["qm"]["estimate"]
        # A genuine whole-source scalar uses the same original-index v4 evidence.
        region = {
            "window_seconds": [64.0, 256.0],
            "bpm": 120.0,
            "retained": 384,
            "excluded_raw_indices": [],
            "max_residual_seconds": 0.0,
            "rms_residual_seconds": 0.0,
        }
        core.update(selected=region, regions=[region])
        probe = core["integer_probe"]
        probe.update(
            common_region=region,
            pivot_raw_index=319.5,
            pivot_seconds=159.8125,
            windows=[
                {**region, "window_seconds": bounds, "retained": 32}
                for bounds in ([64.0, 80.0], [240.0, 256.0])
            ],
            window_gap_seconds=160.5,
        )
    else:
        own = _conflict_data(qm_factor)
        qm["raw_beat_frames"] = [t / (512 / 44100) for t in own["raw_beats"]]
        # QM evidence owns ODF frames. Build its strict summaries from the exact
        # frame * (hop / rate) seconds that the public DTO will replay.
        own = _conflict_data(
            qm_factor,
            raw_seconds=[frame * (512 / 44100) for frame in qm["raw_beat_frames"]],
        )
        core = {
            key: deepcopy(own[key])
            for key in ("status", "bpm", "selected", "regions", "integer_probe", "ordinal_fit")
        }
        core["kernel_version"] = 9
    qm["raw_downbeat_indices"] = []
    qm["estimate"] = core
    evidence = {
        "kind": "precise_qm" if precise else "mean_estimate",
        "neural_result": {
            key: data[key] for key in ("schema_version", "backend", "policy", "status", "bpm")
        },
        "source": source,
        "qm": qm,
    }
    if not precise:
        regions = [
            {
                "window_seconds": [i * 64.0, (i + 1) * 64.0],
                "bpm": 120.0,
                "retained": 128,
                "excluded_raw_indices": [],
                "max_residual_seconds": 0.0,
                "rms_residual_seconds": 0.0,
            }
            for i in (1, 2, 3)
        ]
        qm_regions = deepcopy(regions)
        for region in qm_regions:
            region.update(bpm=120.0 / qm_factor, retained=int(128 / qm_factor))
        evidence["mean"] = {
            "window_seconds": [64.0, 256.0],
            "neural_regions": regions,
            "qm_regions": qm_regions,
            "neural_mean_bpm": 120.0,
            "qm_mean_bpm": 120.0 / qm_factor,
            "qm_bpm_factor": qm_factor,
            "adjusted_qm_mean_bpm": 120.0,
            "maximum_period_ratio": 1.0,
        }
    data.update(
        schema_version=10,
        backend="beat_this_qm_dsp",
        policy="neural-qm-tempo-fallback-v10",
        status="estimated" if precise else "mean_estimate",
        bpm=120.0,
        tempo_fallback=evidence,
    )
    return data


def _mean_method() -> AppTempoMethod:
    return AppTempoMethod.model_validate(_mean_data(), strict=True)


def _different_source_precise() -> AppTempoMethod:
    data = _mean_data(precise=True)
    source = data["tempo_fallback"]["source"]
    source["source_sha256"] = data["source_sha256"] = "f" * 64
    descriptor = json.loads(source["decoder_descriptor_json"])
    descriptor["original"]["sha256"] = "f" * 64
    source["decoder_descriptor_json"] = json.dumps(
        descriptor, sort_keys=True, separators=(",", ":")
    )
    source["decoder_identity"] = hashlib.sha256(
        source["decoder_descriptor_json"].encode()
    ).hexdigest()
    return AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize(("precise", "factor"), [(False, 1.0), (False, 2.0), (True, 1.0)])
def test_last_fallback_replays_original_conflict_and_independent_qm(
    factor: float, *, precise: bool
) -> None:
    data = _mean_data(precise=precise, qm_factor=factor)
    method = AppTempoMethod.model_validate(data, strict=True)
    assert method.bpm == 120.0
    assert method.status == ("estimated" if precise else "mean_estimate")
    assert method.ordinal_fit.outcome == "conflicting_sections"
    assert method.tempo_fallback.qm.estimate.bpm == (120.0 if precise else None)
    if factor == 2.0:
        coarse_trial = method.tempo_fallback.qm.estimate.ordinal_fit.period_selection.trials[2]
        assert coarse_trial.probe_windows == [[80.0, 112.0], [208.0, 240.0]]
    encoded = method.model_dump_json()
    assert encoded.count('"raw_beats":') == 1
    assert encoded.count('"raw_beat_frames":') == 1
    assert AppTempoMethod.model_validate_json(encoded, strict=True) == method
    assert AppTempoMethod.model_validate(method.model_dump(mode="python"), strict=True) == method


@pytest.mark.parametrize("factor", [0.5, 1.0, 2.0])
def test_one_global_factor_preserves_the_direct_inclusive_ratio_boundary(factor: float) -> None:
    assert mean_period_ratio([120.0] * 3, [120.0 / factor] * 3, factor) == 1.0
    assert mean_period_ratio([120.0] * 3, [121.2 / factor] * 3, factor) == 1.01
    assert mean_period_ratio([120.0] * 3, [121.200000001 / factor] * 3, factor) is None
    assert (
        mean_period_ratio([120.0] * 3, [120.0 / factor, 240.0 / factor, 120.0 / factor], factor)
        is None
    )


def _damage_local_mean(data: dict, damage: str) -> None:
    mean = data["tempo_fallback"]["mean"]
    region = mean["neural_regions"][0]
    if damage == "window":
        region["window_seconds"][0] += 0.1
    elif damage == "mask":
        index = next(i for i, t in enumerate(data["raw_beats"]) if 64.0 <= t < 128.0)
        region["excluded_raw_indices"] = [index]
        region["retained"] -= 1
    elif damage == "retained":
        region["retained"] = True
    elif damage == "residual":
        region["max_residual_seconds"] = 0.001
    elif damage == "mean":
        mean["neural_mean_bpm"] = data["bpm"] = 119.0
    elif damage == "factor":
        mean["qm_bpm_factor"] = 2.0
    elif damage == "ratio":
        mean["maximum_period_ratio"] = 1.005
    elif damage == "old_metric":
        mean["maximum_relative_period_difference"] = 0.0


def _damage_source_selection(data: dict, damage: str) -> None:
    evidence = data["tempo_fallback"]
    if damage == "qm_success":
        evidence["qm"]["estimate"]["bpm"] = 120.0
    elif damage == "hop":
        evidence["qm"]["odf_hop_samples"] = 511
    elif damage == "ceil":
        evidence["qm"]["analyzer"]["frame_count"] -= 1
    elif damage == "mono":
        evidence["source"]["verified_nn_mono_sha256"] = "0" * 64
    elif damage == "range":
        evidence["range_constraint"] = {"minimum_bpm": 130.0, "maximum_bpm": 150.0}
    elif damage == "mean_null":
        evidence["mean"] = None
    elif damage == "range_null":
        evidence["range_constraint"] = None
    else:
        data["bpm"] = 121.0


@pytest.mark.parametrize(
    "damage",
    [
        "window",
        "mask",
        "retained",
        "residual",
        "mean",
        "factor",
        "ratio",
        "old_metric",
        "qm_success",
        "hop",
        "ceil",
        "mono",
        "range",
        "mean_null",
        "range_null",
        "selected",
    ],
)
def test_mean_rejects_forged_local_math_source_and_selection(damage: str) -> None:
    data = _mean_data()
    if damage in {
        "window",
        "mask",
        "retained",
        "residual",
        "mean",
        "factor",
        "ratio",
        "old_metric",
    }:
        _damage_local_mean(data, damage)
    else:
        _damage_source_selection(data, damage)
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("version", range(1, 10))
def test_old_schemas_reject_fallback_field_even_when_null(version: int) -> None:
    data = _mean_data()
    data.update(schema_version=version, tempo_fallback=None)
    with pytest.raises(ValidationError, match="fallback fields"):
        AppTempoMethod.model_validate(data, strict=True)


def test_mean_copy_and_restore_preserve_selected_scalar_key_and_full_evidence(
    controller: AppController,
) -> None:
    method = _mean_method()
    analysis = SampleAnalysis(
        bpm=120.0,
        key="D#m",
        beat_grid=BeatGrid(beats=method.raw_beats, downbeats=[], bars=[]),
        bpm_method=method,
    )
    restored = SampleAnalysis.model_validate_json(analysis.model_dump_json())
    assert restored == analysis
    controller.project.sample_paths[0] = "samples/fixture.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    controller.project.sample_analysis[0] = restored
    snapshot = capture_copy_snapshot(controller.project, 0)
    assert CopySnapshotData.model_validate_json(snapshot.payload_json).analysis == restored
    damaged = json.loads(snapshot.payload_json)
    damaged["analysis"]["bpm"] = 119.0
    with pytest.raises(ValidationError):
        CopySnapshotData.model_validate(damaged, strict=True)


def _previous(method: AppTempoMethod, *, bpm: float | None = None) -> SampleAnalysis:
    return SampleAnalysis(
        bpm=method.bpm if bpm is None else bpm,
        key="Am",
        beat_grid=BeatGrid(beats=[0.25], downbeats=[], bars=[]),
        bpm_method=method,
    )


@pytest.mark.parametrize("tempo_only", [False, True])
@pytest.mark.parametrize("manual_intent", [None, "manual", "tap"])
@pytest.mark.parametrize("prior_version", [4, 10])
def test_same_source_precise_scalar_grid_and_method_outrank_a_new_mean(
    controller: AppController,
    audio_engine_mock: Mock,
    manual_intent: str | None,
    prior_version: int,
    *,
    tempo_only: bool,
) -> None:
    precise = (
        _v4_method()
        if prior_version == 4
        else AppTempoMethod.model_validate(_mean_data(precise=True), strict=True)
    )
    previous = _previous(precise)
    controller.project.sample_analysis[0] = previous
    controller.project.pad_timing_intent[0] = "legacy"
    if manual_intent is not None:
        controller.project.manual_bpm[0] = 94.0
        controller.project.pad_timing_intent[0] = manual_intent
    audio_engine_mock.pad_timing_intent.return_value = "legacy"
    audio_engine_mock.set_pad_bpm.reset_mock()
    candidate = _mean_method()
    if tempo_only:
        controller.loader._apply_tempo_only_analysis(0, {"bpm_method": candidate.model_dump_json()})
    else:
        controller.loader._store_analysis_completion(
            0,
            _previous(candidate).model_dump(mode="python"),
            key_completed=False,
            preserve_key=True,
        )
    retained = controller.project.sample_analysis[0]
    assert retained is previous
    assert retained.bpm_method is precise
    assert retained.beat_grid == previous.beat_grid
    audio_engine_mock.set_pad_bpm.assert_not_called()
    assert controller.project.manual_bpm[0] == (94.0 if manual_intent is not None else None)
    if manual_intent is not None:
        assert controller.project.pad_timing_intent[0] == manual_intent
        # Clearing a later performer override still reveals the retained precise tuple.
        controller.project.manual_bpm[0] = None
        controller.project.pad_timing_intent[0] = "legacy"
        assert controller.project.sample_analysis[0].bpm_method is precise


def test_retaining_precise_tempo_still_merges_independently_completed_key(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    precise = AppTempoMethod.model_validate(_mean_data(precise=True), strict=True)
    previous = _previous(precise)
    controller.project.sample_analysis[0] = previous
    audio_engine_mock.set_pad_bpm.reset_mock()
    candidate = _previous(_mean_method()).model_copy(update={"key": "F#m"})
    controller.loader._store_analysis_completion(
        0, candidate.model_dump(mode="python"), key_completed=True, preserve_key=False
    )
    retained = controller.project.sample_analysis[0]
    assert retained.key == "F#m"
    assert retained.bpm_method is precise
    assert retained.bpm == previous.bpm
    assert retained.beat_grid == previous.beat_grid
    audio_engine_mock.set_pad_bpm.assert_not_called()


def test_optional_mean_can_upgrade_an_unproven_scalar_without_changing_key(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    previous = _previous(_mean_method()).model_copy(update={"bpm": 117.0, "bpm_method": None})
    controller.project.sample_analysis[0] = previous
    audio_engine_mock.pad_timing_intent.return_value = "legacy"
    controller.loader._apply_tempo_only_analysis(
        0, {"bpm_method": _mean_method().model_dump_json()}
    )
    selected = controller.project.sample_analysis[0]
    assert selected.bpm == 120.0
    assert selected.bpm_method.status == "mean_estimate"
    assert selected.key == "Am"
    assert selected.beat_grid.beats == selected.bpm_method.raw_beats


def test_failed_new_analysis_keeps_a_previous_mean_and_its_key(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    previous = _previous(_mean_method())
    controller.project.sample_analysis[0] = previous
    audio_engine_mock.set_pad_bpm.reset_mock()
    failed = previous.model_dump(mode="python")
    failed.update(bpm=0.0, key="unknown", bpm_method=_conflict_data())
    controller.loader._store_analysis_completion(0, failed, key_completed=False, preserve_key=True)
    assert controller.project.sample_analysis[0] is previous
    audio_engine_mock.set_pad_bpm.assert_not_called()


@pytest.mark.parametrize("reason", ["no_precise", "different_source", "upgrade"])
def test_new_source_and_precise_upgrade_use_existing_scalar_publication(
    controller: AppController, audio_engine_mock: Mock, reason: str
) -> None:
    mean = _mean_method()
    precise = AppTempoMethod.model_validate(_mean_data(precise=True), strict=True)
    old = mean if reason in {"no_precise", "upgrade"} else _different_source_precise()
    controller.project.sample_analysis[0] = _previous(old)
    controller.project.pad_timing_intent[0] = "legacy"
    audio_engine_mock.pad_timing_intent.return_value = "automatic"
    selected = precise if reason == "upgrade" else mean
    controller.loader._store_analysis_completion(
        0, _previous(selected).model_dump(mode="python"), key_completed=False, preserve_key=True
    )
    assert controller.project.sample_analysis[0].bpm_method == selected
    assert controller.project.sample_analysis[0].key == "Am"
    audio_engine_mock.set_pad_bpm.assert_called_with(0, 120.0)


@pytest.mark.parametrize("reason", ["manual", "tap", "automatic", "native", "stale"])
def test_optional_mean_respects_later_manual_native_and_stale_intent(
    controller: AppController, audio_engine_mock: Mock, reason: str
) -> None:
    previous = _previous(_mean_method())
    controller.project.sample_analysis[0] = previous
    audio_engine_mock.pad_timing_intent.return_value = (
        "automatic" if reason == "native" else "legacy"
    )
    if reason in {"manual", "tap"}:
        controller.project.manual_bpm[0] = 119.25
        controller.project.pad_timing_intent[0] = reason
    elif reason == "automatic":
        controller.project.pad_timing_intent[0] = "automatic"
    controller.loader._finish_loaded_timing(
        0,
        {
            "analysis_kind": "tempo_only",
            "analysis": {"bpm_method": _mean_method().model_dump_json()},
        },
        new_assignment=False,
        restored_assignment=False,
        timing_stale=reason == "stale",
    )
    assert controller.project.sample_analysis[0] is previous


@pytest.mark.parametrize("current_request", [None, 12])
def test_cancelled_or_superseded_mean_completion_does_not_publish(
    controller: AppController, audio_engine_mock: Mock, current_request: int | None
) -> None:
    previous = _previous(_mean_method())
    controller.project.sample_analysis[0] = previous
    if current_request is not None:
        controller.loader._analysis_request_ids[0] = current_request
    audio_engine_mock.set_pad_bpm.reset_mock()
    controller.loader._handle_task_success(
        0,
        {
            "task": "analysis",
            "request_id": 11,
            "analysis": _previous(_mean_method()).model_dump(mode="python"),
        },
    )
    assert controller.project.sample_analysis[0] is previous
    audio_engine_mock.set_pad_bpm.assert_not_called()


def _damaged_fresh_analysis(damage: str, encoding: str) -> dict:
    data = _mean_data()
    scalar = 120.0
    if damage in {"ratio", "mask"}:
        _damage_local_mean(data, damage)
    elif damage == "source":
        _damage_source_selection(data, "mono")
    elif damage == "scalar":
        scalar = 119.0
    else:
        data = _conflict_data()
    if damage in {"scalar", "unavailable"}:
        method = AppTempoMethod.model_validate(data, strict=True)
        assert method.bpm != scalar
    else:
        with pytest.raises(ValidationError):
            AppTempoMethod.model_validate(data, strict=True)
    method_value = json.dumps(data) if encoding == "json" else data
    return {
        "bpm": scalar,
        "key": "Bm",
        "key_status": "ready",
        "beat_grid": {"beats": [1.0], "downbeats": [], "bars": []},
        "bpm_method": method_value,
    }


@pytest.mark.parametrize("damage", ["ratio", "mask", "source", "scalar", "unavailable"])
@pytest.mark.parametrize("encoding", ["dict", "json"])
def test_fresh_task_refuses_invalid_provenance_before_scalar_publication_and_keeps_key(
    controller: AppController, audio_engine_mock: Mock, damage: str, encoding: str
) -> None:
    _seed(controller, audio_engine_mock)
    precise = _v4_method()
    previous = _previous(precise).model_copy(update={"key": "Em"})
    controller.project.sample_analysis[0] = previous
    controller.loader.analyze_sample_async(0)
    candidate = _damaged_fresh_analysis(damage, encoding)
    # Recovery remains intentionally lenient; only a fresh completion closes this seam.
    historical = SampleAnalysis.model_validate(candidate)
    assert historical.bpm == candidate["bpm"]
    assert (historical.bpm_method is None) is (damage != "unavailable")
    audio_engine_mock.set_pad_bpm.reset_mock()
    _poll(
        controller,
        audio_engine_mock,
        {
            "type": "task_success",
            "task": "analysis",
            "id": 0,
            "request_id": 11,
            "analysis": candidate,
        },
    )
    retained = controller.project.sample_analysis[0]
    assert retained.bpm == previous.bpm
    assert retained.beat_grid == previous.beat_grid
    assert retained.bpm_method is precise
    assert retained.key == "Bm"
    assert controller.project.pad_key_intent[0].source.raw_key == "Bm"
    assert "fresh bpm provenance" in controller.session.sample_analysis_errors[0].lower()
    assert 0 not in controller.loader._analysis_request_ids
    audio_engine_mock.set_pad_bpm.assert_not_called()


def test_invalid_fresh_provenance_without_previous_analysis_keeps_only_source_key_owner(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _seed(controller, audio_engine_mock)
    controller.project.sample_analysis[0] = None
    controller.loader.analyze_sample_async(0)
    audio_engine_mock.set_pad_bpm.reset_mock()
    _poll(
        controller,
        audio_engine_mock,
        {
            "type": "task_success",
            "task": "analysis",
            "id": 0,
            "request_id": 11,
            "analysis": _damaged_fresh_analysis("ratio", "json"),
        },
    )
    assert controller.project.sample_analysis[0] is None
    assert controller.project.pad_key_intent[0].source.raw_key == "Bm"
    assert "Fresh BPM provenance" in controller.session.sample_analysis_errors[0]
    audio_engine_mock.set_pad_bpm.assert_not_called()
