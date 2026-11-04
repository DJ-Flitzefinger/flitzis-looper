"""Output preferences and cached observations; rendering never queries a driver."""

from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.audio_output_options import AudioOutputOptions
from flitzis_looper.ui.contextmanager import item_width

if TYPE_CHECKING:
    from collections.abc import Sequence

    from flitzis_looper.controller.audio_output import AudioOutputController

_SAMPLE_TYPES = {
    0: "Int16 MSB",
    1: "Int24 MSB",
    2: "Int32 MSB",
    3: "Float32 MSB",
    4: "Float64 MSB",
    8: "Int32 MSB16",
    9: "Int32 MSB18",
    10: "Int32 MSB20",
    11: "Int32 MSB24",
    16: "Int16 LSB",
    17: "Int24 LSB",
    18: "Int32 LSB",
    19: "Float32 LSB",
    20: "Float64 LSB",
    24: "Int32 LSB16",
    25: "Int32 LSB18",
    26: "Int32 LSB20",
    27: "Int32 LSB24",
}


def _channel_label(channel: dict[str, object]) -> str:
    index = channel.get("index")
    number = index + 1 if isinstance(index, int) else "?"
    code = channel.get("native_sample_type")
    format_ = (
        _SAMPLE_TYPES.get(code, f"unsupported type {code}")
        if isinstance(code, int)
        else "unknown format"
    )
    return f"{number}: {channel.get('name')} / group {channel.get('group')} / {format_}"


def _installed_channels(control: AudioOutputController) -> list[dict[str, object]]:
    info = control.observed or {}
    asio = info.get("asio_control")
    if (
        control.draft.backend != "asio"
        or not isinstance(asio, dict)
        or control.draft.device_id is None
        or control.draft.device_id != asio.get("device_id")
    ):
        return []
    channels = asio.get("channels")
    return [row for row in channels if isinstance(row, dict)] if isinstance(channels, list) else []


def _replace(control: AudioOutputController, **changes: object) -> None:
    control.draft = AudioOutputOptions.model_validate(control.draft.model_dump() | changes)


def _choice(
    label: str, current: str | None, values: Sequence[tuple[str | None, str]]
) -> tuple[bool, str | None]:
    preview = next((name for value, name in values if value == current), str(current))
    with item_width(320):
        if not imgui.begin_combo(label, preview):
            return False, current
        try:
            for value, name in values:
                if imgui.selectable(name, value == current)[0]:
                    return True, value
        finally:
            imgui.end_combo()
    return False, current


def _automatic_number(control: AudioOutputController, label: str, field: str, default: int) -> None:
    value = getattr(control.draft, field)
    changed, automatic = imgui.checkbox(f"Auto##{field}", value is None)
    if changed:
        _replace(control, **{field: None if automatic else default})
        value = getattr(control.draft, field)
    imgui.same_line()
    imgui.begin_disabled(value is None)
    try:
        with item_width(220):
            changed, number = imgui.input_int(label, default if value is None else value)
        if changed and value is not None and 0 < number <= 0x7FFF_FFFF:
            _replace(control, **{field: number})
    finally:
        imgui.end_disabled()


def _routing(control: AudioOutputController) -> None:
    automatic = control.draft.left_channel is None
    changed, selected = imgui.checkbox("Automatic stereo routing", automatic)
    if changed:
        _replace(
            control, left_channel=None if selected else 0, right_channel=None if selected else 1
        )
    if control.draft.left_channel is None:
        return
    channels = _installed_channels(control)
    for field, label in (
        ("left_channel", "Left physical output"),
        ("right_channel", "Right physical output"),
    ):
        current = getattr(control.draft, field)
        if channels:
            values = [
                (str(row["index"]), _channel_label(row))
                for row in channels
                if row.get("pcm_supported") is True
            ]
            changed, channel_id = _choice(label, str(current), values)
            number = int(channel_id) + 1 if channel_id is not None else 0
        else:
            with item_width(220):
                changed, number = imgui.input_int(label, current + 1)
        other = (
            control.draft.right_channel if field == "left_channel" else control.draft.left_channel
        )
        if changed and 0 < number <= 0x7FFF_FFFF and number - 1 != other:
            _replace(control, **{field: number - 1})


