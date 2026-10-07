"""Opt-in human-operated app adapter and off-callback evidence collection.

This module is imported only by the explicit packet ``run`` command. It uses the
normal AppController/UI, source-bound acceptance, derived refresh and productive
device stream. It never starts pad playback or controls capture/listening.
"""

import hashlib
import importlib
import json
import math
import os
import queue
import sys
import threading
import time
import uuid
from concurrent.futures import Future, ThreadPoolExecutor
from datetime import UTC, datetime
from pathlib import Path
from typing import TYPE_CHECKING

from flitzis_looper.analysis.productive_loop_packet import (
    ProductiveRunPlan,
    QuarterReference,
    SnapshotRequest,
    file_sha256,
    load_plan,
    map_raw_quarters,
    write_json,
)
from flitzis_looper.analysis.productive_loop_snapshot import comparison_pad
from flitzis_looper.analysis.reference_inputs_validation import read_json_bytes
from flitzis_looper.controller import AppController
from flitzis_looper.controller.accepted_publication import (
    DEFAULT_CONSTANT_TIMING_PCM_LIMIT_BYTES,
    MAX_CONSTANT_TIMING_PCM_LIMIT_BYTES,
    ExplicitTimingAssessment,
    validate_constant_timing_pcm_limit,
)
from flitzis_looper.ui import run_ui

if TYPE_CHECKING:
    from flitzis_looper_audio import ConstantTimingTicket


def utc_now() -> str:
    """Return an explicit UTC timestamp for evidence association, not audio time."""
    return datetime.now(UTC).isoformat()


def report(message: str) -> None:
    """Make the concrete readiness/artifact path visible to the human operator."""
    sys.stdout.write(message + "\n")
    sys.stdout.flush()


def _native_dimension(metadata: dict[str, object], key: str, maximum: int) -> int:
    value = metadata.get(key)
    if not isinstance(value, int) or isinstance(value, bool) or not 0 < value <= maximum:
        msg = f"invalid actual native {key} for PCM admission"
        raise ValueError(msg)
    return value


