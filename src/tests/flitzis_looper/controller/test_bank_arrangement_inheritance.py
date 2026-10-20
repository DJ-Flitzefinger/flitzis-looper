"""A displaced inheriting occupant also owns global Key Lock intent until settlement."""

from typing import TYPE_CHECKING

import pytest

from tests.flitzis_looper.controller.test_bank_arrangement_controller import _harness

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("clear", [False, True])
@pytest.mark.parametrize("before_inherits", [False, True])
def test_global_edit_guard_includes_last_displaced_occupant_with_explicit_copy_source(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    clear: bool,
    before_inherits: bool,
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    project = harness.app.project
    project.key_lock = False
    project.pad_key_lock[0] = True
    for sample_id in (36, 37, 71):
        project.pad_key_lock[sample_id] = True
    project.pad_key_lock[71] = None if before_inherits else True
    harness.app.persistence.mark_dirty()
    harness.app.persistence.flush()
    request = harness.controller.capture_request(None if clear else 0, 1)
    assert harness.controller.begin(request)
    before = project.model_dump()
    written = harness.app.persistence.config_path.read_bytes()
    revision = harness.app.persistence.revision

    if before_inherits:
        with pytest.raises(RuntimeError, match="Key Lock inheritance is reserved"):
            harness.controller.ensure_global_key_lock_edit_allowed()
    else:
        harness.controller.ensure_global_key_lock_edit_allowed()

    assert project.model_dump() == before
    assert harness.app.persistence.config_path.read_bytes() == written
    assert harness.app.persistence.revision == revision
    # The original worker remains staged, so cancellation must retain its owner.
    assert not harness.controller.cancel()
    harness.run_worker()
    harness.controller.poll()
    assert not harness.controller.active
    assert harness.controller.status == "cancelled"
    assert harness.app.persistence._migration_owner is None
    assert harness.native.bank_publications == []
