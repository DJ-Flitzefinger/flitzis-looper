"""Recover only malformed output intent while retaining its original disk value."""

import json
from dataclasses import dataclass

from pydantic import ValidationError

from flitzis_looper.audio_output_options import AudioOutputOptions


@dataclass(frozen=True, slots=True)
class OutputRecovery:
    """Retained invalid field; it grants no automatic/default playback request."""

    original: object
    message: str


def recover_audio_output_json(raw: str) -> tuple[str, OutputRecovery | None]:
    """Keep valid project fields when only output preferences fail validation."""
    try:
        data = json.loads(raw)
    except ValueError, RecursionError:
        return raw, None
    if not isinstance(data, dict) or "audio_output" not in data:
        return raw, None
    try:
        AudioOutputOptions.model_validate(data["audio_output"])
    except ValidationError:
        recovery = OutputRecovery(
            data["audio_output"],
            "Saved audio output settings are invalid. Review and apply output settings.",
        )
        data["audio_output"] = AudioOutputOptions().model_dump(mode="json")
        return json.dumps(data), recovery
    return raw, None
