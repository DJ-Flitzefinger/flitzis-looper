"""GUI admission boundary for source-bound pad gestures."""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController


def perform_pad_action(controller: AppController, pad_id: int, execute: Callable[[], None]) -> bool:
    """Report a rejected gesture without replaying it or aborting the GUI frame."""
    residency = controller.transport.residency
    residency.report_launch_error(pad_id, None)
    try:
        execute()
    except (RuntimeError, ValueError) as error:
        residency.report_launch_error(pad_id, str(error))
        return False
    return True
