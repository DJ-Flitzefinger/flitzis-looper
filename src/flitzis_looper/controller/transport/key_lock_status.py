"""Read-only KEYLOCK presentation derived from current native feedback."""

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class KeyLockStatus:
    """Keep requested intent and confirmed processing separate for a pad or broadcast."""

    requested: bool
    effective: bool | None
    pending: bool = False
    unconfirmed: bool = False
    error: str | None = None
    mixed: bool = False
