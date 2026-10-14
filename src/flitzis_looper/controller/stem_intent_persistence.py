"""Contained recovery of musical selection intent without current Native authority."""

from pydantic import ValidationError

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.stem_intent import PadStemIntent


def recover_project_stem_intent(data: dict[str, object]) -> dict[str, object]:
    """Preserve valid known slots and default rejected selection metadata only.

    The caller retains the original malformed config and reports validation.
    Unknown/extra slots never become assignments or current Native permissions.
    """
    recovered = dict(data)
    raw = data.get("pad_stem_intent")
    values = [PadStemIntent() for _ in range(NUM_SAMPLES)]
    if isinstance(raw, list):
        for sample_id, value in enumerate(raw[:NUM_SAMPLES]):
            try:
                values[sample_id] = PadStemIntent.model_validate(value)
            except ValidationError:
                continue
    recovered["pad_stem_intent"] = values
    return recovered
