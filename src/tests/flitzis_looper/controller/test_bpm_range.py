"""Request range evidence preserves independent scalar, source and performer owners."""

from copy import deepcopy
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.bpm_range_options import BpmRangeOptions
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.copy_snapshot import CopySnapshotData, capture_copy_snapshot
from flitzis_looper.models import (
    BeatGrid,
    PadContentIdentity,
    ProjectState,
    SampleAnalysis,
    SessionState,
)
from tests.flitzis_looper.controller.test_adaptive_tempo_probe import _v4_method
from tests.flitzis_looper.controller.test_corroborated_tempo import _v6_data, _v6_method
from tests.flitzis_looper.controller.test_inferred_tempo_probe import _v5_method
from tests.flitzis_looper.controller.test_neural_tempo_restore import _method, _v2_method

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def _range_data(*, chosen_bpm: float = 60.0, selected: bool = True, version: int = 4) -> dict:
    data = (
        _v6_method() if version == 6 else _v5_method() if version == 5 else _v4_method()
    ).model_dump(mode="python")
    minimum, maximum = (50.0, 100.0) if chosen_bpm < 120 else (200.0, 260.0)
    evidence = {
        "minimum_bpm": minimum,
        "maximum_bpm": maximum,
        "primary": {
            key: data[key] for key in ("schema_version", "backend", "policy", "status", "bpm")
        },
        "outcome": "qm_half_double" if selected else "no_supported_candidate",
    }
    if selected:
        joint = _v6_data()["qm_corroboration"]
        source, qm = deepcopy(joint["source"]), deepcopy(joint["qm"])
        # Known perfect raw observations have their own independent60/240BPM
        # kernel evidence; no production assignment/search helper generates them.
        period = 60.0 / chosen_bpm
        qm["raw_beat_frames"] = [
            (0.0625 + period * k) / (512 / 44100) for k in range(int(100 / period))
        ]
        qm["raw_downbeat_indices"] = list(range(0, len(qm["raw_beat_frames"]), 4))
        core = qm["estimate"]
        common = {
            "window_seconds": [20.0, 80.0],
            "bpm": chosen_bpm,
            "retained": int(60 / period),
            "excluded_raw_indices": [],
            "max_residual_seconds": 0.0,
            "rms_residual_seconds": 0.0,
        }
        core.update(bpm=chosen_bpm, selected=common, regions=[common])
        core["integer_probe"].update(
            candidate_bpm=chosen_bpm,
            base_bpm=chosen_bpm,
            windows=[
                {**common, "window_seconds": bounds, "retained": int(16 / period)}
                for bounds in ([20.0, 36.0], [64.0, 80.0])
            ],
            common_region=common,
            pivot_seconds=50.0625 - period / 2,
            window_gap_seconds=28.0 + period,
        )
        # The common mask begins at20s and ends one period before80s.
        core["integer_probe"]["pivot_raw_index"] = (50.0 - period / 2) / period
        source["loaded_frame_count"] = data["frame_count"]
        qm["analyzer"]["frame_count"] = data["frame_count"] * 44100 // data["sample_rate_hz"]
        evidence["selection"] = {"source": source, "qm": qm, "period_drift_seconds": 0.0}
    data.update(
        schema_version=7,
        backend="beat_this_bpm_range",
        policy="neural-user-range-v7",
        status="estimated" if selected else "insufficient",
        bpm=chosen_bpm if selected else None,
        bpm_range=evidence,
    )
    return data


@pytest.mark.parametrize("chosen", [60.0, 240.0])
def test_range_replays_both_half_double_directions_without_changing_primary_arrays(
    chosen: float,
) -> None:
    data = _range_data(chosen_bpm=chosen)
    method = AppTempoMethod.model_validate(data, strict=True)
    assert method.bpm == chosen
    assert method.bpm_range.primary.bpm == 120.0
    assert method.raw_beats == _v4_method().raw_beats
    assert AppTempoMethod.model_validate_json(method.model_dump_json()) == method
    assert method.model_dump_json().count('"raw_beats"') == 1


@pytest.mark.parametrize("bound", ["minimum_bpm", "maximum_bpm"])
def test_range_selected_value_accepts_inclusive_bounds(bound: str) -> None:
    data = _range_data()
    data["bpm_range"][bound] = 60.0
    assert AppTempoMethod.model_validate(data).bpm == 60.0


