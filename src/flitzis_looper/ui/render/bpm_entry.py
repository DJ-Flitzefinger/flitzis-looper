import math
import string
from decimal import Decimal

BPM_ENTRY_DIGITS = frozenset(string.digits)


def sanitize_bpm_entry_text(text: str) -> str:
    """Retain decimal BPM digits without rounding or truncating their precision."""
    sanitized: list[str] = []
    has_decimal = False

    for char in text.replace(",", "."):
        if char in BPM_ENTRY_DIGITS:
            sanitized.append(char)
        elif char == "." and not has_decimal:
            has_decimal = True
            sanitized.append(char)

    return "".join(sanitized)


def format_bpm_entry_value(bpm: float) -> str:
    """Return a round-trip decimal buffer without unsupported exponent notation."""
    return format(Decimal(str(bpm)), "f")


def filtered_bpm_entry_char(
    char_code: int,
    current_text: str,
    _cursor_pos: int,
    *,
    has_selection: bool,
) -> int | None:
    """Return a replacement char code, or None when BPM entry must reject it."""
    if char_code == 0:
        return 0

    accepted: int | None = None
    char = chr(char_code)
    if char == ",":
        char = "."
        char_code = ord(".")

    if char in BPM_ENTRY_DIGITS or (char == "." and ("." not in current_text or has_selection)):
        accepted = char_code

    return accepted


def parse_bpm_entry_text(text: str) -> float | None:
    """Parse a finite positive BPM without quantizing its fractional value."""
    sanitized = sanitize_bpm_entry_text(text)
    if sanitized in {"", "."}:
        return None
    try:
        value = float(sanitized)
    except ValueError:
        return None
    if not math.isfinite(value) or value <= 0.0:
        return None
    return value
