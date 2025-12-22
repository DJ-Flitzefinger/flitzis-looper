from dataclasses import dataclass, field

from flitzis_looper.constants import NUM_PADS


@dataclass
class AppState:
    # Looper state
    selected_bank: int = 0
    sample_paths: list[str | None] = field(default_factory=lambda: [None] * NUM_PADS)
    multi_loop: bool = False
    key_lock: bool = False
    bpm_lock: bool = False
    volume: float = 1.0
    speed: float = 1.0
    active_sample_ids: set[int] = field(default_factory=set)

    # UI
    selected_pad: int = 0
    sidebar_left_expanded: bool = False
    sidebar_right_expanded: bool = True

    # Non-persistent
    pressed_pads: list[bool] = field(default_factory=lambda: [False] * NUM_PADS)
    pending_file_dialog: int | None = None  # pad_id