def preparation_pcm_budget(
    metadata: dict[str, object], reference: QuarterReference, sample_id: int
) -> dict[str, object]:
    """Derive a finite PCM-only cap from genuine loaded geometry and source extent.

    The source, mono/copy and analyzer terms match native admission. Padding
    bounds two output FFT units for the pinned Rubato 1.0/1024-frame setup;
    native allocation-capacity checks remain authoritative at execution.
    """
    if metadata.get("source_sha256") != reference.source_sha256 or (
        type(metadata.get("pad_id")) is not int or metadata.get("pad_id") != sample_id
    ):
        msg = "actual native source differs from the frozen packet identity"
        raise ValueError(msg)
    source_id = metadata.get("source_id")
    if not isinstance(source_id, str) or not 0 < len(source_id) <= 1024:
        msg = "actual native source identity unavailable for PCM admission"
        raise ValueError(msg)
    _native_dimension(metadata, "source_generation", 2**64 - 1)
    rate = _native_dimension(metadata, "sample_rate_hz", 384_000)
    if rate < 8000:
        msg = "actual native sample rate is outside supported PCM geometry"
        raise ValueError(msg)
    channels = _native_dimension(metadata, "channels", 32)
    frames = _native_dimension(metadata, "frame_count", 2**64 - 1)
    expected_frames = (
        reference.source_frame_count * rate + reference.source_sample_rate_hz - 1
    ) // reference.source_sample_rate_hz
    if frames != expected_frames:
        msg = "actual native extent differs from the frozen complete source"
        raise ValueError(msg)
    converted = (frames * 44_100 + rate - 1) // rate
    common_rate = math.gcd(rate, 44_100)
    reduced_input = rate // common_rate
    fft_output_frames = ((1024 + reduced_input - 1) // reduced_input) * (44_100 // common_rate)
    padding = 0 if rate == 44_100 else 2 * fft_output_frames * 4
    required = frames * channels * 4 + frames * 8 + converted * 12 + padding
    pcm_limit_bytes = validate_constant_timing_pcm_limit(
        max(DEFAULT_CONSTANT_TIMING_PCM_LIMIT_BYTES, 1 << (required - 1).bit_length())
    )
    return {
        "schema_version": 1,
        "policy": "actual-loaded-geometry-pcm-power-of-two-v1",
        "native_source_binding": metadata,
        "frozen_source_sha256": reference.source_sha256,
        "frozen_source_sample_rate_hz": reference.source_sample_rate_hz,
        "frozen_source_frame_count": reference.source_frame_count,
        "loaded_interleaved_f32_bytes": frames * channels * 4,
        "mono_and_transient_copy_bytes": frames * 8,
        "analyzer_frame_count": converted,
        "converted_f32_and_analyzer_f64_bytes": converted * 12,
        "fft_output_unit_frames": fft_output_frames,
        "converter_padding_pcm_bytes": padding,
        "estimated_peak_pcm_bytes": required,
        "requested_pcm_limit_bytes": pcm_limit_bytes,
        "default_pcm_limit_bytes": DEFAULT_CONSTANT_TIMING_PCM_LIMIT_BYTES,
        "hard_max_pcm_limit_bytes": MAX_CONSTANT_TIMING_PCM_LIMIT_BYTES,
        "allocation_scope": "Controlled PCM only; FFT/QM/engine/process RAM is separate",
        "execution_capacity_checks": "actual native allocation capacities remain authoritative",
    }


class ProductiveObserver:
    """Bounded GUI intent handoff and background source/evidence serialization."""

    def __init__(
        self, controller: AppController, plan: ProductiveRunPlan, reference: QuarterReference
    ) -> None:
        self.controller = controller
        self.plan = plan
        self.reference = reference
        self.session_id = uuid.uuid4().hex
        self.directory = Path(plan.project_directory) / "observations" / self.session_id
        self.directory.mkdir()
        self.requests: queue.SimpleQueue[SnapshotRequest] = queue.SimpleQueue()
        self.stop = threading.Event()
        self.worker = ThreadPoolExecutor(max_workers=1, thread_name_prefix="loop-evidence")
        self.watcher = threading.Thread(target=self._watch, name="loop-request", daemon=True)
        self.pad_index = 0
        self.stage = "waiting_for_source"
        self.preparation: Future[ConstantTimingTicket] | None = None
        self.mapping: Future[tuple[dict[str, object], str, dict[str, object]]] | None = None
        self.publication: Future[None] | None = None
        self.ticket: ConstantTimingTicket | None = None
        self.request_metadata: dict[str, object] | None = None
        self.prior_analysis_errors: dict[int, str | None] = {}
        self.completed: set[int] = set()
        self.failure: str | None = None
        self.started_at_utc = utc_now()
        self.native_module = importlib.import_module("flitzis_looper_audio.flitzis_looper_audio")
        module_path = Path(self.native_module.__file__ or "")
        self.native_artifact = {"path": str(module_path), "sha256": file_sha256(module_path)}
        config_bytes = read_json_bytes(Path(plan.project_config_path))
        initial_config_path = self.directory / "initial-project-config.json"
        with initial_config_path.open("xb") as stream:
            stream.write(config_bytes)
        project_snapshot = controller.project.model_dump(mode="json")
        project_snapshot_bytes = json.dumps(
            project_snapshot, sort_keys=True, allow_nan=False
        ).encode("utf-8")
        write_json(
            self.directory / "session.json",
            {
                "session_id": self.session_id,
                "started_at_utc": self.started_at_utc,
                "run_plan": plan.model_dump(mode="json"),
                "native_extension": self.native_artifact,
                "initial_project_config": {
                    "path": str(initial_config_path),
                    "sha256": hashlib.sha256(config_bytes).hexdigest(),
                    "original_path": plan.project_config_path,
                },
                "initial_controller_project_snapshot": project_snapshot,
                "initial_controller_project_snapshot_sha256": hashlib.sha256(
                    project_snapshot_bytes
                ).hexdigest(),
                "device": controller._audio.output_device_descriptor(),
                "automatic_playback": False,
                "device_acceptance": "pending",
                "human_listening": "pending",
            },
        )
        # This is a mutable discovery pointer, never a comparison/evidence input.
        pointer = Path(plan.project_directory) / "latest-session.json"
        pointer.write_text(json.dumps({"directory": str(self.directory)}) + "\n", encoding="utf-8")
        self.watcher.start()

    def _watch(self) -> None:
        seen = set(Path(self.plan.project_directory, "requests").glob("*.json"))
        while not self.stop.wait(0.25):
            for path in Path(self.plan.project_directory, "requests").glob("*.json"):
                if path in seen:
                    continue
                try:
                    data = SnapshotRequest.model_validate_json(read_json_bytes(path), strict=True)
                    self.requests.put(data)
                    seen.add(path)
                except (OSError, ValueError) as error:
                    report(f"Observation request rejected ({path}): {error}")
                    write_json(
                        self.directory / f"{path.stem}-request-rejected.json",
                        {
                            "request_file": str(path),
                            "error": str(error),
                            "device_acceptance": "pending",
                            "human_listening": "pending",
                        },
                    )
                    seen.add(path)

    def close(self) -> None:
        """Retire request/evidence workers before the normal app tears down audio."""
        self.stop.set()
        self.watcher.join(timeout=1.0)
        self.worker.shutdown(wait=True, cancel_futures=True)

    def on_frame(self) -> None:
        """Advance existing source/ACK paths; never initiate playback or recording."""
        try:
            self._advance_preparation()
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            if self.failure is None:
                self.failure = str(error)
                self.stage = "failed"
                self.worker.submit(
                    write_json,
                    self.directory / "setup-failed.json",
                    {
                        "error": self.failure,
                        "device_acceptance": "pending",
                        "human_listening": "pending",
                    },
                )
                report(
                    f"NOT READY: packet preparation failed: {error}. "
                    "Do not start a recording or listening timer. No playback, recording "
                    f"or acceptance occurred automatically. Failure evidence: {self.directory}"
                )
        try:
            request = self.requests.get_nowait()
        except queue.Empty:
            return
        self._begin_observation(request)

    def _advance_preparation(self) -> None:
        if self.stage in {"ready", "failed"}:
            return
        sample_id = self.plan.pad_ids[self.pad_index]
        handlers = {
            "waiting_for_source": self._wait_for_source,
            "preparing_actual_native_source": self._finish_preparation,
            "mapping_independent_quarters": self._publish_mapping,
            "awaiting_actual_publication_ack": self._wait_publication_ack,
            "awaiting_actual_derived_ack": self._wait_derived_ack,
        }
        handlers[self.stage](sample_id)

    def _wait_for_source(self, sample_id: int) -> None:
        session = self.controller.session
        if (
            any(
                pad in session.loading_sample_ids or pad in session.analyzing_sample_ids
                for pad in self.plan.pad_ids
            )
            or self.controller.loader._accepted_restore.has_pending()
        ):
            return
        error = session.sample_load_errors.get(sample_id)
        if error is not None:
            raise RuntimeError(error)
        binding = self.controller._audio.current_input_runtime_pad_binding(sample_id)
        if binding is None:
            return
        pcm_limit_bytes = self._admit_pcm_budget(sample_id, binding.metadata())
        self.preparation = self.controller.accepted_timing.prepare(
            sample_id,
            self.reference.timing_error_halfwidth_seconds,
            self.reference.timing_error_provenance,
            intent="automatic",
            pcm_limit_bytes=pcm_limit_bytes,
        )
        self.stage = "preparing_actual_native_source"

    def _admit_pcm_budget(self, sample_id: int, metadata: dict[str, object]) -> int:
        prior_error = self.controller.session.sample_analysis_errors.get(sample_id)
        receipt = {
            "native_source_binding": metadata,
            "independent_reference_sha256": self.plan.reference_sha256,
            "prior_analysis_error_before_explicit_preparation": prior_error,
        }
        try:
            budget = preparation_pcm_budget(metadata, self.reference, sample_id)
        except ValueError as error:
            self.worker.submit(
                write_json,
                self.directory / f"pad-{sample_id}-pcm-admission-rejected.json",
                {**receipt, "error": str(error)},
            )
            raise
        self.worker.submit(
            write_json,
            self.directory / f"pad-{sample_id}-pcm-admission.json",
            {**receipt, **budget},
        )
        self.prior_analysis_errors[sample_id] = prior_error
        pcm_limit_bytes = budget["requested_pcm_limit_bytes"]
        assert isinstance(pcm_limit_bytes, int)
        return pcm_limit_bytes

    def _finish_preparation(self, sample_id: int) -> None:
        if self.preparation is None or not self.preparation.done():
            return
        self.ticket = self.preparation.result()
        prior_error = self.prior_analysis_errors.pop(sample_id, None)
        errors = self.controller.session.sample_analysis_errors
        if prior_error is not None and errors.get(sample_id) == prior_error:
            errors.pop(sample_id)
        self.mapping = self.worker.submit(self._map_ticket, self.ticket)
        self.stage = "mapping_independent_quarters"

    def _publish_mapping(self, sample_id: int) -> None:
        if self.mapping is None or not self.mapping.done() or self.ticket is None:
            return
        self.request_metadata, hypotheses, _mapping = self.mapping.result()
        self.publication = self.controller.accepted_timing.publish(
            sample_id,
            self.ticket,
            ExplicitTimingAssessment(
                hypotheses_json=hypotheses,
                origin_seconds=self.reference.origin_seconds,
                origin_provenance=self.reference.origin_provenance,
                acceptance_policy_version=self.plan.acceptance_policy_version,
                acceptance_provenance=self.plan.acceptance_provenance,
            ),
        )
        self.stage = "awaiting_actual_publication_ack"

    def _wait_publication_ack(self, sample_id: int) -> None:
        if self.publication is None or not self.publication.done() or self.ticket is None:
            return
        self.publication.result()
        status = self.ticket.publication_status()
        if status == "rejected":
            msg = "actual native timing publication rejected"
            raise RuntimeError(msg)
        current = self.controller._audio.current_constant_timing(sample_id)
        if status != "accepted" or current is None:
            return
        if self.request_metadata is None or current.get("accepted_request_id") != (
            self.request_metadata.get("request_id")
        ):
            msg = "current native timing no longer belongs to this request"
            raise RuntimeError(msg)
        period = current.get("period_seconds_per_quarter")
        if not isinstance(period, float):
            msg = "current native period unavailable"
            raise TypeError(msg)
        self.controller.project.pad_loop_auto[sample_id] = False
        self.controller.project.pad_loop_start_s[sample_id] = 0.0
        self.controller.project.pad_loop_end_s[sample_id] = period * self.plan.logical_loop_beats
        if sample_id == self.plan.pad_ids[0]:
            self.controller.session.bpm_lock_anchor_pad_id = sample_id
        self.controller.accepted_timing.refresh_current(sample_id, on_refreshed=self._derived_ready)
        self.stage = "awaiting_actual_derived_ack"

    def _wait_derived_ack(self, sample_id: int) -> None:
        if sample_id not in self.completed:
            error = self.controller.session.sample_analysis_errors.get(sample_id)
            if error is not None:
                raise RuntimeError(error)
            return
        self.pad_index += 1
        if self.pad_index < len(self.plan.pad_ids):
            self.stage = "waiting_for_source"
            return
        self.stage = "ready"
        self.worker.submit(
            write_json,
            self.directory / "setup-ready.json",
            {
                "completed_actual_current_and_derived_ack_pad_ids": sorted(self.completed),
                "ready_at_utc": utc_now(),
                "automatic_playback": False,
                "device_acceptance": "pending",
                "human_listening": "pending",
            },
        )
        report(
            f"READY: actual current + derived ACK on pads {sorted(self.completed)}. "
            "UI pad #1 is native pad 0. Check 1 measures a manually recorded loopback; "
            "check 2 records >=30 minutes of uninterrupted human listening. Neither check "
            "starts automatically or grants acceptance; do them only when you have time. "
            f"Observations: {self.directory}"
        )

    def _map_ticket(
        self, ticket: ConstantTimingTicket
    ) -> tuple[dict[str, object], str, dict[str, object]]:
        metadata = ticket.metadata()
        path = self.directory / f"pad-{metadata['pad_id']}-actual-preparation.json"
        write_json(path, {"actual_native_preparation": metadata})
        try:
            hypotheses, mapping = map_raw_quarters(metadata, self.reference)
        except (TypeError, ValueError) as error:
            write_json(
                self.directory / f"pad-{metadata['pad_id']}-independent-mapping-failed.json",
                {
                    "actual_native_preparation_path": str(path),
                    "actual_native_preparation_sha256": file_sha256(path),
                    "independent_reference_sha256": self.plan.reference_sha256,
                    "error": str(error),
                    "device_acceptance": "pending",
                    "human_listening": "pending",
                },
            )
            raise
        write_json(
            self.directory / f"pad-{metadata['pad_id']}-independent-mapping.json",
            {
                "actual_native_preparation_path": str(path),
                "actual_native_preparation_sha256": file_sha256(path),
                "independent_reference_sha256": self.plan.reference_sha256,
                "independent_mapping": mapping,
                "explicit_hypotheses": json.loads(hypotheses),
            },
        )
        return metadata, hypotheses, mapping

    def _derived_ready(self, sample_id: int) -> None:
        self.completed.add(sample_id)

    def _begin_observation(self, request: SnapshotRequest) -> None:
        if any(item not in self.plan.pad_ids for item in request.pad_ids):
            self.worker.submit(
                write_json,
                self.directory / f"{request.request_id}-request-rejected.json",
                {
                    "request": request.model_dump(mode="json"),
                    "reason": "requested pad is outside the prepared run plan",
                    "device_acceptance": "pending",
                    "human_listening": "pending",
                },
            )
            return
        # Copy control intent once on its owner thread. This is not native truth;
        # callback snapshots below separately supply applied voice authority.
        state: dict[str, object] = {
            "project_intent": self.controller.project.model_dump(mode="json"),
            "session_projection": self.controller.session.model_dump(mode="json"),
            "observer_setup_stage": self.stage,
        }
        source_paths = {
            sample_id: (
                str(Path(path).resolve())
                if (path := self.controller.project.sample_paths[sample_id]) is not None
                else None
            )
            for sample_id in request.pad_ids
        }
        self.worker.submit(self._write_observation, request, state, source_paths)

    def _capture_pad(
        self, sample_id: int, source_path: str | None
    ) -> tuple[dict[str, object], dict[str, object] | None]:
        audio = self.controller._audio
        native_request = audio.request_loop_acceptance_snapshot(sample_id)
        deadline = time.monotonic() + 5.0
        native = audio.loop_acceptance_snapshot(sample_id, native_request)
        while native is None and not self.stop.is_set() and time.monotonic() < deadline:
            self.stop.wait(0.01)
            native = audio.loop_acceptance_snapshot(sample_id, native_request)
        current_before = audio.current_constant_timing(sample_id)
        binding = audio.current_input_runtime_pad_binding(sample_id)
        exported = (
            audio.export_current_constant_timing(sample_id, source_path)
            if source_path is not None and current_before is not None
            else None
        )
        current_after = audio.current_constant_timing(sample_id)
        record: dict[str, object] = {
            "pad_id": sample_id,
            "native_callback_snapshot": native,
            "current_constant_timing_before_export": current_before,
            "current_constant_timing_after_export": current_after,
            "current_source_binding": binding.metadata() if binding is not None else None,
            "verified_current_timing_export": json.loads(exported) if exported else None,
            "actual_source_path": source_path,
        }
        return record, comparison_pad(native, current_before, current_after, exported)

    def _write_observation(
        self,
        request: SnapshotRequest,
        state: dict[str, object],
        source_paths: dict[int, str | None],
    ) -> None:
        audio = self.controller._audio
        pads: list[dict[str, object]] = []
        comparison_pads: list[dict[str, object]] = []
        reasons: list[str] = []
        try:
            for sample_id in request.pad_ids:
                record, comparison = self._capture_pad(sample_id, source_paths[sample_id])
                pads.append(record)
                if comparison is None:
                    reasons.append(f"pad_{sample_id}_current_steady_applied_voice_unavailable")
                else:
                    comparison_pads.append(comparison)
            output_rate = audio.output_sample_rate()
            data: dict[str, object] = {
                "schema_version": 1,
                "evidence_kind": "real_productive_app_observation",
                "session_id": self.session_id,
                "observed_at_utc": utc_now(),
                "request": request.model_dump(mode="json"),
                "native_extension": self.native_artifact,
                "run_plan": self.plan.model_dump(mode="json"),
                "output_device": audio.output_device_descriptor(),
                "output_clock_snapshot": audio.output_clock_snapshot(),
                "native_observation_time_contract": (
                    "Each pad is requested and observed separately. output_frame is the next "
                    "boundary after that callback; callback_observed_at_ns names its start "
                    "and callback_frames its extent. Pads and clock getter are not simultaneous."
                ),
                "pads": pads,
                "control_state": state,
                "comparison_blockers": reasons,
                "device_acceptance": "pending",
                "sustained_human_listening": "pending",
            }
            if not reasons:
                data["capture_comparison"] = {
                    "output": {"sample_rate_hz": output_rate, "clock_identity": self.session_id},
                    "pads": comparison_pads,
                }
            path = self.directory / f"{request.label}-{request.request_id}-productive-run.json"
            write_json(path, data)
            report(f"Observed immutable productive state: {path}")
        except (OSError, RuntimeError, ValueError) as error:
            path = self.directory / f"{request.request_id}-observation-failed.json"
            write_json(
                path,
                {
                    "request": request.model_dump(mode="json"),
                    "error": str(error),
                    "acceptance": "pending",
                },
            )
            report(f"Observation failed: {error}")


def run_packet(workspace: Path, plan_path: Path) -> None:
    """Start only when the human explicitly runs this opt-in CLI command."""
    plan, reference = load_plan(workspace, plan_path)
    original_directory = Path.cwd()
    os.chdir(plan.project_directory)
    controller: AppController | None = None
    observer: ProductiveObserver | None = None
    exit_called = False

    def close_observer() -> None:
        nonlocal exit_called
        if observer is not None:
            observer.close()
        exit_called = True

    try:
        report(
            "Preparing accepted timing for an isolated test project; this is not a recording "
            "or listening test. Wait for READY before starting any recording or timer. "
            "UI pad #1 corresponds to native pad 0. Nothing plays or records automatically; "
            "you may close the app without doing a human test."
        )
        controller = AppController(project_config_path=Path(plan.project_config_path))
        observer = ProductiveObserver(controller, plan, reference)
        run_ui(controller, on_frame=observer.on_frame, on_exit=close_observer)
    finally:
        if not exit_called:
            if observer is not None:
                observer.close()
            if controller is not None:
                controller.shut_down()
        os.chdir(original_directory)
