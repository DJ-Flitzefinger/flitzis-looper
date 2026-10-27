"""Opt-in timing traces. File work stays on a bounded background writer."""

import hashlib
import importlib
import json
import math
import platform
from pathlib import Path
from queue import Empty, Full, Queue
from threading import Event, Thread
from time import monotonic, time_ns
from typing import TYPE_CHECKING
from uuid import uuid4

from flitzis_looper.timing_policy import effective_runtime_policy

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import AudioEngine

MAX_SECONDS = 600.0
MAX_BYTES = 16 * 1024 * 1024
POLL_SECONDS = 0.2


def _finite(value: object) -> object:
    """JSON has no NaN; missing numeric evidence stays explicitly null."""
    if isinstance(value, float):
        return value if math.isfinite(value) else None
    if isinstance(value, dict):
        return {key: _finite(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_finite(item) for item in value]
    return value


class TimingTrace:
    """Transient observer, with no playback or persistence write authority."""

    def __init__(
        self, audio: AudioEngine, project: ProjectState, session: SessionState, root: Path
    ) -> None:
        self._audio = audio
        self._project = project
        self._session = session
        self._root = root
        self._queue: Queue[dict[str, object]] = Queue(maxsize=64)
        self._stop = Event()
        self._thread: Thread | None = None
        self._started = 0.0
        self._next_poll = 0.0
        self._request: int | None = None
        self._request_started = 0.0
        self._last_state = ""
        self._reason = "stopped"
        self._dropped = 0
        self.path: Path | None = None
        self.error: str | None = None

    @property
    def supported(self) -> bool:
        return getattr(self._audio, "SYNC_DIAGNOSTIC_API_VERSION", 0) == 1

    @property
    def active(self) -> bool:
        return self._thread is not None and self._thread.is_alive() and not self._stop.is_set()

    @property
    def finishing(self) -> bool:
        return self._thread is not None and self._thread.is_alive() and self._stop.is_set()

    @property
    def elapsed(self) -> int:
        return max(0, int(monotonic() - self._started)) if self.active else 0

    def start(self) -> None:
        if self.active or self.finishing:
            return
        if not self.supported:
            self.error = "Timing logging requires the updated Native engine. Restart after update."
            return
        self._queue = Queue(maxsize=64)
        self._stop = Event()
        self._started = monotonic()
        self._next_poll = self._started
        self._request = None
        self._last_state = ""
        self._reason = "stopped"
        self._dropped = 0
        self.error = None
        self.path = self._root / f"sync-{time_ns()}-{uuid4().hex[:8]}.jsonl"
        self._emit(
            "start",
            schema="looper-sync-trace-v1",
            duration_limit_s=MAX_SECONDS,
            byte_limit=MAX_BYTES,
            audio_recorded=False,
            pad_numbering="zero-based",
            evidence_limit="Source/output coordinates are not proof of audible beat alignment.",
        )
        try:
            self._thread = Thread(target=self._write, name="LooperTimingTrace", daemon=True)
            self._thread.start()
        except RuntimeError, OSError:
            self._thread = None
            self.path = None
            self.error = "Timing log could not start its writer. Playback is unchanged."
            self.stop("writer_start_failed")

    def stop(self, reason: str = "user") -> None:
        if not self._stop.is_set():
            self._reason = reason
            self._stop.set()

    def close(self) -> None:
        self.stop("shutdown")
        if self._thread is not None:
            self._thread.join(timeout=0.5)

    def mark_drift(self) -> None:
        if self.active:
            self._emit("heard_drift", pending_request_id=self._request)

    def _emit(self, kind: str, **data: object) -> None:
        event = {
            "event": kind,
            "elapsed_s": monotonic() - self._started,
            "wall_time_ns": time_ns(),
            **data,
        }
        try:
            self._queue.put_nowait(event)
        except Full:
            self._dropped += 1
            self.error = "Timing log stopped: writer queue full. Partial log retained."
            self.stop("queue_full")

    def poll(self) -> None:
        if not self.active:
            return
        now = monotonic()
        if now - self._started >= MAX_SECONDS:
            self.stop("duration_limit")
            return
        if now < self._next_poll:
            return
        self._next_poll = now + POLL_SECONDS
        try:
            state = self._control_state()
            encoded = json.dumps(_finite(state), sort_keys=True, separators=(",", ":"))
            if encoded != self._last_state:
                self._last_state = encoded
                self._emit(
                    "control_state",
                    state=state,
                    observed_at_control_ns=self._audio.capture_input_timestamp_ns(),
                )
        except RuntimeError, ValueError, TypeError, AttributeError, OSError:
            self._emit("gap", reason="control_observation_unavailable")
        self._poll_cohort(now)

    def _poll_cohort(self, now: float) -> None:
        try:
            if self._request is not None:
                snapshot = self._audio.sync_diagnostic_snapshot(self._request)
                if snapshot is not None:
                    self._emit("cohort", snapshot=snapshot)
                    self._request = None
                elif now - self._request_started >= 1.0:
                    self._emit("gap", reason="callback_snapshot_timeout", request_id=self._request)
                    self._request = None
            if self._request is None and self.active:
                self._request = self._audio.request_sync_diagnostic_snapshot()
                self._request_started = now
        except RuntimeError, ValueError, TypeError, AttributeError, OSError:
            self._emit(
                "gap", reason="native_or_control_observation_unavailable", request_id=self._request
            )
            self._request = None

    def _control_state(self) -> dict[str, object]:
        project, session = self._project, self._session
        loaded = [i for i, path in enumerate(project.sample_paths) if path is not None]
        pads = []
        for i in loaded:
            analysis = project.sample_analysis[i]
            sync = project.pad_source_sync_intent[i]
            key = project.pad_key_intent[i]
            pads.append({
                "pad_id": i,
                "manual_bpm": project.manual_bpm[i],
                "analysis_bpm": analysis.bpm if analysis else None,
                "analysis_grid": analysis.beat_grid.model_dump() if analysis else None,
                "timing_intent": project.pad_timing_intent[i],
                "sync_intent": sync.model_dump(mode="json"),
                "sync_requested": project.resolve_pad_source_sync(i),
                "realtime_requested": project.resolve_pad_realtime_bpm(i),
                "loop_start_s": project.pad_loop_start_s[i],
                "loop_end_s": project.pad_loop_end_s[i],
                "loop_auto": project.pad_loop_auto[i],
                "loop_bars": project.pad_loop_bars[i],
                "grid_anchor_s": project.pad_grid_anchor_s[i],
                "grid_offset_samples": project.pad_grid_offset_samples[i],
                "key_lock_override": project.pad_key_lock[i],
                "key_lock_requested": project.resolve_pad_key_lock(i),
                "base_shift": key.base_shift,
                "extra_shift": key.extra_shift,
                "stem_mix_mode": project.pad_stem_mix_mode[i],
                "stem_mask": project.pad_stem_intent[i].enabled_mask,
            })
        return {
            "policy": project.global_timing_policy.model_dump(mode="json"),
            "effective_policy": effective_runtime_policy(project.global_timing_policy).model_dump(
                mode="json"
            ),
            "multi_loop": project.multi_loop,
            "bpm_lock": project.bpm_lock,
            "key_lock": project.key_lock,
            "speed": project.speed,
            "key_lock_start_behavior": project.key_lock_start_behavior,
            "loaded_pad_ids": loaded,
            "active_ui_pad_ids": sorted(session.active_sample_ids),
            "loading_pad_ids": sorted(session.loading_sample_ids),
            "ui_master_period_seconds": session.master_period_seconds,
            "bpm_lock_anchor_pad_id": session.bpm_lock_anchor_pad_id,
            "pads": pads,
        }

    def _write(self) -> None:
        path = self.path
        assert path is not None
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("xb") as stream:
                native_path = Path(
                    importlib.import_module("flitzis_looper_audio.flitzis_looper_audio").__file__
                    or ""
                )
                with native_path.open("rb") as native_stream:
                    digest = hashlib.file_digest(native_stream, "sha256").hexdigest()
                self._emit("build", native_sha256=digest, python=platform.python_version())
                written = 0
                while not self._stop.is_set() or not self._queue.empty():
                    try:
                        event = self._queue.get(timeout=0.1)
                    except Empty:
                        continue
                    encoded = (
                        json.dumps(_finite(event), separators=(",", ":"), allow_nan=False) + "\n"
                    ).encode()
                    if written + len(encoded) > MAX_BYTES - 2048:
                        self._reason = "byte_limit"
                        self._stop.set()
                        break
                    stream.write(encoded)
                    stream.flush()
                    written += len(encoded)
                footer = {
                    "event": "stop",
                    "reason": self._reason,
                    "dropped_events": self._dropped,
                    "pending_request_id": self._request,
                    "elapsed_s": monotonic() - self._started,
                }
                stream.write((json.dumps(footer) + "\n").encode())
                stream.flush()
        except OSError, ValueError, TypeError:
            self.error = "Timing log stopped: file write failed. Any partial log is retained."
            self._reason = "write_error"
            self._stop.set()
