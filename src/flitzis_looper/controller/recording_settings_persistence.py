"""Recover only unsupported saved recording preferences before project validation."""

import json

from pydantic import ValidationError

from flitzis_looper.recording_options import RecordingOptions

_FIELD_LABELS = {
    "format": "recording format",
    "wav_encoding": "WAV precision",
    "flac_bit_depth": "FLAC precision",
    "flac_compression": "FLAC compression",
    "mp3_bitrate_kbps": "MP3 bitrate",
}
_UNSUPPORTED_INTEGER = object()


def _config_integer(token: str) -> object:
    """Keep a rejected numeric token invalid without changing interpreter limits."""
    try:
        return int(token)
    except ValueError:
        # Strict recording validation will reject this value and default only
        # that preference. Never turn an unrelated numeric token into a string.
        return _UNSUPPORTED_INTEGER


def _recover_fields(
    values: dict[str, object], defaults: dict[str, object]
) -> tuple[dict[str, object], str | None]:
    repaired = dict(defaults)
    reset_fields: list[str] = []
    for field in defaults:
        if field not in values:
            reset_fields.append(field)
            continue
        try:
            candidate = RecordingOptions.model_validate(
                defaults | {field: values[field]}, strict=True
            )
        except ValidationError:
            reset_fields.append(field)
        else:
            repaired[field] = candidate.model_dump(mode="json")[field]
    unknown_fields = any(field not in defaults for field in values)
    if not reset_fields and not unknown_fields:
        return repaired, None
    details: list[str] = []
    if reset_fields:
        labels = (_FIELD_LABELS.get(field, "recording preference") for field in reset_fields)
        details.append(f"restored defaults for {', '.join(labels)}")
    if unknown_fields:
        details.append("ignored unknown recording fields")
    return repaired, f"Recording settings: {'; '.join(details)}."


def recover_recording_settings_json(raw: str) -> tuple[str, str | None]:
    """Preserve valid preferences and return a nonblocking recording diagnostic.

    Unreadable JSON and non-project shapes retain the existing project loader's
    behavior. Only a repaired recording payload causes JSON reserialization;
    unrelated project values still pass through the normal project validation and
    metadata recovery. Loading never writes the original Config.
    """
    try:
        data = json.loads(raw, parse_int=_config_integer)
    except ValueError, RecursionError:
        return raw, None
    if not isinstance(data, dict):
        return raw, None

    defaults: dict[str, object] = RecordingOptions().model_dump(mode="json")
    values = data.get("recording_options")
    warning: str | None
    if "recording_options" not in data:
        repaired = defaults
        warning = "Recording settings were missing; restored recording defaults."
    elif not isinstance(values, dict):
        repaired = defaults
        warning = "Invalid recording settings; restored recording defaults."
    else:
        repaired, warning = _recover_fields(values, defaults)
        if warning is None:
            return raw, None

    data["recording_options"] = repaired
    try:
        return json.dumps(data, ensure_ascii=False), warning
    except TypeError, ValueError, RecursionError:
        # A remaining integer marker or serialization limit cannot be safely
        # rewritten. Retain the original normal model/fence path and never
        # report a selective repair that was not actually applied.
        return raw, None
