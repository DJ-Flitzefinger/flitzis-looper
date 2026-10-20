"""Original result ownership across actual artifact promotion and retained delivery."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import stems
from tests.flitzis_looper.controller.test_pair_stem_mirror import _prepare
from tests.flitzis_looper.controller.test_pair_stem_observers import (
    _job,
    _move,
    _rig,
    _subscribe,
)

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("failure", ["promotion", "integrity"])
@pytest.mark.parametrize("deferred", [False, True])
def test_promotion_failure_latches_every_original_immediate_and_moved_holder(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure: str, *, deferred: bool
) -> None:
    rig = _rig(tmp_path, monkeypatch)
    job = _job(rig)
    _subscribe(rig, job, 36)
    original, outside = job.subscribers()
    mirror, acknowledgement = _prepare(rig.controller)
    if deferred:
        rig.controller._pair_deferred_sample_ids = frozenset((0, 215))
    if failure == "promotion":
        monkeypatch.setattr(
            stems,
            "promote_generation_artifacts",
            Mock(side_effect=OSError("owned promotion failed")),
        )
    else:
        monkeypatch.setattr(rig.controller, "_entry_files_available", lambda _entry: False)
    rig.controller._run_stem_backend(job)
    rig.controller._poll_generation_events()

    first_failure = outside.interest_owner.failure
    assert first_failure is not None
    assert not outside.interest_owner.committed
    assert not rig.controller.generation_interest_is_pending(outside.interest_owner)
    if deferred:
        assert original.interest_owner.failure is None
        terminal = rig.controller._pair_held_generation_events[job, "terminal"]
        assert terminal.promotion_error == first_failure
        # The first slot-local message is transient. The retained delivery must
        # provide the original outcome after an actual accepted Pair relocation.
        rig.controller._session.stem_generation_errors.clear()
        _move(rig, mirror, acknowledgement)
        rig.controller._pair_deferred_sample_ids = frozenset()
        rig.controller._poll_generation_events()
    assert original.interest_owner.failure == first_failure
    assert not original.interest_owner.committed
    assert not rig.controller.generation_interest_is_pending(original.interest_owner)
    assert not rig.controller._jobs
    assert not rig.controller._pair_held_generation_events
    assert rig.queued == []
