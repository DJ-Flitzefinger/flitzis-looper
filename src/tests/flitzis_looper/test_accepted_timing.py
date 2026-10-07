"""Persisted timing remains historical evidence, with explicit performer intent."""

import json

import pytest
from pydantic import ValidationError

from flitzis_looper.accepted_timing import PersistedAcceptedTiming
from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.models import BeatGrid, ProjectState, SampleAnalysis


def _historical_envelope() -> dict[str, object]:
    return {
        "schema_version": 1,
        "encoding": "accepted-constant-timing-qm-raw-v1",
        "record": {
            "accepted_revision": "accepted-constant-timing-v1:" + "a" * 64,
            "period_bits": "3fe0000000000001",
            "origin": {"seconds_bits": "8000000000000000", "provenance": "independent anchor"},
            "decision": {"policy_version": "fixture-policy-v1", "provenance": "explicit decision"},
            "binding": {
                "job": {"pad_id": 0, "request_id": 2**64 - 1, "source_generation": 14},
                "source_sha256": "b" * 64,
                "pcm_sha256": "c" * 64,
                "frame_count": 28800000,
                "sample_rate_hz": 48000,
                "origin_seconds_bits": "0000000000000000",
            },
            "raw": {"beat_frames_bits": ["0000000000000000", "3ff0000000000001"]},
        },
    }


def _analysis_with_timing(timing: object) -> SampleAnalysis:
    return SampleAnalysis.model_validate({
        "bpm": 120.00128936767578,
        "key": "C",
        "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
        "accepted_timing": timing,
    })


def test_envelope_preserves_native_bits_and_full_historical_evidence() -> None:
    envelope = _historical_envelope()
    persisted = PersistedAcceptedTiming.model_validate(envelope)
    roundtrip = PersistedAcceptedTiming.model_validate_json(persisted.model_dump_json())

    assert roundtrip.model_dump(mode="json") == envelope
    assert json.loads(roundtrip.model_dump_json()) == envelope
    assert "ticket" not in roundtrip.model_dump()


def test_sample_analysis_project_roundtrip_keeps_evidence_independent_of_display_bpm() -> None:
    project = ProjectState()
    project.sample_paths[0] = "samples/fixture.wav"
    project.sample_analysis[0] = _analysis_with_timing(_historical_envelope())
    project.pad_timing_intent[0] = "automatic"

    restored = ProjectState.model_validate_json(project.model_dump_json())
    analysis = restored.sample_analysis[0]

    assert analysis is not None
    assert analysis.bpm == 120.00128936767578
    original_analysis = project.sample_analysis[0]
    assert original_analysis is not None
    assert analysis.accepted_timing == original_analysis.accepted_timing
    assert restored.pad_timing_intent[0] == "automatic"


@pytest.mark.parametrize(
    "override",
    [
        {"schema_version": 2},
        {"schema_version": True},
        {"schema_version": 1.0},
        {"encoding": "raw-revision-v1"},
        {"record": []},
        {"record": {}},
        {"runtime_ticket": "opaque"},
    ],
)
def test_unsupported_envelope_discards_only_accepted_extension(override: dict[str, object]) -> None:
    envelope = _historical_envelope() | override
    analysis = _analysis_with_timing(envelope)

    assert analysis.accepted_timing is None
    assert analysis.bpm == 120.00128936767578
    assert analysis.key == "C"
    assert analysis.beat_grid == BeatGrid(beats=[0.0, 0.5], downbeats=[0.0], bars=[0.0])


def test_unsupported_saved_accepted_envelope_preserves_tap_and_manual_intent() -> None:
    project_data = ProjectState().model_dump(mode="json")
    project_data["sample_analysis"][0] = {
        "bpm": 120.0,
        "key": "C",
        "beat_grid": {"beats": [], "downbeats": [], "bars": []},
        "accepted_timing": _historical_envelope() | {"schema_version": 99},
    }
    project_data["pad_timing_intent"][:3] = ["automatic", "tap", "manual"]
    project_data["manual_bpm"][1:3] = [94.0, 90.0]

    restored = ProjectState.model_validate_json(json.dumps(project_data))

    assert restored.pad_timing_intent[:3] == ["automatic", "tap", "manual"]
    assert restored.manual_bpm[1:3] == [94.0, 90.0]
    assert restored.sample_analysis[0] is not None
    assert restored.sample_analysis[0].accepted_timing is None


def test_legacy_analysis_and_raw_revision_do_not_manufacture_acceptance() -> None:
    analysis = _analysis_with_timing({"raw_revision": "source-bound-raw-v1:" + "a" * 64})

    assert analysis.accepted_timing is None


def test_legacy_project_preserves_manual_override_without_inventing_tap() -> None:
    manual_bpm: list[float | None] = [None] * NUM_SAMPLES
    manual_bpm[4] = 94.0
    project = ProjectState.model_validate({"manual_bpm": manual_bpm})

    assert project.pad_timing_intent[4] == "manual"
    assert project.pad_timing_intent[0] == "legacy"
    assert "automatic" not in project.pad_timing_intent
    assert "tap" not in project.pad_timing_intent


def test_explicit_intents_roundtrip_without_promoting_legacy_metadata() -> None:
    project = ProjectState()
    project.pad_timing_intent[:4] = ["automatic", "manual", "tap", "legacy"]
    project.manual_bpm[1:3] = [127.125, 94.0]

    restored = ProjectState.model_validate_json(project.model_dump_json())

    assert restored.pad_timing_intent == project.pad_timing_intent
    assert restored.manual_bpm == project.manual_bpm


@pytest.mark.parametrize("value", [[], ["legacy"], ["automatic"] * (NUM_SAMPLES + 1)])
def test_timing_intent_requires_complete_pad_extent(value: list[str]) -> None:
    with pytest.raises(ValidationError, match="pad_timing_intent must have length"):
        ProjectState.model_validate({"pad_timing_intent": value})


def test_unknown_timing_intent_is_rejected() -> None:
    with pytest.raises(ValidationError, match="Input should be"):
        ProjectState.model_validate({"pad_timing_intent": ["accepted"] * NUM_SAMPLES})
