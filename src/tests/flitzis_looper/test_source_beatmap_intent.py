"""Historical compact selection never claims live qualification or mutable map ownership."""

import json

import pytest
from pydantic import ValidationError

from flitzis_looper.models import ProjectState, SampleAnalysis
from flitzis_looper.source_beatmap_intent import (
    MAX_SOURCE_BEATMAP_REVISION,
    PadSourceBeatmapIntent,
    SourceBeatmapReference,
    next_source_beatmap_intent,
)


def _reference(root: str = "samples/Shared Take") -> SourceBeatmapReference:
    return SourceBeatmapReference(
        schema_version=1,
        kind="source-beatmap-v1",
        reference=f"{root}/.pcm-cache/source-beatmaps/v1/.ready-{'a' * 32}/map.json",
        bytes=64 * 1024 * 1024,
        sha256="b" * 64,
        map_revision="c" * 64,
        decoder_identity="d" * 64,
    )


def test_automatic_selection_is_additive_and_manual_clear_stays_explicit() -> None:
    manual = next_source_beatmap_intent(None, _reference())
    assert set(manual.model_dump()) == {"schema_version", "revision", "selected"}
    automatic = next_source_beatmap_intent(None, _reference(), automatic=True)
    assert automatic.model_dump()["automatic"] is True
    assert PadSourceBeatmapIntent.model_validate_json(automatic.model_dump_json()) == automatic
    cleared = next_source_beatmap_intent(automatic, None)
    assert cleared.automatic is False
    assert cleared.revision == 2
    with pytest.raises(ValidationError):
        PadSourceBeatmapIntent(schema_version=1, revision=1, selected=None, automatic=True)


@pytest.mark.parametrize(
    "root", ["samples", "samples/Shared Take", f"samples/materials/M{'e' * 32}"]
)
def test_exact_native_receipt_roundtrips_all_three_material_namespaces(root: str) -> None:
    reference = _reference(root)
    assert SourceBeatmapReference.model_validate_json(reference.model_dump_json()) == reference
    assert set(reference.model_dump()) == {
        "schema_version",
        "kind",
        "reference",
        "bytes",
        "sha256",
        "map_revision",
        "decoder_identity",
    }
    with pytest.raises(ValidationError, match="frozen"):
        reference.bytes = 1


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("schema_version", True),
        ("schema_version", 1.0),
        ("schema_version", 2),
        ("kind", "original"),
        ("bytes", True),
        ("bytes", 0),
        ("bytes", 64 * 1024 * 1024 + 1),
        ("sha256", "B" * 64),
        ("map_revision", "short"),
        ("decoder_identity", "d" * 63),
        ("verified", True),
        ("file_identity", [1, 2, 3]),
    ],
)
def test_receipt_refuses_forged_or_unsupported_metadata(field: str, value: object) -> None:
    with pytest.raises(ValidationError):
        SourceBeatmapReference.model_validate(_reference().model_dump() | {field: value})


@pytest.mark.parametrize("field", tuple(SourceBeatmapReference.model_fields))
def test_receipt_requires_every_native_field(field: str) -> None:
    data = _reference().model_dump()
    data.pop(field)
    with pytest.raises(ValidationError):
        SourceBeatmapReference.model_validate(data)


@pytest.mark.parametrize(
    "root",
    [
        "other",
        "samples/../foreign",
        "samples/.hidden",
        "samples/#1",
        "samples/CON",
        "samples/lpt¹",
        "samples/trailing.",
        "samples/trailing ",
        "samples/materials",
        "samples/Materials",
        "samples/stems",
        "samples/config.json",
        "samples/A\\B",
        "samples/A\x7fB",
        f"samples/materials/M{'a' * 31}",
    ],
)
def test_receipt_refuses_noncanonical_or_unsafe_material_roots(root: str) -> None:
    with pytest.raises(ValidationError):
        _reference(root)


def test_reference_byte_bound_counts_utf8_without_lowering_native_map_size() -> None:
    reference = _reference("samples/" + "é" * 1800)
    encoded = reference.model_dump_json()
    assert len(reference.reference.encode("utf-8")) < 4096
    assert len(encoded.encode("utf-8")) < 8192
    assert len(json.dumps(reference.model_dump()).encode("utf-8")) > 8192
    assert SourceBeatmapReference.model_validate_json(encoded).bytes == 64 * 1024 * 1024
    with pytest.raises(ValidationError, match="normalized typed artifact"):
        _reference("samples/" + "é" * 2100)


def test_every_request_advances_revision_including_clear_and_aba() -> None:
    a = _reference()
    b = SourceBeatmapReference.model_validate(a.model_dump() | {"map_revision": "e" * 64})
    first = next_source_beatmap_intent(None, a)
    repeated = next_source_beatmap_intent(first, a)
    second = next_source_beatmap_intent(repeated, b)
    again = next_source_beatmap_intent(second, a)
    cleared = next_source_beatmap_intent(again, None)
    cleared_again = next_source_beatmap_intent(cleared, None)
    assert [item.revision for item in (first, repeated, second, again, cleared, cleared_again)] == [
        1,
        2,
        3,
        4,
        5,
        6,
    ]
    assert again.selected == first.selected
    assert cleared.selected is None
    assert cleared_again.selected is None
    assert first.selected is not a
    assert first.revision == 1
    assert next_source_beatmap_intent(None, None).revision == 1


def test_checked_revision_exhaustion_and_unchecked_instances_never_wrap() -> None:
    last = PadSourceBeatmapIntent(
        schema_version=1, revision=MAX_SOURCE_BEATMAP_REVISION, selected=None
    )
    with pytest.raises(ValueError, match="capacity exhausted"):
        next_source_beatmap_intent(last, _reference())
    forged = last.model_copy(update={"revision": True})
    with pytest.raises(ValidationError):
        next_source_beatmap_intent(forged, None)
    with pytest.raises(ValidationError):
        next_source_beatmap_intent(None, _reference().model_copy(update={"bytes": 0}))


@pytest.mark.parametrize(
    "field",
    tuple(
        name for name, field in PadSourceBeatmapIntent.model_fields.items() if field.is_required()
    ),
)
def test_new_intent_is_complete_and_never_fills_partial_metadata(field: str) -> None:
    data = next_source_beatmap_intent(None, _reference()).model_dump()
    data.pop(field)
    with pytest.raises(ValidationError):
        PadSourceBeatmapIntent.model_validate(data)


def test_project_has_one_independent_216_slot_table_outside_scalar_analysis() -> None:
    first = ProjectState()
    second = ProjectState()
    assert first.pad_source_beatmap_intent == [None] * 216
    first.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, _reference())
    assert first.pad_source_beatmap_intent[1] is None
    assert second.pad_source_beatmap_intent[0] is None
    assert "source_beatmap_intent" not in SampleAnalysis.model_fields
    reopened = ProjectState.model_validate_json(first.model_dump_json())
    assert reopened.pad_source_beatmap_intent[0] == first.pad_source_beatmap_intent[0]
    assert reopened.pad_source_beatmap_intent[0] is not first.pad_source_beatmap_intent[0]
    with pytest.raises(ValidationError):
        ProjectState(pad_source_beatmap_intent=[None] * 215)
