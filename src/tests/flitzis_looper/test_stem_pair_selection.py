"""Pure durable pair selection contracts; no file integrity or native ACK claims."""

import json
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.stem_pair_selection import StemPairSelection

if TYPE_CHECKING:
    from pathlib import Path


def _payload(material: str = "a" * 32) -> dict[str, object]:
    root = f"samples/materials/M{material}"
    return {
        "schema_version": 1,
        "descriptor_reference": f"{root}/.pcm-cache/stems/v1/.pairs/{'b' * 32}.json",
        "stem_set_identity": "d" * 64,
        "wav_generation": f"{root}/stems/.ready-{'e' * 32}",
        "pcm_generation": f"{root}/.pcm-cache/stems/v1/.ready-{'f' * 32}",
    }


def test_actual_json_reopen_preserves_frozen_selection_without_artifact_io(tmp_path: Path) -> None:
    original = StemPairSelection.model_validate(_payload())
    path = tmp_path / "selection.json"
    path.write_text(original.model_dump_json(), encoding="utf-8")

    reopened = StemPairSelection.model_validate_json(path.read_text(encoding="utf-8"))

    assert reopened == original
    assert reopened is not original
    assert reopened.model_dump() == _payload()
    assert reopened.model_dump_json() == original.model_dump_json()
    assert not (tmp_path / "samples").exists()
    assert set(json.loads(path.read_text(encoding="utf-8"))) == {
        "schema_version",
        "descriptor_reference",
        "stem_set_identity",
        "wav_generation",
        "pcm_generation",
    }


@pytest.mark.parametrize("material", ["0" * 32, "f" * 32, "0123456789abcdef" * 2])
def test_selection_has_no_origin_pad_or_private_generation_dependency(material: str) -> None:
    original = StemPairSelection.model_validate(_payload(material))
    values = original.model_dump()
    assert f"M{material}" in original.descriptor_reference
    values["descriptor_reference"] = str(values["descriptor_reference"]).replace("b" * 32, "1" * 32)
    values["wav_generation"] = str(values["wav_generation"]).replace("e" * 32, "2" * 32)
    values["pcm_generation"] = str(values["pcm_generation"]).replace(
        ".ready-" + "f" * 32, ".ready-" + "3" * 32
    )
    retry = StemPairSelection.model_validate(values)
    assert retry.stem_set_identity == original.stem_set_identity
    assert retry.descriptor_reference != original.descriptor_reference
    assert retry.wav_generation != original.wav_generation
    assert retry.pcm_generation != original.pcm_generation


@pytest.mark.parametrize(
    "field", ["descriptor_reference", "stem_set_identity", "wav_generation", "pcm_generation"]
)
def test_copies_and_input_payloads_do_not_alias_mutable_selection_state(field: str) -> None:
    payload = _payload()
    original = StemPairSelection.model_validate(payload)
    shallow = original.model_copy()
    deep = original.model_copy(deep=True)
    payload["wav_generation"] = "samples/stems/#216"

    assert shallow == deep == original
    assert shallow is not original
    assert deep is not original
    assert original.wav_generation != payload["wav_generation"]
    for value in (original, shallow, deep):
        with pytest.raises(ValidationError, match="frozen"):
            setattr(value, field, "samples/stems/#1")
        with pytest.raises(ValidationError, match="frozen"):
            delattr(value, field)


def test_unvalidated_model_copy_updates_are_rechecked_at_the_next_schema_boundary() -> None:
    original = StemPairSelection.model_validate(_payload())
    invalid = original.model_copy(update={"pcm_generation": "samples/../outside"})
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate(invalid)
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate_json(invalid.model_dump_json())
    assert original == StemPairSelection.model_validate(_payload())


@pytest.mark.parametrize("revision", [True, False, 1.0, "1", None, 0, 2, -1])
def test_schema_revision_rejects_bool_coercion_and_unsupported_versions(revision: object) -> None:
    values = _payload() | {"schema_version": revision}
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate(values)
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate_json(json.dumps(values))


def test_absent_schema_revision_uses_the_current_durable_version() -> None:
    values = _payload()
    del values["schema_version"]
    selected = StemPairSelection.model_validate(values)
    assert selected.schema_version == 1
    assert selected.model_dump()["schema_version"] == 1


@pytest.mark.parametrize(
    "field",
    ["native_ack", "request_id", "sample_id", "publication", "permit", "source_epoch", "unknown"],
)
def test_unknown_and_runtime_authority_fields_cannot_be_persisted(field: str) -> None:
    values = _payload() | {field: 1}
    with pytest.raises(ValidationError, match="Extra inputs"):
        StemPairSelection.model_validate(values)
    with pytest.raises(ValidationError, match="Extra inputs"):
        StemPairSelection.model_validate_json(json.dumps(values))