def test_range_refuses_a_real_own_kernel_quarter_tempo() -> None:
    data = _range_data(chosen_bpm=30.0)
    data["bpm_range"].update(minimum_bpm=30.0, maximum_bpm=40.0)
    with pytest.raises(ValidationError, match="half/double agreement"):
        AppTempoMethod.model_validate(data, strict=True)


def test_range_primary_projection_shares_arrays_without_serialization(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    data = _range_data()

    def forbidden(*_args, **_kwargs):
        message = "Range validation must not dump/reparse original arrays"
        raise AssertionError(message)

    monkeypatch.setattr(AppTempoMethod, "model_dump", forbidden)
    assert AppTempoMethod.model_validate(data, strict=True).bpm == 60.0


@pytest.mark.parametrize("version", [4, 5, 6])
def test_range_rejection_preserves_original_success_without_new_qm_selection(version: int) -> None:
    method = AppTempoMethod.model_validate(
        _range_data(selected=False, version=version), strict=True
    )
    assert method.bpm is None
    assert method.status == "insufficient"
    assert "selection" not in method.model_dump()["bpm_range"]
    assert (method.qm_corroboration is not None) is (version == 6)


def test_range_cannot_reuse_a_joint_primary_as_independent_nn_success() -> None:
    with pytest.raises(ValidationError, match="independently successful neural primary"):
        AppTempoMethod.model_validate(_range_data(version=6), strict=True)


@pytest.mark.parametrize(
    "forgery",
    [
        "in_range_primary",
        "selected_value",
        "status",
        "raw_qm",
        "source",
        "crop",
        "drift",
        "null_selection",
        "base_header",
        "legacy_joint",
    ],
)
def test_range_rejects_forged_selection_or_original_provenance(forgery: str) -> None:
    data = _range_data(selected=forgery != "null_selection")
    evidence = data["bpm_range"]
    if forgery == "in_range_primary":
        evidence["maximum_bpm"] = 120.0
    elif forgery == "selected_value":
        data["bpm"] = 60.001
    elif forgery == "status":
        data["status"] = "single_region_estimate"
    elif forgery == "raw_qm":
        evidence["selection"]["qm"]["raw_beat_frames"][20] += 10.0
    elif forgery == "source":
        evidence["selection"]["source"]["verified_nn_mono_sha256"] = "f" * 64
    elif forgery == "crop":
        evidence["selection"]["qm"]["analyzer"]["frame_count"] -= 1
    elif forgery == "drift":
        evidence["selection"]["period_drift_seconds"] = 0.004
    elif forgery == "null_selection":
        evidence["selection"] = None
    elif forgery == "base_header":
        evidence["primary"]["bpm"] = 120.001
    else:
        data["qm_corroboration"] = None
    with pytest.raises(ValidationError):
        AppTempoMethod.model_validate(data, strict=True)


@pytest.mark.parametrize("method", [_method, _v4_method, _v5_method, _v6_method])
def test_old_methods_omit_range_and_reject_even_null_range_presence(method) -> None:
    original = method()
    encoded = original.model_dump_json()
    assert "bpm_range" not in encoded
    assert AppTempoMethod.model_validate_json(encoded).model_dump_json() == encoded
    with pytest.raises(ValidationError, match="version seven"):
        AppTempoMethod.model_validate(original.model_dump() | {"bpm_range": None})


@pytest.mark.parametrize("version", [2, 3])
def test_old_counterprobe_versions_reject_null_range_presence(version: int) -> None:
    original = _v2_method(version)
    assert "bpm_range" not in original.model_dump_json()
    with pytest.raises(ValidationError, match="version seven"):
        AppTempoMethod.model_validate(original.model_dump() | {"bpm_range": None})


def test_range_copy_restore_ignores_current_options_and_preserves_manual_key(
    controller: AppController,
) -> None:
    method = AppTempoMethod.model_validate(_range_data())
    project = controller.project
    project.automatic_bpm_range = BpmRangeOptions(enabled=False, minimum_bpm=200, maximum_bpm=250)
    project.sample_paths[0] = "samples/source.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    project.manual_bpm[0] = 94.0
    project.pad_timing_intent[0] = "tap"
    project.sample_analysis[0] = SampleAnalysis(
        bpm=60.0, key="Am", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]), bpm_method=method
    )
    snapshot = capture_copy_snapshot(project, 0)
    copied = CopySnapshotData.model_validate_json(snapshot.payload_json)
    assert copied.analysis.bpm_method == method
    assert copied.analysis.key == "Am"
    restored = ProjectState.model_validate_json(project.model_dump_json())
    assert restored.sample_analysis[0].bpm_method == method
    assert restored.manual_bpm[0] == 94.0
    assert restored.pad_timing_intent[0] == "tap"
    assert not controller.loader._needs_tempo_refinement(0)


