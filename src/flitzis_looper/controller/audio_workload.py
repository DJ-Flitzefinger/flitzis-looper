"""Explicit, finite control-thread observations of the existing output stream."""

import hashlib
import json
import math
import platform
import sys
from collections import deque
from datetime import UTC, datetime
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path
from time import monotonic
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import AudioEngine

_ENVELOPE = [
    "callback_observed_at_ns",
    "output_frame",
    "callback_frames",
    "callback_sample_rate_hz",
    "timestamp_boundary",
    "boundary",
    "fresh",
    "snapshot_age_ns",
]
_EFFECTIVE = [
    "loaded_sample_rate_hz",
    "channels",
    "source_generation",
    "authority_revision",
    "publication_epoch",
    "voice_slot",
    "paused",
    "source_frame",
    "source_fraction",
    "applied_source_rate",
    "target_source_rate",
    "key_lock_requested",
    "key_lock_native_active",
    "native_pitch_scale",
    "stem_all_requested",
    "stem_enabled_mask",
    "prepared_stems_current",
    "stem_transition_active",
    "stem_all_applied",
    "applied_pad_gain_linear",
    "target_pad_gain_linear",
    "master_volume",
    "voice_volume",
    "bpm_lock",
    "master_period_seconds",
    "native_input_fifo_frames",
    "native_output_fifo_frames",
    "native_block_frames",
    "rate_settled",
    "eq_applied_normalized",
    "eq_target_normalized",
]
_DEVICE = [
    "host",
    "endpoint_name",
    "sample_rate_hz",
    "channels",
    "stream_sample_format",
    "default_device_sample_format",
    "requested_buffer_frames",
    "buffer_policy",
]


def _scalars(data: dict[str, object], names: list[str]) -> dict[str, object]:
    """Copy only named scalar values; never include source paths or binding dictionaries."""
    result: dict[str, object] = {}
    for name in names:
        if name not in data:
            continue
        value = data[name]
        if (
            value is None
            or type(value) in {str, int, bool}
            or (type(value) is float and math.isfinite(value))
        ):
            result[name] = value
        elif isinstance(value, list) and all(type(v) is float and math.isfinite(v) for v in value):
            result[name] = list(value)
    return result


def runtime_artifact_identity() -> dict[str, object]:
    """Identify the on-disk loaded extension without claiming full source/build custody."""
    result: dict[str, object] = {
        "python": platform.python_version(),
        "os": platform.system(),
        "architecture": platform.machine(),
        "native_file_sha256": None,
        "full_build_provenance": "unavailable; file hash identifies the extension file only",
    }
    try:
        result["package_version"] = version("flitzis-looper")
    except PackageNotFoundError:
        result["package_version"] = None
    module = sys.modules.get("flitzis_looper_audio.flitzis_looper_audio")
    filename = getattr(module, "__file__", None)
    if filename:
        try:
            with Path(filename).open("rb") as stream:
                content = stream.read(64 * 1024 * 1024 + 1)
            if len(content) <= 64 * 1024 * 1024:
                result["native_file_sha256"] = hashlib.sha256(content).hexdigest()
        except OSError:
            pass
    return result


