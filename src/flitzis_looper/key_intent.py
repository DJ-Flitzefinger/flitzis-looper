"""Pure durable key metadata and neutral transposition intent.

These values describe performer intent. They never admit a native pitch event or
certify support in the current audio backend.
"""

from collections.abc import Iterable, MutableSequence
from typing import Annotated, Literal, overload

from pydantic import BaseModel, ConfigDict, Field

MAX_KEY_EPOCH = (1 << 64) - 1
type KeyEpoch = Annotated[int, Field(strict=True, ge=0, le=MAX_KEY_EPOCH)]

_ROOT_NAMES = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")
_FLAT_ALIASES = {"Ab": "G#", "Eb": "D#", "Bb": "A#"}


class MusicalKey(BaseModel):
    """Recognized pitch class and mode, independent of an octave or live permit."""

    model_config = ConfigDict(frozen=True, extra="forbid")
    root: int = Field(strict=True, ge=0, le=11)
    mode: Literal["major", "minor"]

    @property
    def label(self) -> str:
        """Return one canonical sharp spelling for this pitch class and mode."""
        return _ROOT_NAMES[self.root] + ("m" if self.mode == "minor" else "")

    def shifted(self, semitones: int) -> MusicalKey:
        """Derive a nominal label without mutating any numerical intent."""
        if isinstance(semitones, bool) or not isinstance(semitones, int):
            msg = "semitones must be an integer"
            raise TypeError(msg)
        return MusicalKey(root=(self.root + semitones) % 12, mode=self.mode)


def recognize_key(raw: str | None) -> MusicalKey | None:
    """Recognize the native producer's 24 names and its six accepted flat aliases."""
    if raw is None:
        return None
    minor = raw.endswith("m")
    name = raw[:-1] if minor else raw
    name = _FLAT_ALIASES.get(name, name)
    if name not in _ROOT_NAMES:
        return None
    return MusicalKey(root=_ROOT_NAMES.index(name), mode="minor" if minor else "major")


class SourceKeyVersion(BaseModel):
    """Versioned source display metadata; saved versions are never live authority."""

    model_config = ConfigDict(frozen=True, extra="forbid")
    version: KeyEpoch = 0
    raw_key: str = Field(strict=True)

    @property
    def recognized(self) -> MusicalKey | None:
        """Return recognized metadata without guessing an arbitrary legacy string."""
        return recognize_key(self.raw_key)


class PadKeyIntent(BaseModel):
    """Immutable per-content intent with independently resettable numeric shifts."""

    model_config = ConfigDict(frozen=True, extra="forbid")
    schema_version: int = Field(default=1, strict=True, ge=1, le=1)
    source: SourceKeyVersion | None = None
    correction: str | None = Field(default=None, strict=True)
    analysis_epoch: KeyEpoch = 0
    correction_epoch: KeyEpoch = 0
    base_shift: int = Field(default=0, strict=True, ge=-5, le=6)
    extra_shift: int = Field(default=0, strict=True, ge=-18, le=18)
    retrigger: bool = Field(default=False, strict=True)

    def changed(self, **updates: object) -> PadKeyIntent:
        """Construct a fully validated new value before replacing saved intent."""
        return PadKeyIntent.model_validate(self.model_dump() | updates)

    def corrected(self, raw: str | None) -> PadKeyIntent:
        """Set or remove metadata correction while preserving all other intent."""
        if raw == self.correction:
            return self
        return self.changed(correction=raw, correction_epoch=next_key_epoch(self.correction_epoch))

    @property
    def source_label(self) -> str | None:
        """Return corrected source text, retaining unknown legacy display metadata."""
        if self.correction is not None:
            return self.correction
        return self.source.raw_key if self.source is not None else None

    @property
    def source_key(self) -> MusicalKey | None:
        """Return only recognized source metadata for absolute-key calculations."""
        return recognize_key(self.source_label)

    @property
    def base_key(self) -> MusicalKey | None:
        """Derive the nominal base key without changing the chosen shift."""
        source = self.source_key
        return source.shifted(self.base_shift) if source is not None else None

    @property
    def result_key(self) -> MusicalKey | None:
        """Derive the nominal result pitch class; numeric shifts retain octave intent."""
        source = self.source_key
        return source.shifted(self.base_shift + self.extra_shift) if source is not None else None

    def with_base_key(self, target: MusicalKey) -> PadKeyIntent:
        """Choose an absolute same-mode key idempotently with the fixed +6 tie."""
        source = self.source_key
        if source is None or source.mode != target.mode:
            msg = "absolute key requires a recognized source and the same major/minor mode"
            raise ValueError(msg)
        delta = (target.root - source.root) % 12
        return self.changed(base_shift=delta - 12 if delta > 6 else delta)


def next_key_epoch(epoch: int) -> int:
    """Advance a bounded epoch without wrapping or reusing an old metadata version."""
    if epoch >= MAX_KEY_EPOCH:
        msg = "key metadata epoch capacity exhausted"
        raise ValueError(msg)
    return epoch + 1


class KeyCorrectionView(MutableSequence[str | None]):
    """Fixed-size legacy manual_key facade over the sole durable intent table."""

    def __init__(self, intents: list[PadKeyIntent]) -> None:
        self._intents = intents

    def __len__(self) -> int:
        return len(self._intents)

    @overload
    def __getitem__(self, index: int) -> str | None: ...

    @overload
    def __getitem__(self, index: slice[int | None, int | None, int | None]) -> list[str | None]: ...

    def __getitem__(
        self, index: int | slice[int | None, int | None, int | None]
    ) -> str | list[str | None] | None:
        if isinstance(index, slice):
            return [intent.correction for intent in self._intents[index]]
        return self._intents[index].correction

    @overload
    def __setitem__(self, index: int, value: str | None) -> None: ...

    @overload
    def __setitem__(
        self, index: slice[int | None, int | None, int | None], value: Iterable[str | None]
    ) -> None: ...

    def __setitem__(
        self,
        index: int | slice[int | None, int | None, int | None],
        value: str | Iterable[str | None] | None,
    ) -> None:
        current = list(self)
        if isinstance(index, slice):
            if value is None or isinstance(value, str):
                msg = "manual_key slice requires an iterable of corrections"
                raise ValueError(msg)
            current[index] = value
        else:
            if value is not None and not isinstance(value, str):
                msg = "manual_key must contain strings or None"
                raise ValueError(msg)
            current[index] = value
        if len(current) != len(self):
            msg = "manual_key must retain its fixed pad count"
            raise ValueError(msg)
        prepared = [
            intent.corrected(raw) for intent, raw in zip(self._intents, current, strict=True)
        ]
        self._intents[:] = prepared

    def __delitem__(self, index: int | slice[int | None, int | None, int | None]) -> None:
        msg = "manual_key must retain its fixed pad count"
        raise ValueError(msg)

    def insert(self, index: int, value: str | None) -> None:
        msg = "manual_key must retain its fixed pad count"
        raise ValueError(msg)

    def __eq__(self, other: object) -> bool:
        return list(self) == other

    def __hash__(self) -> int:
        msg = "mutable key correction views are not hashable"
        raise TypeError(msg)
