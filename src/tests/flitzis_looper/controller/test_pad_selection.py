"""Navigation captures displayed content without granting any Native authority."""

import pytest

from flitzis_looper.controller.pad_selection import PreparedPadSelection, record_pad_selection
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from flitzis_looper.pair_snapshot import capture_pair_snapshot


def test_repeated_slot_navigation_recaptures_the_actual_content_after_reuse() -> None:
    project, session = ProjectState(), SessionState()
    original = PadContentIdentity(instance_id="a" * 32)
    replacement = PadContentIdentity(instance_id="b" * 32)
    project.pad_content[0] = original
    record_pad_selection(project, session, 0)
    assert session.pad_selection_revision == 1
    assert session.pad_selection_content_id == original.instance_id
    project.pad_content[0] = replacement
    record_pad_selection(project, session, 0)
    assert session.pad_selection_revision == 2
    assert session.pad_selection_content_id == replacement.instance_id
    record_pad_selection(project, session, 36)
    assert project.selected_pad == 36
    assert session.pad_selection_revision == 3
    assert session.pad_selection_content_id is None
    assert "pad_selection_revision" not in project.model_dump()
    assert SessionState().pad_selection_content_id is None


def test_exhausted_navigation_revision_preserves_the_entire_original_selection() -> None:
    project = ProjectState(selected_pad=36)
    session = SessionState(pad_selection_revision=(1 << 64) - 1, pad_selection_content_id="a" * 32)
    before = project.model_dump_json(), session.model_dump_json()
    with pytest.raises(RuntimeError, match="revisions exhausted"):
        record_pad_selection(project, session, 0)
    assert (project.model_dump_json(), session.model_dump_json()) == before


def test_new_pair_captures_current_selected_replacement_without_reusing_old_navigation() -> None:
    project, session = ProjectState(), SessionState()
    project.sample_paths[0] = "samples/original.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    record_pad_selection(project, session, 0)
    project.sample_paths[0] = "samples/replacement.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="b" * 32)
    snapshot = capture_pair_snapshot(project, 0, 215, operation="move")
    selection = PreparedPadSelection(project, session, snapshot)
    assert session.pad_selection_content_id == "a" * 32
    assert selection.projected_pad() == 215
    assert project.selected_pad == 0
