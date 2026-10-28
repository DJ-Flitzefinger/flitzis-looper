"""Ordinary scalar BPM-Lock fixture receipts; no playback-map or policy path."""

from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController

BPM_REFERENCE_SOURCE_IDENTITY = "b" * 64


def _master_provenance(
    source_identity: str | None = BPM_REFERENCE_SOURCE_IDENTITY,
    accepted_revision: str | None = None,
    *,
    kind: str = "qualified",
) -> dict[str, object]:
    return {
        "kind": kind,
        "source_identity": source_identity,
        "accepted_revision": accepted_revision,
    }


def _bind_fixture_source_identity(audio: Mock, *, pad_id: int) -> None:
    """Give only one locally captured original source an explicit correlation value."""
    original_provider = audio.current_input_runtime_pad_binding.side_effect
    original_source: tuple[object, ...] | None = None

    def current_binding(sample_id: int) -> object:
        nonlocal original_source
        binding = (
            original_provider(sample_id)
            if callable(original_provider)
            else audio.current_input_runtime_pad_binding.return_value
        )
        if isinstance(binding, FakeInputRuntimePadBinding):
            metadata = binding.metadata()
            source = tuple(
                metadata.get(key)
                for key in (
                    "source_id",
                    "source_generation",
                    "source_sha256",
                    "sample_rate_hz",
                    "frame_count",
                    "channels",
                    "content_lifetime",
                )
            )
            if original_source is None and sample_id == pad_id:
                original_source = source
            if source == original_source:
                binding._metadata["source_identity"] = BPM_REFERENCE_SOURCE_IDENTITY
        return binding

    audio.current_input_runtime_pad_binding.side_effect = current_binding


def _configured(controller: AppController, audio: Mock, *, pad_id: int = 0) -> dict[str, object]:
    project = controller.project
    project.multi_loop = True
    project.sample_paths[pad_id] = project.sample_paths[pad_id] or "samples/hierarchy-reference.wav"
    project.sample_durations[pad_id] = project.sample_durations[pad_id] or 10.0
    project.manual_bpm[pad_id] = project.manual_bpm[pad_id] or 120.0
    project.pad_content[pad_id] = project.pad_content[pad_id] or PadContentIdentity(
        instance_id="a" * 32
    )
    project.selected_pad = pad_id
    receipt: dict[str, object] = {
        "request_revision": 0,
        "effect_revision": 0,
        "state": "applied",
        "requested_enabled": False,
        "effective_enabled": False,
        "reason": None,
        "execution_frame": None,
        "submission_available": True,
    }
    audio.bpm_lock_status = Mock(side_effect=receipt.copy)
    _bind_fixture_source_identity(audio, pad_id=pad_id)
    initial_speed = project.speed
    initial_period = 60.0 / (project.manual_bpm[pad_id] or 120.0) / initial_speed
    actual_provenance = _master_provenance(None, kind="absent")
    submitted_provenance: dict[str, object] | None = None
    submitted_revision: int | None = None

    def observe_rate() -> dict[str, object]:
        nonlocal actual_provenance
        if receipt["state"] == "applied" and receipt["effect_revision"] == submitted_revision:
            actual_provenance = (
                dict(submitted_provenance)
                if receipt["effective_enabled"] and submitted_provenance is not None
                else _master_provenance(None, kind="absent")
            )
        return {
            "request_revision": 0,
            "effect_revision": 0,
            "state": "applied",
            "requested_speed": project.speed,
            "requested_master_period": None,
            "effective_speed": initial_speed,
            "effective_master_period": (initial_period if receipt["effective_enabled"] else None),
            "execution_frame": 0,
            "error": None,
            "master_provenance": dict(actual_provenance),
        }

    audio.global_rate_status = Mock(side_effect=observe_rate)

    def submit(*, enabled: bool, anchor_binding: object = None) -> None:
        nonlocal submitted_provenance, submitted_revision
        if enabled:
            assert isinstance(anchor_binding, FakeInputRuntimePadBinding)
            metadata = anchor_binding.metadata()
            accepted = metadata["accepted_timing"]
            submitted_provenance = _master_provenance(
                cast("str", metadata["source_identity"]),
                cast("str", accepted["revision"]) if isinstance(accepted, dict) else None,
            )
        receipt.update(
            request_revision=cast("int", receipt["request_revision"]) + 1,
            state="queued",
            requested_enabled=enabled,
            reason=None,
            execution_frame=None,
        )
        submitted_revision = cast("int", receipt["request_revision"])

    audio.set_bpm_lock = Mock(side_effect=submit)
    return receipt


def _acknowledged_bpm_reference(controller: AppController) -> None:
    """Fixture-only coherent Native effect; real admission/audio has separate regressions."""
    parameters = controller.transport.global_params
    parameters._begin_bpm_lock_application(capture_anchor=False)
    qualified = parameters._qualified_bpm_anchor()
    assert qualified is not None
    application = parameters._bpm_application
    assert application is not None
    application.fingerprint = qualified[1]
    application.admitted_revision = 1
    application.master_projected = True
    parameters._bpm_feedback = {
        "request_revision": 1,
        "effect_revision": 1,
        "state": "applied",
        "requested_enabled": True,
        "effective_enabled": True,
        "reason": None,
        "execution_frame": 64,
        "submission_available": True,
    }


def _rate_image(
    audio: Mock,
    *,
    state: str,
    revision: int = 2,
    requested_period: float = 1.0 / 3.0,
    accepted_revision: str | None = None,
    source_identity: str | None = BPM_REFERENCE_SOURCE_IDENTITY,
    provenance_kind: str = "qualified",
) -> dict[str, object]:
    feedback: dict[str, object] = {
        "request_revision": revision,
        "effect_revision": 0 if state == "queued" else revision,
        "state": state,
        "requested_speed": 1.5,
        "requested_master_period": requested_period,
        "effective_speed": 1.0,
        "effective_master_period": 0.5,
        "execution_frame": None if state == "queued" else 128,
        "error": "Source history is unavailable." if state == "rejected" else None,
        "master_provenance": _master_provenance(
            source_identity, accepted_revision, kind=provenance_kind
        ),
    }
    audio.global_rate_status.side_effect = None
    audio.global_rate_status.return_value = feedback
    return feedback