def _observations(control: AudioOutputController) -> None:
    info = control.observed
    if info is None:
        imgui.text_wrapped("No output configuration has been installed.")
        return
    asio = info.get("asio_control")
    asio = asio if isinstance(asio, dict) else {}
    driver = asio.get("driver")
    driver = driver if isinstance(driver, dict) else {}
    name = asio.get("device_name") or info.get("endpoint_name") or "Default output"
    imgui.text_wrapped(f"Installed: {info.get('host', 'Unknown backend')} / {name}")
    rate = info.get("sample_rate_hz")
    frames = info.get("initialized_buffer_frames") or info.get(
        "accepted_initialization_period_frames"
    )
    imgui.text_wrapped(
        f"Initialized rate: {rate} Hz. Buffer / engine period: "
        f"{frames if frames is not None else 'host managed'} frames."
    )
    callback = info.get("callback_observation")
    if isinstance(callback, dict) and callback.get("callback_count"):
        imgui.text_wrapped(
            f"Observed callbacks: {callback.get('min_frames')} to "
            f"{callback.get('max_frames')} frames; latest {callback.get('last_frames')} frames."
        )
    else:
        imgui.text_wrapped("Actual callback size has not been observed yet.")
    if driver:
        imgui.text_wrapped(
            f"Driver buffer limits: {driver.get('min_frames')} to {driver.get('max_frames')}; "
            f"preferred {driver.get('preferred_frames')}; granularity {driver.get('granularity')}."
        )
        imgui.text_wrapped(
            f"Driver-reported output latency: {driver.get('output_latency_frames')} frames. "
            "This is separate from the nominal buffer interval."
        )
        _asio_observations(asio, driver)
    periods = info.get("engine_periods")
    if isinstance(periods, dict):
        imgui.text_wrapped(
            f"Shared engine periods for the initialized format: {periods.get('min')} "
            f"to {periods.get('max')} frames, multiples of {periods.get('fundamental')}; "
            f"Auto uses reported default {periods.get('default')}."
        )
        imgui.text_wrapped(f"Client format policy: {info.get('engine_format_policy')}")
        snapshot = info.get("shared_engine_initialization_snapshot")
        if isinstance(snapshot, dict):
            imgui.text_wrapped(
                f"Shared engine snapshot at initialization: {snapshot.get('period_frames')} "
                f"frames / {snapshot.get('sample_rate_hz')} Hz. This is not a live observation."
            )
            if snapshot.get("error"):
                imgui.text_wrapped(str(snapshot["error"]))
    if "buffer_capacity_frames" in info:
        imgui.text_wrapped(
            f"Shared stream capacity: {info['buffer_capacity_frames']} frames; "
            f"reported stream latency: {info.get('reported_stream_latency_100ns')} x 100 ns."
        )
    imgui.text_wrapped(
        "Requested settings, initialized periods, callback sizes and total audible latency "
        "are separate measurements."
    )


def _asio_buffer_pattern(driver: dict[str, object]) -> str:
    granularity = driver.get("granularity")
    if granularity == -1:
        return "minimum multiplied by powers of two, within the reported limits"
    if isinstance(granularity, int) and granularity > 0:
        return f"minimum plus multiples of {granularity}, within the reported limits"
    return "only the coherent preferred size; other fixed sizes are not advertised"


def _asio_observations(asio: dict[str, object], driver: dict[str, object]) -> None:
    imgui.text_wrapped(f"Legal driver buffer pattern: {_asio_buffer_pattern(driver)}.")
    imgui.text_wrapped(
        f"ASIO initialized rate: {asio.get('initialized_sample_rate_hz')} Hz; "
        f"last control query: {asio.get('last_control_query_rate_hz')} Hz. "
        "Queries are not continuous observations."
    )
    if asio.get("info_query_error"):
        imgui.text_wrapped(str(asio["info_query_error"]))
    status = asio.get("status")
    if isinstance(status, dict):
        _asio_status(status)
    probes = asio.get("common_rate_probes")
    if isinstance(probes, list):
        _asio_rate_probes(probes)
    channels = asio.get("channels")
    if isinstance(channels, list):
        _asio_channels(channels)


