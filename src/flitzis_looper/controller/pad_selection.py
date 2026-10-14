"""Content-bound UI selection intent, without Native playback authority."""

from typing import TYPE_CHECKING

from flitzis_looper.models import validate_sample_id
from flitzis_looper.pair_snapshot import project_selected_pair_content

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper.pair_snapshot import PairSnapshot


def record_pad_selection(project: ProjectState, session: SessionState, pad_id: int) -> None:
    """Record the displayed content chosen by every accepted navigation gesture."""
    validate_sample_id(pad_id)
    if session.pad_selection_revision == (1 << 64) - 1:
        message = "Pad selection intent revisions exhausted"
        raise RuntimeError(message)
    content = project.pad_content[pad_id]
    session.pad_selection_content_id = content.instance_id if content is not None else None
    session.pad_selection_revision += 1
    project.selected_pad = pad_id


class PreparedPadSelection:
    """Project one original Pair's selection identically into Config and live UI."""

    def __init__(
        self, project: ProjectState, session: SessionState, snapshot: PairSnapshot
    ) -> None:
        self._project = project
        self._session = session
        self._pair = snapshot.projected()
        self._revision = session.pad_selection_revision
        self._selected_pad = project.selected_pad
        content = project.pad_content[self._selected_pad]
        # A new Pair starts from the current complete display image. Earlier
        # navigation may have selected a lifetime since removed or replaced.
        # Only newer gestures during this own mirror need their captured IDs.
        self._content_id = content.instance_id if content is not None else None

    def projected_pad(self) -> int:
        """Follow the chosen surviving content even across partial own stores."""
        if self._session.pad_selection_revision == self._revision:
            selected_pad, content_id = self._selected_pad, self._content_id
        else:
            selected_pad = self._project.selected_pad
            content_id = self._session.pad_selection_content_id
        return project_selected_pair_content(selected_pad, content_id, self._pair)
