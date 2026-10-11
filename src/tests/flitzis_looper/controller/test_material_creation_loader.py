"""Loader persistence policy with explicit native manifest/ACK doubles.

Readable binding resolution and saved ProjectState are real; the numeric birth
receipt is a policy fixture. Native fresh ACK and physical cleanup are Rust proofs.
"""

import json
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import PadContentIdentity, ProjectState
from tests.flitzis_looper.controller.test_material_creation_history import _policy_birth

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.material_migration_model import MaterialCreationReceipt


def _loaded_birth(controller: AppController, audio: Mock) -> tuple[str, MaterialCreationReceipt]:
    samples = Path("samples")
    samples.mkdir(exist_ok=True)
    artifact, alias = _policy_birth(samples)
    creation = artifact.evidence.material_creation
    assert creation is not None
    audio.load_sample_async.return_value = 71
    audio.cold_source_manifest.return_value = None
    controller.loader.load_sample_async(215, alias.old_reference)
    audio.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 215,
            "request_id": 71,
            "cached_path": alias.new_reference,
            "duration_s": 1.0,
        },
        None,
    ]
    controller.loader.poll_loader_events()
    assert controller.project.sample_paths[215] == alias.new_reference
    content = controller.project.pad_content[215]
    assert content is not None
    assert content.material_id == alias.material_id
    assert content.material_creation is None
    assert controller.loader._material_creation_pending[215] == (
        content.instance_id,
        alias.new_reference,
    )
    audio.poll_loader_events.side_effect = None
    audio.poll_loader_events.return_value = None
    return alias.new_reference, creation


def test_loader_birth_query_waits_for_native_manifest_and_persists_same_instance_and_key_intent(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    reference, creation = _loaded_birth(controller, audio_engine_mock)
    previous = controller.project.pad_content[215]
    assert previous is not None
    intent = PadKeyIntent(
        source=SourceKeyVersion(version=9, raw_key="Em"),
        correction="Cm",
        base_shift=-4,
        extra_shift=12,
        retrigger=True,
    )
    controller.project.pad_key_intent[215] = intent
    restore = Mock()
    controller.loader.bind_material_creation_restore(restore)
    controller.loader.poll_loader_events()
    assert 215 in controller.loader._material_creation_pending
    audio_engine_mock.cold_source_manifest.return_value = json.dumps({
        "original_path": reference,
        "material_id": previous.material_id,
        "material_creation": creation.model_dump(),
    })
    controller.loader.poll_loader_events()
    saved_content = controller.project.pad_content[215]
    assert saved_content is not None
    assert saved_content.instance_id == previous.instance_id
    assert saved_content.material_creation == creation
    assert controller.project.pad_key_intent[215] == intent
    assert 215 not in controller.loader._material_creation_pending
    restore.assert_not_called()
    controller.persistence.flush()
    saved = ProjectState.model_validate_json(controller.persistence.config_path.read_bytes())
    assert saved.pad_content[215] == saved_content
    assert saved.pad_key_intent[215] == intent
    audio_engine_mock.restore_material_creation.assert_not_called()
    audio_engine_mock.retire_project_asset.assert_not_called()


@pytest.mark.parametrize("stale", ["instance", "path", "birth_root"])
def test_loader_birth_metadata_cannot_follow_stale_instance_path_or_another_root(
    controller: AppController, audio_engine_mock: Mock, stale: str
) -> None:
    reference, creation = _loaded_birth(controller, audio_engine_mock)
    content = controller.project.pad_content[215]
    assert content is not None
    restore = Mock()
    controller.loader.bind_material_creation_restore(restore)
    if stale == "instance":
        controller.project.pad_content[215] = PadContentIdentity(
            instance_id="f" * 32, material_id=content.material_id
        )
    elif stale == "path":
        controller.project.sample_paths[215] = "samples/old.wav"
    else:
        creation = creation.model_copy(update={"root_reference": "samples/Another readable root"})
    before = controller.project.model_dump()
    audio_engine_mock.cold_source_manifest.reset_mock()
    audio_engine_mock.cold_source_manifest.return_value = json.dumps({
        "original_path": reference,
        "material_id": content.material_id,
        "material_creation": creation.model_dump(),
    })
    controller.loader.poll_loader_events()
    assert controller.project.model_dump() == before
    assert 215 not in controller.loader._material_creation_pending
    if stale == "birth_root":
        assert "another canonical source root" in controller.session.sample_load_errors[215]
    else:
        audio_engine_mock.cold_source_manifest.assert_not_called()
    restore.assert_not_called()
    audio_engine_mock.restore_material_creation.assert_not_called()


def test_loader_unknown_fresh_manifest_routes_exact_saved_birth_to_guarded_restorer(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    reference, creation = _loaded_birth(controller, audio_engine_mock)
    content = controller.project.pad_content[215]
    assert content is not None
    saved = content.model_copy(update={"material_creation": creation})
    controller.project.pad_content[215] = saved
    restore = Mock()
    controller.loader.bind_material_creation_restore(restore)
    audio_engine_mock.cold_source_manifest.return_value = json.dumps({
        "original_path": reference,
        "material_id": content.material_id,
        "material_creation": None,
    })
    controller.loader.poll_loader_events()
    restore.assert_called_once_with(215, saved)
    assert controller.project.pad_content[215] == saved
    audio_engine_mock.restore_material_creation.assert_not_called()
