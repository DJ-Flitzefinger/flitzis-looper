"""Historical map recognition and defensive durable metadata ownership."""

import json

import pytest
from pydantic import ValidationError

from flitzis_looper.source_beatmap import PersistedSourceBeatmap


def _write_attribute(target: object, field: str, value: object) -> None:
    """Exercise the object's assignment guard with an intentionally invalid write."""
    setattr(target, field, value)


def envelope(version: int = 1) -> dict[str, object]:
    return {
        "schema_version": version,
        "encoding": f"source-beatmap-pwa-v{version}",
        "record": {"source": {"decoder_identity": "unverified"}, "anchors": ["exact-bits"]},
    }


@pytest.mark.parametrize("version", [1, 2, 3, 4])
def test_supported_record_roundtrips_as_an_object_without_native_acceptance(version: int) -> None:
    data = envelope(version)
    saved = PersistedSourceBeatmap.model_validate(data)
    assert saved.model_dump() == data
    restored = PersistedSourceBeatmap.from_json(saved.model_dump_json())
    assert restored.model_dump() == data
    # Python deliberately recognizes metadata that Rust must still reject.
    assert restored.record_data()["source"] == {"decoder_identity": "unverified"}


@pytest.mark.parametrize("version", [1, 2, 3, 4])
def test_caller_and_returned_nested_metadata_cannot_mutate_saved_record(version: int) -> None:
    data = envelope(version)
    saved = PersistedSourceBeatmap.model_validate(data)
    source = data["record"]
    assert isinstance(source, dict)
    source["anchors"].append("caller mutation")
    copy = saved.record_data()
    anchors = copy["anchors"]
    assert isinstance(anchors, list)
    anchors.append("returned mutation")
    dumped = saved.model_dump()
    dumped["record"]["anchors"].append("dump mutation")
    assert saved.record_data()["anchors"] == ["exact-bits"]
    with pytest.raises(ValidationError, match="frozen"):
        _write_attribute(saved, "record", "replacement")


@pytest.mark.parametrize("version", [True, False, 1.0, 2.0, 3.0, 4.0, "1", "2", "3", "4", 0, 5])
def test_unsupported_or_noninteger_schema_is_not_recognized(version: object) -> None:
    data = envelope()
    data["schema_version"] = version
    with pytest.raises(ValidationError):
        PersistedSourceBeatmap.model_validate(data)


@pytest.mark.parametrize(
    ("version", "encoding"),
    [
        (1, "source-beatmap-pwa-v2"),
        (2, "source-beatmap-pwa-v1"),
        (3, "source-beatmap-pwa-v2"),
        (2, "source-beatmap-pwa-v3"),
        (4, "source-beatmap-pwa-v3"),
        (3, "source-beatmap-pwa-v4"),
    ],
)
def test_version_and_encoding_must_be_the_same_supported_pair(version: int, encoding: str) -> None:
    data = envelope(version)
    data["encoding"] = encoding
    with pytest.raises(ValidationError, match="does not match schema_version"):
        PersistedSourceBeatmap.model_validate(data)


@pytest.mark.parametrize("record", [{}, [], "encoded object", None, {"number": float("nan")}])
def test_record_must_be_a_nonempty_json_object(record: object) -> None:
    data = envelope()
    data["record"] = record
    with pytest.raises(ValidationError):
        PersistedSourceBeatmap.model_validate(data)


def test_unknown_encoding_and_extra_fields_are_not_recognized() -> None:
    data = envelope()
    data["encoding"] = "future-map-v2"
    with pytest.raises(ValidationError):
        PersistedSourceBeatmap.model_validate(data)
    data = envelope()
    data["current_audio"] = True
    with pytest.raises(ValidationError):
        PersistedSourceBeatmap.model_validate(data)


def test_external_factory_checks_bytes_before_decoding(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("flitzis_looper.source_beatmap.MAX_SOURCE_BEATMAP_BYTES", 8)
    with pytest.raises(ValueError, match="byte limit"):
        PersistedSourceBeatmap.from_json(json.dumps(envelope()))
