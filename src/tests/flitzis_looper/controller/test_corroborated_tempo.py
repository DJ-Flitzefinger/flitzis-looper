"""Joint scalar evidence retains the neural refusal and an independent CPU result."""

import hashlib
import json
import math
from typing import TYPE_CHECKING, NoReturn

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.copy_snapshot import CopySnapshotData, capture_copy_snapshot
from flitzis_looper.models import BeatGrid, PadContentIdentity, SampleAnalysis
from tests.flitzis_looper.controller.test_adaptive_tempo_probe import _v4_method
from tests.flitzis_looper.controller.test_inferred_tempo_probe import _candidate_data, _v5_data

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def _phase_refusal_data(period: float = 0.5) -> dict[str, object]:
    # Equal fifths have offsets[-a,2a,0,-2a,a]. Their sum AND first moment
    # vanish, leaving the explicitly known free line while end probes differ6ms.
    anchor, amplitude = 0.0625, 0.003
    points = [(k, anchor + period * k + amplitude * (-1, 2, 0, -2, 1)[k // 40]) for k in range(200)]
    raw = [t for _, t in points]
    data = _v5_data(extra_peaks=())
    data.update(
        frame_count=100 * 48000, raw_beats=raw, raw_downbeats=[], bpm=None, status="uncorroborated"
    )
    pivot = anchor + period * 99.5
    free = [t - pivot - period * (k - 99.5) for k, t in points]

    def check(candidate_period: float) -> dict[str, object]:
        residuals = [t - pivot - candidate_period * (k - 99.5) for k, t in points]
        rms = math.sqrt(sum(r * r for r in residuals) / 200)
        return {
            "status": "fail",
            "max_residual_seconds": max(map(abs, residuals)),
            "rms_residual_seconds": rms,
            "rms_penalty_seconds": max(0.0, rms - math.sqrt(sum(r * r for r in free) / 200)),
            "window_phase_difference_seconds": abs(
                sum(residuals[-40:]) / 40 - sum(residuals[:40]) / 40
            ),
        }

    data["ordinal_fit"].update(
        outcome="insufficient",
        integer_preferred=False,
        winner=_candidate_data(raw, points, period, anchor, 100.0),
        assignments=[[k, k] for k in range(200)],
        alternatives=[
            _candidate_data(raw, [(2 * k, t) for k, t in points], period / 2, anchor, 100.0),
            _candidate_data(
                raw, [(k // 2, t) for k, t in points if k % 2 == 0], period * 2, anchor, 100.0
            ),
        ],
        harmonics=[
            {
                "factor": factor,
                "seed_period_seconds": period * factor,
                "seed_anchor_seconds": anchor,
                "outcome": "fitted",
                "alternative_index": index,
            }
            for index, factor in enumerate((0.5, 2.0))
        ],
        probe_windows=[[0.0, 20.0], [80.0, 100.0]],
        fractional_check=check(60.0 / (60.0 / period)),
        integer_check=check(0.5),
    )
    return data


def _v6_data(period: float = 0.5) -> dict[str, object]:
    data = _phase_refusal_data(period)
    descriptor = {
        "schema_version": 1,
        "original": {"sha256": data["source_sha256"], "bytes": 1024},
        "pcm": {
            "format": "f32-le-interleaved-v1",
            "channel_layout": "interleaved-channel-index-order-v1",
            "full_frames": 4410000,
            "channels": 2,
            "full_bytes": 35280000,
            "rate_hz": 44100,
            "source_zero_bits": "0000000000000000",
            "provenance": {"codec": "fixture"},
            "mono_revision": "arithmetic-channel-mean-f64-v1",
            "interleaved_sha256": "a" * 64,
            "mono_sha256": "b" * 64,
        },
    }
    encoded = json.dumps(descriptor, sort_keys=True, separators=(",", ":"))
    qm = _v4_method().model_dump(mode="python")
    core = {key: qm[key] for key in ("status", "bpm", "selected", "regions", "integer_probe")}
    core["kernel_version"] = 4
    core["integer_probe"]["pivot_seconds"] += 0.0625
    data.update(
        schema_version=6,
        backend="beat_this_qm_dsp",
        policy="neural-qm-same-period-v6",
        bpm=120.0,
        status="estimated",
    )
    data["qm_corroboration"] = {
        "neural_result": {
            "schema_version": 5,
            "policy": "neural-inferred-ordinals-v5",
            "status": "uncorroborated",
            "bpm": None,
        },
        "source": {
            "source_sha256": data["source_sha256"],
            "decoder_identity": hashlib.sha256(encoded.encode()).hexdigest(),
            "decoder_descriptor_json": encoded,
            "loaded_sample_rate_hz": 48000,
            "loaded_channels": 2,
            "loaded_frame_count": 4800000,
            "loaded_origin_seconds": 0.0,
            "loaded_interleaved_sha256": "c" * 64,
            "verified_nn_mono_sha256": data["mono_sha256"],
        },
        "qm": {
            "backend": "qm_dsp",
            "device": "cpu",
            "configuration": {
                "alpha": 0.9,
                "hop_size": 128,
                "input_tempo": 120.0,
                "max_bin_hz": 50.0,
                "step_secs": 0.01161,
                "tightness": 4.0,
                "viterbi_sigma": 8.0,
                "window_length": 512,
            },
            "normal_mono_f32_sha256": "d" * 64,
            "downmix_revision": "normal-analysis-stereo-f32-add-multiply-half-v1",
            "analyzer": {
                "sample_rate_hz": 44100,
                "frame_count": 4410000,
                "origin_seconds": 0.0,
                "f64_le_sha256": "e" * 64,
                "resampler": "rubato-fft-1.0-44100-delay-trim-tail-flush-v1",
            },
            "odf_hop_samples": 512,
            "raw_beat_frames": [(0.0625 + 0.5 * k) / (512 / 44100) for k in range(200)],
            "raw_downbeat_indices": list(range(0, 200, 4)),
            "estimate": core,
        },
        "period_drift_seconds": abs(0.5 - period) * (100.0 / period),
    }
    return data


def _v6_method() -> AppTempoMethod:
    return AppTempoMethod.model_validate(_v6_data(), strict=True)


@pytest.mark.parametrize("rate", [48000, 44100])
def test_joint_scalar_replays_two_distinct_evidences_without_promoting_neural_phase(
    rate: int,
) -> None:
    data = _v6_data()
    evidence = data["qm_corroboration"]
    if rate == 44100:
        data.update(sample_rate_hz=44100, frame_count=4410000)
        evidence["source"].update(loaded_sample_rate_hz=44100, loaded_frame_count=4410000)
        evidence["qm"]["analyzer"]["resampler"] = "identity-44100-v1"
    method = AppTempoMethod.model_validate(data, strict=True)
    assert method.bpm == 120.0
    assert method.ordinal_fit.outcome == "insufficient"
    assert method.ordinal_fit.fractional_check.status == "fail"
    assert method.qm_corroboration.qm.estimate.bpm == 120.0
    encoded = method.model_dump_json()
    assert encoded.count('"raw_beats":') == 1
    assert encoded.count('"raw_beat_frames":') == 1
    assert AppTempoMethod.model_validate_json(encoded, strict=True) == method


def test_joint_scalar_preserves_a_short_whole_source_qm_status_and_rejects_a_mismatch() -> None:
    # This 40-second original has two genuine ten-second NN probes. Offsets
    # with zero mean and first moment preserve 60 BPM with a 6 ms phase refusal.
    offsets = [-0.003] * 12 + [0.01575] * 8 + [-0.01575] * 8 + [0.003] * 12
    points = [(k, 0.0625 + k + offset) for k, offset in enumerate(offsets)]
    raw = [t for _, t in points]
    data = _v6_data()
    data.update(frame_count=40 * 48000, raw_beats=raw, bpm=60.0, status="single_region_estimate")
    check = {
        "status": "fail",
        "max_residual_seconds": 0.01575,
        "rms_residual_seconds": math.sqrt(sum(r * r for r in offsets) / 40),
        "rms_penalty_seconds": 0.0,
        "window_phase_difference_seconds": 0.006,
    }
    fit = data["ordinal_fit"]
    fit.update(
        winner=_candidate_data(raw, points, 1.0, 0.0625, 40.0),
        assignments=[[k, k] for k in range(40)],
        alternatives=[
            _candidate_data(raw, [(2 * k, t) for k, t in points], 0.5, 0.0625, 40.0),
            _candidate_data(raw, [(k // 2, t) for k, t in points if k % 2 == 0], 2.0, 0.0625, 40.0),
        ],
        probe_windows=[[0.0, 12.0], [28.0, 40.0]],
        fractional_check=dict(check),
        integer_check=dict(check),
        integer_candidate_bpm=60.0,
    )
    for harmonic, period in zip(fit["harmonics"], (0.5, 2.0), strict=True):
        harmonic["seed_period_seconds"] = period
    source = data["qm_corroboration"]["source"]
    qm = data["qm_corroboration"]["qm"]
    source["loaded_frame_count"] = 40 * 48000
    descriptor = json.loads(source["decoder_descriptor_json"])
    descriptor["pcm"].update(full_frames=40 * 44100, full_bytes=40 * 44100 * 8)
    source["decoder_descriptor_json"] = json.dumps(
        descriptor, sort_keys=True, separators=(",", ":")
    )
    source["decoder_identity"] = hashlib.sha256(
        source["decoder_descriptor_json"].encode()
    ).hexdigest()
    qm["analyzer"]["frame_count"] = 40 * 44100
    qm["raw_beat_frames"] = [(0.0625 + k) / (512 / 44100) for k in range(40)]
    qm["raw_downbeat_indices"] = list(range(0, 40, 4))
    times = [frame * (512 / 44100) for frame in qm["raw_beat_frames"]]
    whole = {
        "window_seconds": [0.0, 40.0],
        "bpm": 60.0,
        "retained": 40,
        "excluded_raw_indices": [],
        "max_residual_seconds": 0.0,
        "rms_residual_seconds": 0.0,
    }
    estimate = qm["estimate"]
    estimate.update(status="single_region_estimate", bpm=60.0, selected=whole, regions=[whole])
    estimate["integer_probe"].update(
        candidate_bpm=60.0,
        base_bpm=60.0,
        common_region=whole,
        windows=[
            {
                **whole,
                "window_seconds": [times[a], math.nextafter(times[b], math.inf)],
                "retained": 17,
            }
            for a, b in ((0, 16), (23, 39))
        ],
        window_gap_seconds=times[23] - times[16],
        pivot_raw_index=19.5,
        pivot_seconds=19.5625,
    )
    method = AppTempoMethod.model_validate(data, strict=True)
    assert method.status == method.qm_corroboration.qm.estimate.status == "single_region_estimate"
    assert method.bpm == 60.0
    data["status"] = "estimated"
    with pytest.raises(ValidationError, match="independently admitted QM value"):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize(
    "damage",
    [
        "source",
        "mono",
        "decoder_hash",
        "crop",
        "hop",
        "rate",
        "origin",
        "downmix",
        "configuration",
        "bool_index",
    ],
)
def test_joint_source_and_qm_receipts_reject_forged_or_incomplete_links(damage: str) -> None:
    data = _v6_data()
    evidence = data["qm_corroboration"]
    source, qm = evidence["source"], evidence["qm"]
    if damage == "source":
        source["source_sha256"] = "f" * 64
    elif damage == "mono":
        source["verified_nn_mono_sha256"] = "f" * 64
    elif damage == "decoder_hash":
        source["decoder_identity"] = "f" * 64
    elif damage == "crop":
        data["frame_count"] += 1
        source["loaded_frame_count"] += 1
    elif damage == "hop":
        qm["odf_hop_samples"] = 511
    elif damage == "rate":
        source["loaded_sample_rate_hz"] = 44100
    elif damage == "origin":
        qm["analyzer"]["origin_seconds"] = 0.001
    elif damage == "downmix":
        qm["downmix_revision"] = "normal-analysis-mono-identity-f32-v1"
    elif damage == "configuration":
        qm["configuration"]["input_tempo"] = 123.0
    else:
        qm["raw_downbeat_indices"][0] = False
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


def test_joint_decoder_digest_cannot_escape_original_source_binding() -> None:
    data = _v6_data()
    source = data["qm_corroboration"]["source"]
    descriptor = json.loads(source["decoder_descriptor_json"])
    descriptor["original"]["sha256"] = "f" * 64
    source["decoder_descriptor_json"] = json.dumps(
        descriptor, sort_keys=True, separators=(",", ":")
    )
    source["decoder_identity"] = hashlib.sha256(
        source["decoder_descriptor_json"].encode()
    ).hexdigest()
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize(
    "damage", ["top_value", "qm_value", "drift", "phase_metric", "neural_pass"]
)
def test_joint_selection_does_not_bypass_either_independent_numeric_result(damage: str) -> None:
    data = _v6_data()
    if damage == "top_value":
        data["bpm"] = 121.0
    elif damage == "qm_value":
        data["qm_corroboration"]["qm"]["estimate"]["bpm"] = 121.0
    elif damage == "drift":
        data["qm_corroboration"]["period_drift_seconds"] = 0.004
    elif damage == "phase_metric":
        data["ordinal_fit"]["fractional_check"]["window_phase_difference_seconds"] = 0.005
    else:
        old = _v5_data()
        data.update(
            raw_beats=old["raw_beats"],
            raw_downbeats=old["raw_downbeats"],
            frame_count=old["frame_count"],
            ordinal_fit=old["ordinal_fit"],
        )
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


def test_joint_same_period_guard_checks_full_extent_without_octave_folding() -> None:
    data = _v6_data(0.50003)
    data["qm_corroboration"]["period_drift_seconds"] = 0.004
    with pytest.raises(ValidationError, match="full-extent same-period"):
        AppTempoMethod.model_validate(data, strict=True)


def test_joint_validation_never_roundtrips_the_original_neural_arrays(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    data = _v6_data()

    def forbidden_dump(*_args: object, **_kwargs: object) -> NoReturn:
        message = "Numeric projection must share existing neural arrays"
        raise AssertionError(message)

    monkeypatch.setattr(AppTempoMethod, "model_dump", forbidden_dump)
    assert AppTempoMethod.model_validate(data, strict=True).bpm == 120.0


@pytest.mark.parametrize("version", [1, 2, 3, 4, 5])
def test_old_method_versions_reject_new_field_presence_even_null(version: int) -> None:
    data = _v5_data() if version == 5 else _v4_method().model_dump(mode="python")
    if version < 5:
        data.update(
            schema_version=version,
            policy=f"neural-stable-regions-v{version}",
            bpm=None,
            status="insufficient",
            selected=None,
            regions=[],
            integer_probe=None,
        )
    method = AppTempoMethod.model_validate(data, strict=True)
    assert "qm_corroboration" not in method.model_dump(mode="python")
    data["qm_corroboration"] = None
    with pytest.raises(ValidationError, match="require version six"):
        AppTempoMethod.model_validate(data, strict=True)


def test_joint_copy_and_restore_keep_both_results_and_enforce_selected_bpm(
    controller: AppController,
) -> None:
    method = _v6_method()
    project = controller.project
    project.sample_paths[0] = "samples/source.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    analysis = SampleAnalysis(
        bpm=120.0, key="Am", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]), bpm_method=method
    )
    project.sample_analysis[0] = analysis
    assert SampleAnalysis.model_validate_json(analysis.model_dump_json()).bpm_method == method
    assert not controller.loader._needs_tempo_refinement(0)
    copied = capture_copy_snapshot(project, 0)
    assert CopySnapshotData.model_validate_json(copied.payload_json).analysis.bpm_method == method
    damaged = json.loads(copied.payload_json)
    damaged["analysis"]["bpm"] = 121.0
    with pytest.raises(ValidationError):
        CopySnapshotData.model_validate(damaged, strict=True)
