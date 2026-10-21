"""Compact map intent travels with content while exact legacy seals remain readable."""

import hashlib
import json
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.bank_snapshot import (
    BankSnapshot,
    capture_bank_snapshot,
    project_with_bank_snapshot,
)
from flitzis_looper.controller.copy_arrangement_journal import CopyArrangementJournal
from flitzis_looper.controller.pair_arrangement_journal import (
    PairArrangementAfterImage,
    validated_project_image,
)
from flitzis_looper.copy_snapshot import (
    CopySnapshot,
    canonical_snapshot_json,
    capture_copy_snapshot,
    musical_snapshot_projection,
    project_with_copy_snapshot,
)
from flitzis_looper.models import ProjectState
from flitzis_looper.pair_snapshot import (
    capture_pair_snapshot,
    project_with_pair_snapshot,
)
from flitzis_looper.source_beatmap_intent import next_source_beatmap_intent
from tests.flitzis_looper.controller.test_copy_arrangement_journal import _captured
from tests.flitzis_looper.test_copy_snapshot import _full_project, _target_identity
from tests.flitzis_looper.test_source_beatmap_intent import _reference

if TYPE_CHECKING:
    from pathlib import Path

_LEGACY_FIELDS = {
    "source_path",
    "source_identity",
    "duration",
    "analysis",
    "stem_cache",
    "stem_mix_mode",
    "stem_intent",
    "key_lock",
    "manual_bpm",
    "timing_intent",
    "key_intent",
    "gain_db",
    "eq_low_db",
    "eq_mid_db",
    "eq_high_db",
    "loop_start_s",
    "loop_end_s",
    "loop_auto",
    "loop_bars",
    "grid_offset_samples",
    "grid_anchor_s",
}


def test_neutral_copy_pair_bank_and_pair_after_seals_keep_exact_legacy_21_key_payloads() -> None:
    project = _full_project()
    copied = capture_copy_snapshot(project, 0)
    paired = capture_pair_snapshot(project, 0, 215, operation="move")
    after = PairArrangementAfterImage.from_before(paired)
    bank = capture_bank_snapshot(project, 0, 1)
    copy_data = json.loads(copied.payload_json)
    pair_data = json.loads(paired.payload_json)
    after_data = json.loads(after.payload_json)
    bank_data = json.loads(bank.payload_json)
    images = [
        copy_data,
        pair_data["source"],
        pair_data["target"],
        after_data["source"],
        after_data["target"],
    ]
    images.extend(
        image
        for row in bank_data["slots"]
        for image in (row["source"], row["before"], row["after"])
    )
    assert all(set(image) == _LEGACY_FIELDS for image in images)
    for seal in (copied, paired, after, bank):
        original = seal.payload_json
        assert original == canonical_snapshot_json(json.loads(original))
        assert type(seal).model_validate_json(seal.model_dump_json()).payload_json == original
        assert (
            hashlib.sha256(original.encode()).digest()
            == hashlib.sha256(type(seal).model_validate(seal).payload_json.encode()).digest()
        )
    assert CopySnapshot.model_validate(copied).projected().source_beatmap_intent is None
    assert musical_snapshot_projection(copied.projected())["source_beatmap_intent"] is None


def test_copy_freezes_selection_and_shares_reference_with_independent_content_intent() -> None:
    project = _full_project()
    original = next_source_beatmap_intent(None, _reference())
    project.pad_source_beatmap_intent[0] = original
    snapshot = capture_copy_snapshot(project, 0)
    assert set(json.loads(snapshot.payload_json)) == _LEGACY_FIELDS | {"source_beatmap_intent"}
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(original, None)

    candidate = project_with_copy_snapshot(
        project, 215, snapshot, target_identity=_target_identity(snapshot)
    )

    target = candidate.pad_source_beatmap_intent[215]
    assert target == original
    assert target is not original
    assert target is not None
    assert target.selected == original.selected
    assert target.selected is not original.selected
    assert candidate.pad_source_beatmap_intent[0] != target
    assert candidate.sample_analysis[215] == project.sample_analysis[0]
    candidate.pad_source_beatmap_intent[215] = next_source_beatmap_intent(target, None)
    assert snapshot.projected().source_beatmap_intent == original


