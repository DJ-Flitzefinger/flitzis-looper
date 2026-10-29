"""Saved nominal evidence replays independent timing without runtime or label access."""

import base64
import hashlib
import math
import struct
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis import app_tempo_prior, metrical_prior_setup
from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.bpm_range_options import BpmRangeOptions
from flitzis_looper.copy_snapshot import CopySnapshotData, capture_copy_snapshot
from flitzis_looper.models import BeatGrid, PadContentIdentity, ProjectState, SampleAnalysis
from tests.flitzis_looper.controller.test_adaptive_tempo_probe import _v4_method
from tests.flitzis_looper.controller.test_bpm_range import _range_data
from tests.flitzis_looper.controller.test_corroborated_tempo import _v6_method
from tests.flitzis_looper.controller.test_inferred_tempo_probe import _v5_method
from tests.flitzis_looper.controller.test_neural_tempo_restore import _method, _v2_method

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def _packet(raw: bytes, shape: list[int], encoding: str) -> dict:
    return {
        "encoding": encoding,
        "shape": shape,
        "sha256": hashlib.sha256(raw).hexdigest(),
        "data": base64.b64encode(raw).decode("ascii"),
    }


def _set_distribution(prior: dict, weights: dict[int, float]) -> None:
    # Identical analytically known rows have this same exact global mean.
    # This fixture neither runs the classifier nor repeats its aggregation code.
    values = [weights.get(index + 30, 0.0) for index in range(256)]
    patches = prior["frontend"]["feature_shape"][0]
    prior["probabilities"] = _packet(
        struct.pack("<256f", *values) * patches, [patches, 256], "float32-le/base64"
    )
    prior["global"] = _packet(struct.pack("<256d", *values), [256], "float64-le/base64")
    prior["global"]["first_argmax_class"] = max(range(256), key=values.__getitem__) + 30


