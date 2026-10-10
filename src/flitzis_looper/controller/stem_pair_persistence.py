"""Recover unsupported pair metadata locally without granting disk or live rights."""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pydantic import ValidationError


def is_pair_metadata_error(location: tuple[int | str, ...]) -> bool:
    """Recognize only the new per-pad field, not malformed legacy stem intent."""
    return (
        len(location) >= 3
        and location[0] == "stem_cache"
        and type(location[1]) is int
        and location[2] == "pair"
    )


def recover_stem_pair_metadata(recovered: dict[str, object], error: ValidationError) -> None:
    """Fence invalid pair entries while retaining their other saved pad fields."""
    entries = recovered.get("stem_cache")
    if not isinstance(entries, list):
        return
    for item in error.errors():
        location = item["loc"]
        if not is_pair_metadata_error(location):
            continue
        index = location[1]
        if isinstance(index, int) and 0 <= index < len(entries):
            entry = entries[index]
            if isinstance(entry, dict):
                entry["pair"] = None
                entry["available"] = False
