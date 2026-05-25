from typing import Literal, Self

from pydantic import BaseModel, Field, field_validator, model_validator


class MidiBinding(BaseModel):
    """Device-neutral MIDI binding."""

    kind: Literal["note", "cc", "nrpn"]
    channel: int = Field(ge=1, le=16)
    number: int = Field(ge=0, le=16383)

    @property
    def key(self) -> str:
        return f"midi:{self.kind}:{self.channel}:{self.number}"

    @model_validator(mode="after")
    def _validate_number_for_kind(self) -> Self:
        if self.kind in {"note", "cc"} and self.number > 127:
            msg = f"{self.kind} MIDI binding number out of range: {self.number}"
            raise ValueError(msg)
        return self

    @classmethod
    def from_key(cls, key: str) -> MidiBinding:
        parts = key.split(":")
        if len(parts) != 4 or parts[0] != "midi" or parts[1] not in {"note", "cc", "nrpn"}:
            msg = f"invalid MIDI binding key: {key}"
            raise ValueError(msg)
        kind: Literal["note", "cc", "nrpn"]
        if parts[1] == "note":
            kind = "note"
        elif parts[1] == "cc":
            kind = "cc"
        else:
            kind = "nrpn"
        return cls(kind=kind, channel=int(parts[2]), number=int(parts[3]))


class KeyboardBinding(BaseModel):
    """Keyboard binding as a normalized key plus modifiers."""

    key_name: str = Field(min_length=1)
    ctrl: bool = False
    alt: bool = False
    shift: bool = False
    super: bool = False

    @field_validator("key_name", mode="before")
    @classmethod
    def _normalize_key_name(cls, value: object) -> object:
        if isinstance(value, str):
            return value.strip().upper()
        return value

    @property
    def key(self) -> str:
        modifiers = "".join(
            (
                "C" if self.ctrl else "-",
                "A" if self.alt else "-",
                "S" if self.shift else "-",
                "M" if self.super else "-",
            )
        )
        return f"keyboard:{self.key_name}:{modifiers}"

    @classmethod
    def from_key(cls, key: str) -> KeyboardBinding:
        parts = key.split(":")
        if len(parts) != 3 or parts[0] != "keyboard" or len(parts[2]) != 4:
            msg = f"invalid keyboard binding key: {key}"
            raise ValueError(msg)
        mods = parts[2]
        return cls(
            key_name=parts[1],
            ctrl=mods[0] == "C",
            alt=mods[1] == "A",
            shift=mods[2] == "S",
            super=mods[3] == "M",
        )