def _prior_data(*, chosen_bpm: float = 60.0, version: int = 4) -> dict:
    data = _range_data(chosen_bpm=chosen_bpm, version=version)
    range_evidence = data.pop("bpm_range")
    if version == 5:
        # This original NN fixture is320s. Give QM its own complete perfect
        # observations and historical middle60% support at that same extent.
        qm = range_evidence["selection"]["qm"]
        period = 60.0 / chosen_bpm
        qm["raw_beat_frames"] = [
            (0.0625 + period * k) / (512 / 44100) for k in range(int(320 / period))
        ]
        qm["raw_downbeat_indices"] = list(range(0, len(qm["raw_beat_frames"]), 4))
        core = qm["estimate"]
        common = dict(core["selected"], window_seconds=[64.0, 256.0], retained=int(192 / period))
        core.update(selected=common, regions=[common])
        core["integer_probe"].update(
            common_region=common,
            windows=[
                dict(common, window_seconds=bounds, retained=int(16 / period))
                for bounds in ([64.0, 80.0], [240.0, 256.0])
            ],
            pivot_raw_index=(160.0 - period / 2) / period,
            pivot_seconds=160.0625 - period / 2,
            window_gap_seconds=160.0 + period,
        )
    input_frames, input_rate = data["frame_count"], data["sample_rate_hz"]
    target = (input_frames * 11025 + input_rate - 1) // input_rate
    patches = 1 + (max(256, 1 + target // 512) - 256) // 128
    pins = metrical_prior_setup
    prior = {
        "profile": {
            "profile_id": pins.PROFILE_ID,
            "profile_sha256": "1" * 64,
            "numeric_profile_sha256": pins.NUMERIC_PROFILE_SHA256,
            "python_version": "3.12.13",
            "python_executable_sha256": pins.PYTHON_SHA256,
            "python_dll_sha256": pins.PYTHON_DLL_SHA256,
            "node_version": "v25.5.0",
            "node_executable_sha256": pins.NODE_SHA256,
            "tfjs_version": "4.22.0",
            "graph_sha256": "f93693c5a913b69aa378438665937600c09286f57714359155f2763496aba184",
            "shard_sha256": "f7688e8868706813cd74610442f9923fca51162c1ebde6cf7fecfd62ebd24a15",
            "frontend_reference_sha256": pins.E4_SHA256,
            "frontend_runner_sha256": pins.FRONTEND_SHA256,
            "inference_reference_sha256": pins.REFERENCE_NODE_SHA256,
            "inference_runner_sha256": pins.INFERENCE_SHA256,
        },
        "input": {
            "source_sha256": data["source_sha256"],
            "mono_sha256": data["mono_sha256"],
            "sample_rate_hz": input_rate,
            "frame_count": input_frames,
            "origin_seconds": 0.0,
            "downmix_revision": "arithmetic-channel-mean-f64-v1",
        },
        "frontend": {
            "frontend_id": "tempocnn-author-482c64ad-librosa011-global-std-f16-v1",
            "sample_rate_hz": 11025,
            "frame_count": target,
            "origin_seconds": 0.0,
            "mono_sha256": "2" * 64,
            "feature_shape": [patches, 40, 256, 1],
            "feature_sha256": "3" * 64,
            "normalization": {
                "scope": "entire-overlapping-window-tensor",
                "precision": "float64",
                "roundtrip": "float16-to-float32",
                "mean_f64_le_hex": struct.pack("<d", 1.0).hex(),
                "std_f64_le_hex": struct.pack("<d", 2.0).hex(),
            },
        },
        "execution": {"origin": "computed", "device": "cpu"},
        # Synthetic schema fixtures, not actual musical/runtime observations.
        "resources": {
            "native_live_bytes": 4 * 1024 * 1024,
            "aggregate_limit_bytes": 536870912,
            "frontend_admitted_child_peak_bytes": 10 * 1024 * 1024,
            "frontend_observed_child_peak_bytes": 8 * 1024 * 1024,
            "classifier_observed_child_peak_bytes": 7 * 1024 * 1024,
            "measurement": "windows-private-and-working-set-peaks",
            "aggregate_claim": True,
        },
    }
    _set_distribution(prior, {int(chosen_bpm): 1.0})
    data.update(
        schema_version=8,
        backend="beat_this_qm_tempocnn",
        policy="neural-qm-nominal-prior-v8",
        nominal_selection={
            "neural_result": range_evidence["primary"],
            **range_evidence["selection"],
            "prior": prior,
        },
    )
    return data


@pytest.mark.parametrize("version", [4, 5])
@pytest.mark.parametrize("chosen", [60.0, 240.0])
def test_prior_replays_both_octaves_and_original_nn_versions(version: int, chosen: float) -> None:
    method = AppTempoMethod.model_validate(
        _prior_data(chosen_bpm=chosen, version=version), strict=True
    )
    original = _v4_method() if version == 4 else _v5_method()
    assert method.bpm == chosen
    assert method.nominal_selection.neural_result.bpm == 120.0
    assert method.raw_beats == original.raw_beats
    assert method.raw_downbeats == original.raw_downbeats
    assert AppTempoMethod.model_validate_json(method.model_dump_json()) == method
    assert method.model_dump_json().count('"raw_beats"') == 1


def test_prior_primary_projection_never_serializes_or_copies_raw_arrays(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    data = _prior_data()
    original_validate = AppTempoMethod._validate_estimate
    raw_ids = []

    def inspect(method):
        raw_ids.append(id(method.raw_beats))
        original_validate(method)

    def forbidden(*_args, **_kwargs):
        message = "Saved prior validation must not serialize raw arrays or query a runtime"
        raise AssertionError(message)

    monkeypatch.setattr(AppTempoMethod, "_validate_estimate", inspect)
    monkeypatch.setattr(AppTempoMethod, "model_dump", forbidden)
    monkeypatch.setattr(metrical_prior_setup, "verify_profile", forbidden)
    assert AppTempoMethod.model_validate(data, strict=True).bpm == 60.0
    assert len(raw_ids) == 2
    assert raw_ids[0] == raw_ids[1]


@pytest.mark.parametrize("change", ["shape", "complete_extent"])
def test_prior_rejects_geometry_before_decoding_packed_probabilities(
    monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    data = _prior_data()
    prior = data["nominal_selection"]["prior"]
    if change == "shape":
        prior["frontend"]["feature_shape"][0] = 1025
    else:
        prior["input"]["frame_count"] += 48000

    def forbidden(*_args):
        message = "Invalid geometry reached packed allocation"
        raise AssertionError(message)

    monkeypatch.setattr(app_tempo_prior, "unpack_packet", forbidden)
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize(
    "forgery",
    [
        "cdf_hash",
        "cdf_shape",
        "global_bits",
        "argmax",
        "base64",
        "row_probability",
        "softmax_sum",
        "tie",
        "prefers_nn",
        "source",
        "raw_qm",
        "raw_nn",
        "profile",
        "normalization",
        "scalar",
        "status",
        "drift",
        "nested_range",
        "nested_qm",
    ],
)
def test_prior_rejects_forged_saved_evidence(forgery: str) -> None:  # noqa: C901, PLR0912
    data = _prior_data()
    evidence = data["nominal_selection"]
    prior = evidence["prior"]
    if forgery == "cdf_hash":
        prior["probabilities"]["sha256"] = "0" * 64
    elif forgery == "cdf_shape":
        prior["probabilities"]["shape"][1] = 255
    elif forgery == "global_bits":
        values = [0.0] * 256
        values[30] = math.nextafter(1.0, 0.0)
        prior["global"] = _packet(struct.pack("<256d", *values), [256], "float64-le/base64")
        prior["global"]["first_argmax_class"] = 60
    elif forgery == "argmax":
        prior["global"]["first_argmax_class"] = 120
    elif forgery == "base64":
        # Nonzero ignored pad bits decode identically but are not canonical.
        value = prior["global"]["data"]
        prior["global"]["data"] = value[:-2] + "B="
    elif forgery == "row_probability":
        _set_distribution(prior, {60: 1.5, 120: -0.5})
    elif forgery == "softmax_sum":
        _set_distribution(prior, {60: 0.5})
    elif forgery == "tie":
        _set_distribution(prior, {60: 0.5, 120: 0.5})
    elif forgery == "prefers_nn":
        _set_distribution(prior, {120: 1.0})
    elif forgery == "source":
        prior["input"]["mono_sha256"] = "0" * 64
    elif forgery == "raw_qm":
        evidence["qm"]["raw_beat_frames"][20] += 10.0
    elif forgery == "raw_nn":
        data["raw_beats"][40] += 0.1
    elif forgery == "profile":
        prior["profile"]["numeric_profile_sha256"] = "0" * 64
    elif forgery == "normalization":
        prior["frontend"]["normalization"]["std_f64_le_hex"] = struct.pack("<d", math.nan).hex()
    elif forgery == "scalar":
        data["bpm"] = math.nextafter(60.0, math.inf)
    elif forgery == "status":
        data["status"] = "single_region_estimate"
    elif forgery == "drift":
        evidence["period_drift_seconds"] = 0.001
    elif forgery == "nested_range":
        data["bpm_range"] = None
    elif forgery == "nested_qm":
        data["qm_corroboration"] = None
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


def test_prior_requires_half_double_timing_even_if_classifier_prefers_quarter() -> None:
    with pytest.raises(ValidationError, match="half/double agreement"):
        AppTempoMethod.model_validate(_prior_data(chosen_bpm=30.0), strict=True)


@pytest.mark.parametrize("bounds", [(60.0, 120.0), (60.0, 119.0), (61.0, 120.0)])
def test_prior_replays_captured_range_for_both_candidates(bounds: tuple[float, float]) -> None:
    data = _prior_data()
    data["nominal_selection"]["range_constraint"] = dict(
        zip(("minimum_bpm", "maximum_bpm"), bounds, strict=True)
    )
    if bounds == (60.0, 120.0):
        assert AppTempoMethod.model_validate(data, strict=True).bpm == 60.0
    else:
        with pytest.raises(ValidationError, match="captured request range"):
            AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("cached", [False, True])
def test_prior_origin_and_resource_claims_cannot_be_upgraded(*, cached: bool) -> None:
    data = _prior_data()
    prior = data["nominal_selection"]["prior"]
    if cached:
        prior["execution"] = {
            "origin": "cached_observation",
            "device": "cpu",
            "cached_observation": {
                "runner_sha256": metrical_prior_setup.REFERENCE_NODE_SHA256,
                "cdf_sha256": app_tempo_prior.CDF_SHA256,
                "input_manifest_sha256": app_tempo_prior.INPUT_SHA256,
                "frontend_seal_sha256": app_tempo_prior.FRONTEND_SEAL_SHA256,
                "numeric_profile_sha256": metrical_prior_setup.NUMERIC_PROFILE_SHA256,
            },
        }
        prior["resources"].update(measurement="cached-rss-only", aggregate_claim=False)
    method = AppTempoMethod.model_validate(data, strict=True)
    assert method.nominal_selection.prior.execution.origin == (
        "cached_observation" if cached else "computed"
    )
    prior["resources"].update(
        measurement="windows-private-and-working-set-peaks" if cached else "cached-rss-only",
        aggregate_claim=cached,
    )
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


def test_computed_prior_rejects_even_null_cache_proof_and_excess_memory() -> None:
    data = _prior_data()
    prior = data["nominal_selection"]["prior"]
    prior["execution"]["cached_observation"] = None
    with pytest.raises(ValidationError, match="omit historical"):
        AppTempoMethod.model_validate(data, strict=True)
    del prior["execution"]["cached_observation"]
    prior["resources"]["native_live_bytes"] = 536870912
    with pytest.raises(ValidationError, match="exceeds512MiB"):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("version", range(1, 8))
def test_legacy_versions_reject_even_null_nominal_selection(version: int) -> None:
    methods = {1: _method, 2: _v2_method, 4: _v4_method, 5: _v5_method, 6: _v6_method}
    data = _range_data() if version == 7 else methods.get(version, _v2_method)().model_dump()
    if version == 3:
        data.update(schema_version=3, policy="neural-stable-regions-v3")
    assert "nominal_selection" not in data
    data["nominal_selection"] = None
    with pytest.raises(ValidationError, match="version eight"):
        AppTempoMethod.model_validate(data, strict=True)


def test_prior_copy_restore_keeps_saved_evidence_manual_key_and_grid(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    method = AppTempoMethod.model_validate(_prior_data(), strict=True)

    def forbidden(*_args, **_kwargs):
        message = "Copy/restore must not query or install a prior runtime"
        raise AssertionError(message)

    monkeypatch.setattr(metrical_prior_setup, "verify_profile", forbidden)
    project = controller.project
    project.automatic_bpm_range = BpmRangeOptions(enabled=True, minimum_bpm=200, maximum_bpm=250)
    project.sample_paths[0] = "samples/source.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    project.manual_bpm[0] = 94.0
    project.pad_timing_intent[0] = "tap"
    grid = BeatGrid(beats=[], downbeats=[], bars=[])
    project.sample_analysis[0] = SampleAnalysis(
        bpm=60.0, key="Am", beat_grid=grid, bpm_method=method
    )
    copied = CopySnapshotData.model_validate_json(capture_copy_snapshot(project, 0).payload_json)
    restored = ProjectState.model_validate_json(project.model_dump_json())
    assert copied.analysis.bpm_method == restored.sample_analysis[0].bpm_method == method
    assert copied.analysis.key == restored.sample_analysis[0].key == "Am"
    assert copied.analysis.beat_grid == restored.sample_analysis[0].beat_grid == grid
    assert restored.manual_bpm[0] == 94.0
    assert restored.pad_timing_intent[0] == "tap"
    assert not controller.loader._needs_tempo_refinement(0)