def test_range_defaults_and_config_roundtrip_are_independent_of_existing_pads(
    tmp_path: Path,
) -> None:
    project = ProjectState()
    assert project.automatic_bpm_range == BpmRangeOptions(
        enabled=False, minimum_bpm=50, maximum_bpm=170
    )
    settings = SettingsController(project, SessionState(), Mock())
    settings.set_automatic_bpm_range(BpmRangeOptions(enabled=True, minimum_bpm=30, maximum_bpm=300))
    persistence = ProjectPersistence(project)
    persistence.config_path = tmp_path / "range-project.json"
    persistence.mark_dirty()
    persistence.flush()
    assert (
        ProjectPersistence.from_config_path(persistence.config_path).project.automatic_bpm_range
        == project.automatic_bpm_range
    )


@pytest.mark.parametrize(
    "value",
    [
        {"enabled": 1},
        {"minimum_bpm": True},
        {"maximum_bpm": "170"},
        {"minimum_bpm": float("nan")},
        {"maximum_bpm": float("inf")},
        {"minimum_bpm": 29.9},
        {"maximum_bpm": 300.1},
        {"minimum_bpm": 170, "maximum_bpm": 170},
    ],
)
def test_range_options_are_strict_even_while_disabled(value: dict) -> None:
    with pytest.raises(ValidationError):
        BpmRangeOptions.model_validate(value)


def test_range_settings_configure_before_persist_and_preserve_all_pad_intent() -> None:
    project, session, audio, changed = ProjectState(), SessionState(), Mock(), Mock()
    project.manual_bpm[0], project.pad_timing_intent[0] = 94.0, "tap"
    before = project.model_dump(exclude={"automatic_bpm_range"})
    settings = SettingsController(project, session, audio, changed)
    options = BpmRangeOptions(enabled=True, minimum_bpm=60, maximum_bpm=180)
    observed = []
    audio.configure_automatic_bpm_range.side_effect = lambda *_args: observed.append(
        project.automatic_bpm_range
    )
    settings.set_automatic_bpm_range(options)
    assert observed == [BpmRangeOptions()]
    audio.configure_automatic_bpm_range.assert_called_once()
    assert audio.configure_automatic_bpm_range.call_args.args == (True, 60.0, 180.0)
    assert project.automatic_bpm_range == options
    assert project.model_dump(exclude={"automatic_bpm_range"}) == before
    settings.set_automatic_bpm_range(options)
    assert changed.call_count == 1
    assert audio.configure_automatic_bpm_range.call_count == 1


def test_range_settings_failure_or_forged_instance_never_persists() -> None:
    project, audio, changed = ProjectState(), Mock(), Mock()
    settings = SettingsController(project, SessionState(), audio, changed)
    audio.configure_automatic_bpm_range.side_effect = RuntimeError("Range unavailable")
    with pytest.raises(RuntimeError, match="Range unavailable"):
        settings.set_automatic_bpm_range(BpmRangeOptions(enabled=True))
    assert project.automatic_bpm_range == BpmRangeOptions()
    changed.assert_not_called()
    audio.reset_mock()
    with pytest.raises(ValidationError):
        settings.set_automatic_bpm_range(BpmRangeOptions().model_copy(update={"minimum_bpm": True}))
    audio.configure_automatic_bpm_range.assert_not_called()


def test_loaded_range_is_bound_before_primary_worker_and_survives_rebinding(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    project = controller.project
    project.automatic_bpm_range = BpmRangeOptions(enabled=True, minimum_bpm=65, maximum_bpm=180)
    audio_engine_mock.reset_mock()
    observed = []
    audio_engine_mock.configure_primary_beat_analysis.side_effect = lambda *_args: observed.append(
        audio_engine_mock.configure_automatic_bpm_range.call_args.args
    )
    controller.loader.bind_asset_lifecycle(
        controller._assets, beat_this_installation_root=Path("worker")
    )
    assert observed == [(True, 65.0, 180.0)]
    assert project.automatic_bpm_range == BpmRangeOptions(
        enabled=True, minimum_bpm=65, maximum_bpm=180
    )
    assert (
        ProjectState.model_validate_json(project.model_dump_json()).automatic_bpm_range
        == project.automatic_bpm_range
    )