def test_move_resets_empty_source_and_swap_carries_cleared_map_revision() -> None:
    project = _full_project()
    selected = next_source_beatmap_intent(None, _reference())
    project.pad_source_beatmap_intent[0] = selected
    moved = project_with_pair_snapshot(
        project, capture_pair_snapshot(project, 0, 215, operation="move")
    )
    assert moved.pad_source_beatmap_intent[0] is None
    assert moved.pad_source_beatmap_intent[215] == selected
    assert moved.pad_content[215] == project.pad_content[0]
    copied = project_with_copy_snapshot(
        moved,
        0,
        capture_copy_snapshot(moved, 215),
        target_identity=_target_identity(capture_copy_snapshot(moved, 215)),
    )
    cleared = next_source_beatmap_intent(selected, None)
    copied.pad_source_beatmap_intent[0] = cleared
    swapped = project_with_pair_snapshot(
        copied, capture_pair_snapshot(copied, 0, 215, operation="swap")
    )
    assert swapped.pad_source_beatmap_intent[0] == selected
    assert swapped.pad_source_beatmap_intent[215] == cleared
    assert swapped.pad_source_beatmap_intent[215].revision == 2


def test_bank_copy_and_clear_preserve_non_neutral_and_cleared_revisions() -> None:
    project = _full_project()
    selected = next_source_beatmap_intent(None, _reference())
    project.pad_source_beatmap_intent[0] = selected
    snapshot = capture_bank_snapshot(project, 0, 1)
    copied = project_with_bank_snapshot(project, snapshot)
    assert copied.pad_source_beatmap_intent[36] == selected
    assert copied.pad_source_beatmap_intent[36] is not selected
    assert copied.pad_source_beatmap_intent[37:72] == [None] * 35
    assert BankSnapshot.model_validate_json(snapshot.model_dump_json()) == snapshot
    copied.pad_source_beatmap_intent[0] = next_source_beatmap_intent(selected, None)
    recaptured = capture_bank_snapshot(copied, 0, 1)
    recopy = project_with_bank_snapshot(copied, recaptured)
    assert recopy.pad_source_beatmap_intent[36] == copied.pad_source_beatmap_intent[0]
    clear = project_with_bank_snapshot(recopy, capture_bank_snapshot(recopy, None, 1))
    assert clear.pad_source_beatmap_intent[36:72] == [None] * 36
    assert clear.pad_source_beatmap_intent[0] == copied.pad_source_beatmap_intent[0]


@pytest.mark.parametrize("operation", ["move", "bank"])
def test_stale_map_edit_refuses_whole_prepared_operation(operation: str) -> None:
    project = _full_project()
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, _reference())
    pair = capture_pair_snapshot(project, 0, 215, operation="move")
    bank = capture_bank_snapshot(project, 0, 1)
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(
        project.pad_source_beatmap_intent[0], None
    )
    before = project.model_dump_json()
    if operation == "move":
        with pytest.raises(ValueError, match="musical content changed"):
            project_with_pair_snapshot(project, pair)
    else:
        with pytest.raises(ValueError, match="musical content changed"):
            project_with_bank_snapshot(project, bank)
    assert project.model_dump_json() == before


def test_complete_legacy_project_omits_only_new_table_without_rewriting_raw_bytes() -> None:
    data = _full_project().model_dump(mode="json")
    data.pop("pad_source_beatmap_intent")
    raw = json.dumps(data, indent=3, ensure_ascii=False)
    digest = hashlib.sha256(raw.encode()).hexdigest()
    projected = validated_project_image(raw)
    assert projected.pad_source_beatmap_intent == [None] * 216
    assert hashlib.sha256(raw.encode()).hexdigest() == digest
    for missing in ("pad_gain_db", "sample_analysis"):
        damaged = data.copy()
        damaged.pop(missing)
        with pytest.raises(ValueError, match="incomplete"):
            validated_project_image(json.dumps(damaged))
    with pytest.raises(ValueError, match="unknown"):
        validated_project_image(json.dumps(data | {"native_map_verified": True}))


