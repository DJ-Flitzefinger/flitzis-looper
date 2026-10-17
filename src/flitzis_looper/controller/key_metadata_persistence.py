"""Recover unsupported key metadata locally without discarding performer intent."""

import json

from pydantic import ValidationError

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.key_intent import PadKeyIntent


def recover_project_key_intent(data: dict[str, object]) -> dict[str, object]:
    """Retain each valid known field when only the new key-intent payload is invalid.

    Explicit correction=None stays explicit. Legacy manual_key is never imported
    here, since the presence of the new field already establishes its precedence.
    Native permits and historical source validity are never restored by recovery.
    """
    values = data.get("pad_key_intent")
    recovered: list[PadKeyIntent] = []
    for sample_id in range(NUM_SAMPLES):
        raw = values[sample_id] if isinstance(values, list) and sample_id < len(values) else None
        intent = PadKeyIntent()
        if isinstance(raw, dict):
            # A valid octave-bearing base can depend on its compensating extra.
            # Recover that pair together, before unrelated field-level fallback.
            numeric = {field: raw[field] for field in ("base_shift", "extra_shift") if field in raw}
            try:
                intent = intent.changed(**numeric)
                numeric_valid = True
            except ValidationError:
                numeric_valid = False
            for field in PadKeyIntent.model_fields:
                if field not in raw:
                    continue
                if numeric_valid and field in {"base_shift", "extra_shift"}:
                    continue
                try:
                    intent = intent.changed(**{field: raw[field]})
                except ValidationError:
                    # Discard only this unsupported field, not other saved settings.
                    continue
            if "extra_retrigger" not in raw and type(raw.get("retrigger")) is bool:
                intent = intent.changed(extra_retrigger=raw["retrigger"])
        recovered.append(intent)
    return data | {"pad_key_intent": recovered}


def load_project_with_key_recovery(raw: str) -> dict[str, object] | None:
    """Decode a project object for bounded key-only fallback after validation fails."""
    data = json.loads(raw)
    if not isinstance(data, dict) or "pad_key_intent" not in data:
        return None
    return recover_project_key_intent(data)