class AudioWorkloadCapture:
    """Retain one report; poll only while an explicit capture is in progress.

    The ordinary App control tick owns this object. It creates no worker, audio
    device, playback command, source permit or persistent project state.
    """

    def __init__(self, project: ProjectState, session: SessionState, audio: AudioEngine) -> None:
        self._project = project
        self._session = session
        self._audio = audio
        self.active = False
        self.summary = "No audio workload captured."
        self.report_text: str | None = None
        self._remaining: deque[int] = deque()
        self._pending: tuple[int, int, float] | None = None
        self._deadline = 0.0
        self._ticks = 0
        self._report: dict[str, object] = {}
        self._rows: list[dict[str, object]] = []

    def _control_state(self) -> dict[str, object]:
        return {
            "scope": "control intent and feedback; not simultaneous Native state or host CPU load",
            "active_pad_ids": sorted(self._session.active_sample_ids),
            "paused_pad_ids": sorted(self._session.paused_sample_ids),
            "loading_pad_ids": sorted(self._session.loading_sample_ids),
            "analyzing_pad_ids": sorted(self._session.analyzing_sample_ids),
            "stem_generating_pad_ids": sorted(self._session.stem_generating_sample_ids),
            "bpm_lock_requested": self._project.bpm_lock,
            "multi_loop_requested": self._project.multi_loop,
            "key_lock_requested": self._project.key_lock,
            "speed_requested": self._project.speed,
            "master_bpm_control": self._session.master_bpm,
            "external_background_load": "unknown",
        }

    def start(self) -> None:
        """Replace the prior report on an explicit request; reject overlap silently."""
        if self.active:
            return
        self.report_text = None
        self._rows = []
        self._pending = None
        ids = sorted(self._session.active_sample_ids | self._session.paused_sample_ids)
        self._remaining = deque(ids or [self._project.selected_pad])
        self._report = {
            "schema": "audio-workload-v1",
            "started_at_utc": datetime.now(UTC).isoformat(),
            "artifact": runtime_artifact_identity(),
            "control_start": self._control_state(),
            "selection": "active/paused control PADs; selected PAD fallback when empty",
            "pad_ids": list(self._remaining),
            "observations": self._rows,
            "limits": (
                "Serial, nonsimultaneous one-shot observations; no render duration, CPU load, "
                "nonzero PCM, acoustic cause, quality or deadline acceptance. Requested buffer "
                "size is not observed callback size. Unavailable voice values remain unknown."
            ),
        }
        try:
            device = self._audio.output_device_descriptor()
            self._report["output_device"] = None if device is None else _scalars(device, _DEVICE)
        except RuntimeError:
            self._report["output_device"] = None
        self._deadline = monotonic() + 5.0
        self._ticks = 0
        self.active = True
        self.summary = "Capturing audio workload..."

    def _intent(self, pad_id: int) -> dict[str, object]:
        return {
            "key_lock_override": self._project.pad_key_lock[pad_id],
            "stem_mix_mode": self._project.pad_stem_mix_mode[pad_id],
            "stem_enabled_mask": self._project.pad_stem_intent[pad_id].enabled_mask,
            "gain_db": self._project.pad_gain_db[pad_id],
        }

    def poll(self) -> None:
        """Perform at most one Native request or read per ordinary control tick."""
        if not self.active:
            return
        now = monotonic()
        self._ticks += 1
        if now >= self._deadline or self._ticks > 600:
            self.cancel("capture deadline reached")
            return
        if self._pending is None:
            if self._remaining:
                self._request_next(now)
            else:
                self._finish("complete")
            return
        self._read_pending(now)

    def _request_next(self, now: float) -> None:
        pad_id = self._remaining.popleft()
        try:
            request = self._audio.request_loop_acceptance_snapshot(pad_id)
        except RuntimeError, ValueError:
            self._rows.append({"pad_id": pad_id, "status": "request unavailable"})
            return
        self._pending = (pad_id, request, now + 0.25)
        self._rows.append({"pad_id": pad_id, "control_intent_at_request": self._intent(pad_id)})

    def _read_pending(self, now: float) -> None:
        assert self._pending is not None
        pad_id, request, expires = self._pending
        if now >= expires:
            self._rows[-1]["status"] = "timeout; pending or superseded request"
            self._pending = None
            return
        try:
            response = self._audio.loop_acceptance_snapshot(pad_id, request)
        except RuntimeError, ValueError:
            self._rows[-1]["status"] = "read unavailable"
            self._pending = None
            return
        if response is None:
            return
        row = self._rows[-1]
        row.update(_scalars(response, ["status", "reason", "request_id", "current_acknowledged"]))
        if response.get("request_id") == request:
            row["callback"] = _scalars(response, _ENVELOPE)
            if response.get("current_acknowledged") is True and response.get("fresh") is True:
                row["effective_voice"] = _scalars(response, _EFFECTIVE)
        else:
            row["status"] = "unavailable; request identity differs"
            row["current_acknowledged"] = False
        row["control_intent_at_read"] = self._intent(pad_id)
        self._pending = None

    def cancel(self, reason: str = "cancelled") -> None:
        """Stop polling and explicitly retain unattempted/unfinished rows."""
        if not self.active:
            return
        if self._pending is not None:
            self._rows[-1]["status"] = reason
        self._rows.extend(
            {"pad_id": pad_id, "status": f"not attempted: {reason}"} for pad_id in self._remaining
        )
        self._finish(reason)

    def _finish(self, status: str) -> None:
        self.active = False
        self._pending = None
        self._remaining.clear()
        self._report["capture_status"] = status
        self._report["control_end"] = self._control_state()
        self._report["finished_at_utc"] = datetime.now(UTC).isoformat()
        self.report_text = json.dumps(self._report, indent=2, allow_nan=False)
        self.summary = f"Audio workload: {status}; {len(self._rows)} PAD observations."
