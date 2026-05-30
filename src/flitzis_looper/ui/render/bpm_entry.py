import math
import string

BPM_ENTRY_DIGITS = frozenset(string.digits)


def sanitize_bpm_entry_text(text: str) -> str:
    sanitized: list[str] = []
    has_decimal = False
    decimal_places = 0

    for char in text.replace(",", "."):
        if char in BPM_ENTRY_DIGITS:
            if has_decimal:
                if decimal_places >= 2:
                    continue
                decimal_places += 1
            sanitized.append(char)
        elif char == "." and not has_decimal:
            has_decimal = True
            sanitized.append(char)

    return "".join(sanitized)


def filtered_bpm_entry_char(
    char_code: int,
    current_text: str,
    cursor_pos: int,
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

    if char in BPM_ENTRY_DIGITS:
        if "." not in current_text or has_selection:
            accepted = char_code
        else:
            decimals = current_text.split(".", 1)[1]
            if cursor_pos <= current_text.index(".") or len(decimals) < 2:
                accepted = char_code
    elif char == "." and ("." not in current_text or has_selection):
        accepted = char_code

    return accepted


def parse_bpm_entry_text(text: str) -> float | None:
    sanitized = sanitize_bpm_entry_text(text)
    if sanitized in {"", "."}:
        return None
    try:
        value = float(sanitized)
    except ValueError:
        return None
    if not math.isfinite(value) or value <= 0.0:
        return None
    return round(value, 2)