def test_legacy_copy_journal_retains_exact_original_project_strings_and_hash(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    data = json.loads(journal.captured_project_image_json)
    data.pop("pad_source_beatmap_intent")
    legacy_text = json.dumps(data, indent=3)
    digest = hashlib.sha256(legacy_text.encode()).hexdigest()
    legacy = journal.changed(
        captured_project_image_json=legacy_text,
        project_image_json=legacy_text,
        captured_config_sha256=digest,
    )
    reopened = CopyArrangementJournal.model_validate_json(legacy.model_dump_json())
    assert reopened.captured_project_image_json == legacy_text
    assert reopened.project_image_json == legacy_text
    assert reopened.captured_config_sha256 == digest
    assert reopened.source.payload_json == journal.source.payload_json
    candidate = project_with_copy_snapshot(
        persistence.project,
        journal.target_id,
        journal.source,
        target_identity=journal.target_identity,
    )
    after = candidate.model_dump(mode="json")
    after.pop("pad_source_beatmap_intent")
    after_text = json.dumps(after, indent=4)
    after_digest = hashlib.sha256(after_text.encode()).hexdigest()
    committed = legacy.changed(
        phase="config_committed",
        image_role="after",
        project_image_json=after_text,
        committed_revision=legacy.intent_revision,
        committed_config_sha256=after_digest,
    )
    assert committed.project_image_json == after_text
    assert committed.committed_config_sha256 == after_digest
    with pytest.raises(ValidationError, match="same-revision"):
        committed.changed(project_image_json=after_text + "\n")


@pytest.mark.parametrize("family", ["copy", "pair", "bank"])
@pytest.mark.parametrize("damage", ["partial_intent", "partial_receipt", "unknown_receipt"])
def test_every_new_musical_seal_refuses_incomplete_or_unknown_map_metadata(
    family: str, damage: str
) -> None:
    project = _full_project()
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, _reference())
    if family == "copy":
        seal = capture_copy_snapshot(project, 0)
        payload = json.loads(seal.payload_json)
        image = payload
    elif family == "pair":
        seal = capture_pair_snapshot(project, 0, 215, operation="move")
        payload = json.loads(seal.payload_json)
        image = payload["source"]
    else:
        seal = capture_bank_snapshot(project, 0, 1)
        payload = json.loads(seal.payload_json)
        image = payload["slots"][0]["source"]
    intent = image["source_beatmap_intent"]
    if damage == "partial_intent":
        intent.pop("revision")
    elif damage == "partial_receipt":
        intent["selected"].pop("decoder_identity")
    else:
        intent["selected"]["verified"] = True
    with pytest.raises(ValidationError):
        type(seal)(payload_json=canonical_snapshot_json(payload))


def test_explicit_clear_keeps_new_seal_field_and_revision_instead_of_becoming_legacy() -> None:
    project = _full_project()
    selected = next_source_beatmap_intent(None, _reference())
    cleared = next_source_beatmap_intent(selected, None)
    project.pad_source_beatmap_intent[0] = cleared
    sealed = capture_copy_snapshot(project, 0)
    payload = json.loads(sealed.payload_json)
    assert payload["source_beatmap_intent"] == {
        "schema_version": 1,
        "revision": 2,
        "selected": None,
    }
    assert sealed.projected().source_beatmap_intent == cleared


@pytest.mark.parametrize(
    "damage", ["partial_intent", "partial_receipt", "unknown_receipt", "bool_revision"]
)
def test_complete_new_project_refuses_nested_map_damage(damage: str) -> None:
    project = _full_project()
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, _reference())
    data = project.model_dump(mode="json")
    intent = data["pad_source_beatmap_intent"][0]
    if damage == "partial_intent":
        intent.pop("selected")
    elif damage == "partial_receipt":
        intent["selected"].pop("sha256")
    elif damage == "unknown_receipt":
        intent["selected"]["file_identity"] = [1, 2, 3]
    else:
        intent["revision"] = True
    with pytest.raises((ValueError, ValidationError)):
        validated_project_image(json.dumps(data))


def test_216_receipts_remain_compact_without_embedding_full_envelopes() -> None:
    intent = next_source_beatmap_intent(None, _reference())
    project = ProjectState(pad_source_beatmap_intent=[intent] * 216)
    data = project.model_dump(mode="json")
    encoded = project.model_dump_json()
    assert len(encoded.encode()) < 512 * 1024
    assert "source-beatmap-pwa-v1" not in encoded
    assert all(
        row["selected"]["bytes"] == 64 * 1024 * 1024 for row in data["pad_source_beatmap_intent"]
    )
    assert project.pad_source_beatmap_intent[0] is not project.pad_source_beatmap_intent[1]
