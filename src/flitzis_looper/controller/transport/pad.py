from typing import TYPE_CHECKING

from flitzis_looper.audio_gain import clamp_gain_db
from flitzis_looper.constants import PAD_EQ_DB_MAX, PAD_EQ_DB_MIN
from flitzis_looper.controller.validation import ensure_finite
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController


class PadController:
    """Manage pad state, like gain, EQ, and key."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._audio = transport._audio
        self._loop = transport.loop

    def set_pad_gain(self, sample_id: int, gain_db: float) -> None:
        validate_sample_id(sample_id)
        ensure_finite(gain_db)
        clamped = clamp_gain_db(gain_db)
        self._audio.set_pad_gain(sample_id, clamped)
        self._project.pad_gain_db[sample_id] = clamped
        self._transport._mark_project_changed()

    def set_pad_eq(self, sample_id: int, low_db: float, mid_db: float, high_db: float) -> None:
        validate_sample_id(sample_id)
        for value in (low_db, mid_db, high_db):
            ensure_finite(value)

        low_db = min(max(low_db, PAD_EQ_DB_MIN), PAD_EQ_DB_MAX)
        mid_db = min(max(mid_db, PAD_EQ_DB_MIN), PAD_EQ_DB_MAX)
        high_db = min(max(high_db, PAD_EQ_DB_MIN), PAD_EQ_DB_MAX)

        self._audio.set_pad_eq(sample_id, low_db, mid_db, high_db)
        self._project.pad_eq_low_db[sample_id] = low_db
        self._project.pad_eq_mid_db[sample_id] = mid_db
        self._project.pad_eq_high_db[sample_id] = high_db
        self._transport._mark_project_changed()

    def set_pad_key_lock(self, sample_id: int, *, enabled: bool) -> None:
        """Enable or disable Key Lock for one pad."""
        validate_sample_id(sample_id)
        if self._project.sample_paths[sample_id] is None:
            if self._project.pad_key_lock[sample_id]:
                disabled = False
                self._audio.set_pad_key_lock(sample_id, disabled)
                self._project.pad_key_lock[sample_id] = False
                self._transport._mark_project_changed()
            return

        if self._project.pad_key_lock[sample_id] is enabled:
            return

        self._audio.set_pad_key_lock(sample_id, enabled)
        self._project.pad_key_lock[sample_id] = enabled
        self._transport._mark_project_changed()

    def toggle_pad_key_lock(self, sample_id: int) -> None:
        """Toggle Key Lock for one pad."""
        validate_sample_id(sample_id)
        self.set_pad_key_lock(sample_id, enabled=not self._project.pad_key_lock[sample_id])

    def set_manual_key(self, sample_id: int, key: str) -> None:
        """Set a pad's manual key override."""
        validate_sample_id(sample_id)
        if not key:
            msg = "key must be a non-empty string"
            raise ValueError(msg)
        self._project.manual_key[sample_id] = key
        self._transport._mark_project_changed()

    def clear_manual_key(self, sample_id: int) -> None:
        """Clear a pad's manual key override."""
        validate_sample_id(sample_id)
        self._project.manual_key[sample_id] = None
        self._transport._mark_project_changed()

    def effective_key(self, sample_id: int) -> str | None:
        """Return the effective key for a pad (manual overrides detected)."""
        validate_sample_id(sample_id)

        manual = self._project.manual_key[sample_id]
        if manual is not None:
            return manual

        analysis = self._project.sample_analysis[sample_id]
        return analysis.key if analysis is not None else None
