from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.state import AppState


def test_multi_loop_defaults_disabled(state: AppState) -> None:
    assert state.multi_loop is False
    assert state.key_lock is False
    assert state.bpm_lock is False
    assert not state.active_sample_ids


def test_speed_defaults_to_one(state: AppState) -> None:
    assert state.speed == 1.0
