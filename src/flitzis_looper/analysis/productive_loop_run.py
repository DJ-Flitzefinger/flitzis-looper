"""Opt-in human-operated app adapter and off-callback evidence collection.

This module is imported only by the explicit packet ``run`` command. It uses the
normal AppController/UI, source-bound acceptance, derived refresh and productive
device stream. It never starts pad playback or controls capture/listening.
"""

import hashlib
import importlib
import json
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
from flitzis_looper.controller.accepted_publication import ExplicitTimingAssessment
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
                report(f"Packet preparation failed: {error}")
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
        if sample_id in session.loading_sample_ids or sample_id in session.analyzing_sample_ids:
            return
        error = session.sample_load_errors.get(sample_id)
        if error is not None:
            raise RuntimeError(error)
        binding = self.controller._audio.current_input_runtime_pad_binding(sample_id)
        if binding is None:
            return
        if binding.metadata().get("source_sha256") != self.plan.source_sha256:
            msg = "loaded source differs from independently verified packet"
            raise ValueError(msg)
        self.preparation = self.controller.accepted_timing.prepare(
            sample_id,
            self.reference.timing_error_halfwidth_seconds,
            self.reference.timing_error_provenance,
            intent="automatic",
        )
        self.stage = "preparing_actual_native_source"

    def _finish_preparation(self, _sample_id: int) -> None:
        if self.preparation is None or not self.preparation.done():
            return
        self.ticket = self.preparation.result()
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
            f"Play and capture manually. Observations: {self.directory}"
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
    plan, reference = load_plan(workspace, plan_path.resolve())
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
