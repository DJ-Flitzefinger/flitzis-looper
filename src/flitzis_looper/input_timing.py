def validate_input_timestamp_ns(value: object) -> int | None:
    """Validate a transient timestamp captured from the Rust engine's epoch.

    Args:
        value: Nanoseconds from the active engine epoch, or None for legacy input.

    Returns:
        The unchanged timestamp, including zero, or None.

    Raises:
        ValueError: If the value is not an unsigned 64-bit integer or None.
    """
    if value is None:
        return None
    if type(value) is not int or not 0 <= value <= (1 << 64) - 1:
        msg = "received_at_ns must be an unsigned 64-bit integer or None"
        raise ValueError(msg)
    return value
