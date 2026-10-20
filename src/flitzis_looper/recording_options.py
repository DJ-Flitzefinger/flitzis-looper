"""Immutable recording defaults, independent of Native capture and encoder readiness."""

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, field_validator

type RecordingFormat = Literal["wav", "flac", "mp3"]
type WavEncoding = Literal["float32", "pcm16", "pcm24"]


class RecordingOptions(BaseModel):
    """Global format and retained per-format choices for future recording takes."""

    model_config = ConfigDict(
        frozen=True, extra="forbid", revalidate_instances="always", strict=True
    )

    format: RecordingFormat = "wav"
    wav_encoding: WavEncoding = "float32"
    flac_bit_depth: int = Field(default=24, strict=True)
    flac_compression: int = Field(default=8, strict=True, ge=0, le=8)
    mp3_bitrate_kbps: int = Field(default=320, strict=True)

    @field_validator("flac_bit_depth")
    @classmethod
    def _validate_flac_bit_depth(cls, value: int) -> int:
        # Integer Literals can accept equal floats; strict integer validation
        # precedes membership so 16.0, True and numeric strings cannot enter.
        if value not in {16, 24}:
            message = "FLAC recording bit depth must be 16 or 24"
            raise ValueError(message)
        return value

    @field_validator("mp3_bitrate_kbps")
    @classmethod
    def _validate_mp3_bitrate_kbps(cls, value: int) -> int:
        if value not in {128, 192, 256, 320}:
            message = "MP3 recording bitrate must be 128, 192, 256 or 320 kbit/s"
            raise ValueError(message)
        return value