def _asio_status(status: dict[str, object]) -> None:
    flags = status.get("notifications")
    flags = flags if isinstance(flags, int) else 0
    notices = [
        name
        for bit, name in (
            (1, "reset required"),
            (2, "rate changed"),
            (4, "resync required"),
            (8, "latencies changed"),
            (16, "overload"),
            (32, "clock lost"),
            (64, "same-rate digital input notice"),
        )
        if flags & bit
    ]
    imgui.text_wrapped(f"Driver notices: {', '.join(notices) or 'none'}.")
    if status.get("rate_notice_count"):
        notified = status.get("last_notified_rate") or "unknown"
        imgui.text_wrapped(
            f"Last rate notification: {notified} Hz; count {status['rate_notice_count']}."
        )
    if status.get("callback_error"):
        imgui.text_wrapped(f"Driver callback error: {status['callback_error']}")


def _asio_rate_probes(probes: list[object]) -> None:
    supported = [
        str(row.get("sample_rate_hz"))
        for row in probes
        if isinstance(row, dict) and row.get("supported") is True
    ]
    imgui.text_wrapped(
        f"Supported common-rate probes: {', '.join(supported) or 'none'}. "
        "This list is not exhaustive."
    )


def _asio_channels(channels: list[object]) -> None:
    if not imgui.tree_node("Installed driver's physical outputs"):
        return
    try:
        for channel in channels:
            if isinstance(channel, dict):
                imgui.text_wrapped(_channel_label(channel))
    finally:
        imgui.tree_pop()


def _backend_device_controls(control: AudioOutputController) -> None:
    if imgui.button("Refresh output devices"):
        control.refresh_inventory()
    backends = [("auto", "Auto (system backend)")]
    backends.extend((str(item["id"]), str(item["name"])) for item in control.inventory)
    changed, backend = _choice("Backend", control.draft.backend, backends)
    if changed:
        _replace(
            control,
            backend=backend,
            device_id=None,
            left_channel=None,
            right_channel=None,
            buffer_frames=None,
        )
    selected = next(
        (item for item in control.inventory if item.get("id") == control.draft.backend), None
    )
    devices: list[tuple[str | None, str]] = [(None, "Auto (backend default)")]
    if selected is not None:
        device_rows = selected.get("devices", [])
        if isinstance(device_rows, list):
            devices.extend((device["id"], device["name"]) for device in device_rows)
        if selected.get("discovery_error"):
            imgui.text_wrapped(str(selected["discovery_error"]))
    changed, device_id = _choice("Device / driver", control.draft.device_id, devices)
    if changed:
        _replace(control, device_id=device_id)


def _apply_controls(control: AudioOutputController) -> None:
    imgui.text_wrapped(
        "Stop playback and finish recording, loading and arrangement work before Apply. "
        "Unsupported requests are rejected without rounding or changing backend."
    )
    imgui.text_wrapped(
        "After source startup restoration, existing paired stems require the "
        "installed sample rate explicitly. Auto or another rate is refused "
        "before stopping the current output."
    )
    if imgui.button("Apply audio output"):
        control.apply(control.draft)
    if control.phase == "device_failed" and imgui.button("Retry device close"):
        control.retry_failed_close()


def audio_output_controls(control: AudioOutputController) -> None:
    """Edit one exact request through the stopped output ownership checks."""
    imgui.text_unformatted("Audio output")
    imgui.text_wrapped(f"Status: {control.phase.replace('_', ' ')}")
    if control.error:
        imgui.text_wrapped(control.error)
    imgui.begin_disabled(control.busy)
    try:
        _backend_device_controls(control)
        _routing(control)
        _automatic_number(control, "Sample rate (Hz)", "sample_rate_hz", 48000)
        _automatic_number(control, "Buffer / period (frames)", "buffer_frames", 512)
        _apply_controls(control)
    finally:
        imgui.end_disabled()
    if (
        (control.transaction_active or control.phase in {"startup_restoring", "startup_failed"})
        and not control.exclusive_native_access
        and imgui.button("Cancel output change")
    ):
        control.cancel()
    if control.previous_errors:
        imgui.text_wrapped("Previous source errors, retained from before restoration:")
        for (sample_id, operation), error in control.previous_errors.items():
            imgui.text_wrapped(f"PAD {sample_id + 1} / {operation}: {error}")
    _observations(control)