@pytest.mark.parametrize("field", ["descriptor_reference", "wav_generation", "pcm_generation"])
@pytest.mark.parametrize(
    "fault",
    ["parent", "dot", "absolute", "drive", "unc", "backslash", "ads", "slash", "newline", "double"],
)
def test_references_reject_noncanonical_path_spellings(field: str, fault: str) -> None:
    values = _payload()
    reference = str(values[field])
    changes = {
        "parent": reference.replace("/materials/", "/materials/../materials/"),
        "dot": reference.replace("/materials/", "/materials/./"),
        "absolute": f"/{reference}",
        "drive": f"D:/{reference}",
        "unc": f"//server/share/{reference}",
        "backslash": reference.replace("/", "\\"),
        "ads": f"{reference}:stream",
        "slash": f"{reference}/",
        "newline": f"{reference}\n",
        "double": reference.replace("/materials/", "/materials//"),
    }
    values[field] = changes[fault]
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate(values)


@pytest.mark.parametrize("field", ["descriptor_reference", "wav_generation", "pcm_generation"])
@pytest.mark.parametrize(
    "material", ["a" * 31, "a" * 33, "A" * 32, "g" * 32, "é" * 32, "M" + "a" * 32]
)
def test_material_ids_are_exact_ascii_lowercase_hex(field: str, material: str) -> None:
    values = _payload()
    values[field] = str(values[field]).replace("M" + "a" * 32, f"M{material}")
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate(values)


@pytest.mark.parametrize("field", ["descriptor_reference", "wav_generation", "pcm_generation"])
@pytest.mark.parametrize("generation", ["1" * 31, "1" * 33, "A" * 32, "g" * 32, "音" * 32])
def test_generation_ids_are_exact_ascii_lowercase_hex(field: str, generation: str) -> None:
    values = _payload()
    old_id = {
        "descriptor_reference": "b" * 32,
        "wav_generation": "e" * 32,
        "pcm_generation": "f" * 32,
    }[field]
    values[field] = str(values[field]).replace(old_id, generation)
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate(values)


@pytest.mark.parametrize(
    ("field", "replacement"),
    [
        ("descriptor_reference", "samples/stems/#1/.complete.json"),
        ("descriptor_reference", "samples/materials/M" + "a" * 32 + "/original/Take.wav"),
        ("descriptor_reference", "samples/materials/M" + "a" * 32 + "/stems/.ready-" + "b" * 32),
        ("wav_generation", "samples/stems/#216"),
        ("wav_generation", "samples/materials/M" + "a" * 32 + "/stems/.generation-" + "b" * 32),
        ("pcm_generation", "samples/.pcm-cache/v1/.ready-" + "b" * 32),
        (
            "pcm_generation",
            "samples/materials/M" + "a" * 32 + "/.pcm-cache/v1/.ready-" + "b" * 32,
        ),
    ],
)
def test_legacy_wrong_kind_and_private_staging_references_are_not_pair_selection(
    field: str, replacement: str
) -> None:
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate(_payload() | {field: replacement})


def test_wav_and_pcm_areas_cannot_be_swapped() -> None:
    values = _payload()
    values["wav_generation"], values["pcm_generation"] = (
        values["pcm_generation"],
        values["wav_generation"],
    )
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate(values)


@pytest.mark.parametrize("field", ["descriptor_reference", "wav_generation", "pcm_generation"])
def test_three_valid_references_must_name_one_material(field: str) -> None:
    values = _payload()
    values[field] = str(values[field]).replace("M" + "a" * 32, "M" + "0" * 32)
    with pytest.raises(ValidationError, match="same material"):
        StemPairSelection.model_validate(values)


@pytest.mark.parametrize(
    "identity", ["a" * 63, "a" * 65, "A" * 64, "g" * 64, "é" * 32, "a" * 64 + "\n"]
)
def test_complete_set_identity_is_exact_lowercase_sha256(identity: str) -> None:
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate(_payload() | {"stem_set_identity": identity})


@pytest.mark.parametrize(
    "field", ["descriptor_reference", "stem_set_identity", "wav_generation", "pcm_generation"]
)
@pytest.mark.parametrize("value", [True, False, 1, 1.0, None, b"bytes", [], {}])
def test_reference_and_digest_fields_do_not_coerce_non_strings(field: str, value: object) -> None:
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate(_payload() | {field: value})


@pytest.mark.parametrize(
    "field", ["descriptor_reference", "stem_set_identity", "wav_generation", "pcm_generation"]
)
def test_missing_selection_fields_cannot_invent_a_complete_pair(field: str) -> None:
    values = _payload()
    del values[field]
    with pytest.raises(ValidationError):
        StemPairSelection.model_validate(values)
