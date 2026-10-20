"""A genuinely committed Copy retains its lineage through material migration."""

from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller import material_migration as migration_module
from flitzis_looper.controller.pad_arrangement import (
    NativeAdapterProtocol,
    PadArrangementController,
)
from flitzis_looper.controller.persistence import ProjectPersistence
from tests.flitzis_looper.controller.test_material_migration import (
    _IDS,
    _MATERIAL,
    _NEW,
    _OLD,
    _DeliveredLease,
    _Harness,
)
from tests.flitzis_looper.controller.test_pad_arrangement import _Assets, _Jobs, _ScriptedNative

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle


@pytest.fixture
def migration(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> _Harness:
    monkeypatch.setattr(migration_module, "ProjectAssetLease", _DeliveredLease)
    return _Harness(controller, audio_engine_mock)


def test_material_migration_preserves_committed_copy_lineage_in_candidate_and_saved_image(
    migration: _Harness,
) -> None:
    app = migration.controller
    project = app.project
    source = project.pad_content[0]
    assert source is not None
    project.sample_paths[215] = None
    project.pad_content[215] = None
    native = _ScriptedNative()
    jobs = _Jobs()
    copy = PadArrangementController(
        app.persistence,
        app.session,
        cast("NativeAdapterProtocol", native),
        cast("ProjectAssetLifecycle", _Assets()),
        stem_task_runner=jobs,
    )

    assert copy.copy_to_empty(0, 215)
    jobs.run()
    copy.poll()
    native.complete_ack = True
    copy.poll()

    assert copy.status == "completed"
    assert not copy.active
    copied = project.pad_content[215]
    assert copied is not None
    assert copied.instance_id != source.instance_id
    assert copied.copied_from == source.instance_id
    assert copied.material_id is None
    assert project.sample_paths[215] == _OLD
    app._assets.sync_assignments()

    migration.begin()
    migration.migration.poll()
    assert set(migration.sources) == set(_IDS)
    with pytest.raises(ValueError, match="matching actual migration source ACK"):
        migration.migration._candidate()
    project.pad_gain_db[215] = -8.5
    app.persistence.mark_dirty()
    migration.acknowledge()
    assert all(ticket.is_current() for ticket in migration.sources.values())

    candidate = migration.migration._candidate()

    assert candidate.pad_content[215] == copied.model_copy(update={"material_id": _MATERIAL})
    assert candidate.sample_paths[215] == _NEW
    assert project.pad_content[215] == copied
    assert project.sample_paths[215] == _OLD
    migration.migration.poll()
    assert migration.migration.status == "config_committed"
    saved = ProjectPersistence.from_config_path(app.persistence.config_path).project
    for image in (project, saved):
        content = image.pad_content[215]
        assert content is not None
        assert content.instance_id == copied.instance_id
        assert content.copied_from == source.instance_id
        assert content.material_id == _MATERIAL
        assert image.sample_paths[215] == _NEW
        assert image.pad_gain_db[215] == -8.5
    assert migration.delivered[215].released
