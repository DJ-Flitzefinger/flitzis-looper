"""Hardware-free packet/source mapping and synthetic native-observation guards."""

import json
import queue
import struct
import threading
import time
from concurrent.futures import Future, ThreadPoolExecutor
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.analysis.productive_loop_packet import (
    ProductiveRunPlan,
    QuarterReference,
    SnapshotRequest,
    file_sha256,
    load_plan,
    map_raw_quarters,
    prepare_packet,
    request_snapshot,
)
from flitzis_looper.analysis.productive_loop_packet import (
    main as packet_main,
)
from flitzis_looper.analysis.productive_loop_run import (
    ProductiveObserver,
    preparation_pcm_budget,
    run_packet,
)
from flitzis_looper.analysis.productive_loop_snapshot import comparison_pad
from flitzis_looper.controller import accepted_restore
from flitzis_looper.models import ProjectState
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding, current_timing_metadata

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import ConstantTimingTicket


def reference(path: Path) -> QuarterReference:
    return QuarterReference(
        source_sha256=file_sha256(path),
        source_sample_rate_hz=48_000,
        source_frame_count=48_000 * 3,
        feature_source_frames=[0, 24_000, 48_000, 72_000, 96_000],
        quarter_counts=[0, 1, 2, 3, 4],
        quarter_note_denominator=1,
        provenance="independently authored synthetic test-quarter assertion",
        reference_kind="authored_quarter_pulse_fixture",
        origin_seconds=0.0,
        origin_provenance="authored fixture frame zero",
        raw_match_halfwidth_seconds=0.05,
        timing_error_halfwidth_seconds=0.05,
        timing_error_provenance="engineering raw-to-authored feature bound",
    )


def metadata(ref: QuarterReference, *, rate: int = 48_000) -> dict[str, object]:
    return {
        "source_sha256": ref.source_sha256,
        "sample_rate_hz": rate,
        "frame_count": rate * 3,
        "beat_seconds": [0.503, 1.004, 2.005],
    }


def budget_binding(ref: QuarterReference, *, rate: int = 48_000) -> dict[str, object]:
    return {
        "pad_id": 0,
        "source_id": "synthetic-loaded-source",
        "source_generation": 7,
        "source_sha256": ref.source_sha256,
        "sample_rate_hz": rate,
        "channels": 2,
        "frame_count": (ref.source_frame_count * rate + ref.source_sample_rate_hz - 1)
        // ref.source_sample_rate_hz,
    }


@pytest.mark.parametrize("rate", [44_100, 48_000, 96_000])
def test_actual_geometry_uses_loaded_frames_and_keeps_normal_default(
    tmp_path: Path, rate: int
) -> None:
    source = tmp_path / "synthetic-source"
    source.write_bytes(b"synthetic independent geometry reference")
    ref = reference(source)
    binding = budget_binding(ref, rate=rate)
    receipt = preparation_pcm_budget(binding, ref, 0)
    assert receipt["native_source_binding"] == binding
    assert receipt["requested_pcm_limit_bytes"] == 512 * 1024 * 1024
    assert receipt["analyzer_frame_count"] == 3 * 44_100


def test_complete_600_second_stereo_geometry_admits_bounded_explicit_policy(tmp_path: Path) -> None:
    source = tmp_path / "synthetic-source"
    source.write_bytes(b"synthetic geometry; not a device or decoded audio fixture")
    ref = reference(source)
    ref.source_frame_count = 600 * 48_000
    receipt = preparation_pcm_budget(budget_binding(ref), ref, 0)
    assert receipt["estimated_peak_pcm_bytes"] == 778_320_000 + 2 * 1029 * 4
    assert receipt["requested_pcm_limit_bytes"] == 1024 * 1024 * 1024
    assert receipt["loaded_interleaved_f32_bytes"] == 28_800_000 * 2 * 4
    assert receipt["converter_padding_pcm_bytes"] == 8232
    with pytest.raises(ValueError, match="at most 1 GiB"):
        preparation_pcm_budget(budget_binding(ref, rate=96_000), ref, 0)


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("pad_id", False),
        ("source_generation", True),
        ("source_generation", 0),
        ("source_id", ""),
        ("source_sha256", "f" * 64),
        ("sample_rate_hz", True),
        ("sample_rate_hz", 7999),
        ("sample_rate_hz", 384_001),
        ("channels", 0),
        ("channels", 33),
        ("frame_count", 144_001),
        ("frame_count", 144_000.0),
    ],
)
def test_pcm_admission_rejects_unbound_or_invalid_actual_geometry(
    tmp_path: Path, field: str, value: object
) -> None:
    source = tmp_path / "synthetic-source"
    source.write_bytes(b"synthetic independent geometry reference")
    ref = reference(source)
    binding = budget_binding(ref)
    binding[field] = value
    with pytest.raises(ValueError, match=r"actual native|PCM admission"):
        preparation_pcm_budget(binding, ref, 0)


def native_pair(pad: int = 0) -> tuple[dict[str, object], dict[str, object], str]:
    current = current_timing_metadata(
        sample_id=pad,
        period=0.500000000123,
        revision="accepted-constant-timing-v1:" + "d" * 64,
    )
    binding = FakeInputRuntimePadBinding(
        pad, accepted_timing=current, intent="automatic"
    ).metadata()
    native: dict[str, object] = {
        "pad_id": pad,
        "current_acknowledged": True,
        "fresh": True,
        "status": "available",
        "current_binding": binding,
        "paused": False,
        "rate_settled": True,
        "source_seek_mode": 0,
        "stem_transition_active": False,
        "loaded_sample_rate_hz": 48_000,
        "musical_loop_period_frames": 1500.25,
        "physical_loop_start_frame": 19,
        "physical_loop_end_frame": 1519,
        "applied_source_rate": 0.73,
        "target_source_rate": 0.73,
        "applied_pad_gain_linear": 1.0,
        "target_pad_gain_linear": 1.0,
        "eq_applied_normalized": [0.5, 0.5, 0.5],
        "eq_target_normalized": [0.5, 0.5, 0.5],
        "stem_all_requested": False,
        "stem_all_applied": False,
        "key_lock_requested": False,
        "key_lock_native_active": False,
        "source_generation": current["source_generation"],
        "publication_epoch": current["publication_epoch"],
        "authority_revision": 1,
        "effective_accepted_revision": current["revision"],
        "effective_period_seconds_per_quarter": current["period_seconds_per_quarter"],
        "effective_origin_seconds": current["origin_seconds"],
    }

    def bits(value: object) -> str:
        assert isinstance(value, float)
        return struct.pack("!d", value).hex()

    exported = json.dumps({
        "schema_version": 1,
        "encoding": "accepted-constant-timing-qm-raw-v1",
        "record": {
            "accepted_revision": current["revision"],
            "period_bits": bits(current["period_seconds_per_quarter"]),
            "origin": {
                "seconds_bits": bits(current["origin_seconds"]),
                "provenance": current["origin_provenance"],
            },
            "decision": {
                "policy_version": current["acceptance_policy_version"],
                "provenance": current["acceptance_provenance"],
            },
            "evidence": {
                "binding": {
                    **{
                        key: current[key]
                        for key in (
                            "source_sha256",
                            "source_provenance",
                            "pcm_sha256",
                            "sample_rate_hz",
                            "frame_count",
                            "mono_revision",
                        )
                    },
                    "source_zero_bits": bits(current["source_zero_seconds"]),
                    "job": {
                        "pad_id": pad,
                        "request_id": current["accepted_request_id"],
                        "source_id": current["source_id"],
                        "source_generation": current["source_generation"],
                    },
                }
            },
        },
    })
    return native, current, exported


def test_private_packet_creates_isolated_project_and_preserves_original(tmp_path: Path) -> None:
    source = tmp_path / "source.wav"
    source.write_bytes(b"synthetic source bytes for hardware-free preparation")
    original = file_sha256(source)
    output = tmp_path / "exports/device-run"
    path = prepare_packet(tmp_path, source, output, reference=reference(source), pads=6)
    plan, ref = load_plan(tmp_path, path)
    project = ProjectState.model_validate_json(Path(plan.project_config_path).read_bytes())
    assert plan.app_started is False
    assert plan.device_acceptance == "pending"
    assert plan.pad_ids == list(range(6))
    assert project.speed == 0.73
    assert project.bpm_lock
    assert project.sample_paths[:6] == ["samples/acceptance-source.wav"] * 6
    assert all(project.sample_analysis[pad] is None for pad in range(6))
    assert file_sha256(source) == original == ref.source_sha256
    assert file_sha256(Path(plan.source_path)) == original
    with pytest.raises(ValueError, match="already exists"):
        prepare_packet(tmp_path, source, output, reference=ref)


@pytest.mark.parametrize("location", ["repo/evidence", "../outside-evidence"])
def test_packet_rejects_repository_and_external_artifacts(tmp_path: Path, location: str) -> None:
    source = tmp_path / "source.wav"
    source.write_bytes(b"fixture")
    with pytest.raises(ValueError, match="outside_workspace_or_inside_repo"):
        prepare_packet(tmp_path, source, Path(location), reference=reference(source))


@pytest.mark.parametrize("item", ["source_path", "reference_path"])
def test_frozen_packet_rejects_changed_inputs(tmp_path: Path, item: str) -> None:
    source = tmp_path / "source.wav"
    source.write_bytes(b"fixture")
    path = prepare_packet(tmp_path, source, tmp_path / "exports/run", reference=reference(source))
    plan, _ref = load_plan(tmp_path, path)
    Path(getattr(plan, item)).write_bytes(b"changed")
    with pytest.raises(ValueError, match="changed after preparation"):
        load_plan(tmp_path, path)


def test_mapping_missing_raw_events_preserves_independent_counts(tmp_path: Path) -> None:
    source = tmp_path / "source.wav"
    source.write_bytes(b"fixture")
    ref = reference(source)
    hypotheses, proof = map_raw_quarters(metadata(ref), ref)
    assert json.loads(hypotheses)[0]["quarter_counts"] == [1, 2, 4]
    assert proof["independent_landmark_indices"] == [1, 2, 4]
    assert proof["raw_indices"] == [0, 1, 2]
    assert proof["candidate_informed_musical_labels"] is False


@pytest.mark.parametrize("rate", [44_100, 48_000, 96_000])
def test_mapping_uses_source_seconds_across_loaded_rates(tmp_path: Path, rate: int) -> None:
    source = tmp_path / "source.wav"
    source.write_bytes(b"fixture")
    ref = reference(source)
    hypotheses, _proof = map_raw_quarters(metadata(ref, rate=rate), ref)
    assert json.loads(hypotheses)[0]["quarter_counts"] == [1, 2, 4]


@pytest.mark.parametrize("beats", [[0.503, 0.51, 2.0], [0.2, 1.0, 2.0], [0.5, 1.0, 4.0]])
def test_unmatched_or_duplicate_raw_events_fail(tmp_path: Path, beats: list[float]) -> None:
    source = tmp_path / "source.wav"
    source.write_bytes(b"fixture")
    ref = reference(source)
    with pytest.raises(ValueError, match="unique unused independent"):
        map_raw_quarters(dict(metadata(ref), beat_seconds=beats), ref)


def test_snapshot_request_publishes_complete_json_atomically(tmp_path: Path) -> None:
    source = tmp_path / "source.wav"
    source.write_bytes(b"fixture")
    plan = prepare_packet(tmp_path, source, tmp_path / "exports/run", reference=reference(source))
    path = request_snapshot(tmp_path, plan, "dry-start", [0, 1, 2, 3, 4, 5])
    request = SnapshotRequest.model_validate_json(path.read_bytes())
    assert request.operation == "observe_only"
    assert request.pad_ids == list(range(6))
    assert not list(path.parent.glob("*.tmp"))
    with pytest.raises(ValueError, match="subset"):
        request_snapshot(tmp_path, plan, "wrong-pad", [7])


def test_comparison_projects_actual_native_aliases_and_full_revision() -> None:
    native, current, exported = native_pair()
    comparison = comparison_pad(native, current, current, exported)
    assert comparison is not None
    assert comparison["musical_period_loaded_frames"] == 1500.25
    assert comparison["physical_start_loaded_frame"] == 19
    assert comparison["physical_end_loaded_frame"] == 1519
    assert comparison["current_accepted_revision"] == current["revision"]


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("current_acknowledged", False),
        ("fresh", False),
        ("paused", True),
        ("rate_settled", False),
        ("stem_transition_active", True),
        ("source_seek_mode", 1),
        ("applied_source_rate", 0.729),
        ("applied_pad_gain_linear", 0.99),
        ("eq_applied_normalized", [0.5, 0.6, 0.5]),
        ("stem_all_requested", True),
        ("key_lock_requested", True),
        ("musical_loop_period_frames", None),
        ("source_generation", 99),
        ("effective_accepted_revision", "historical-revision"),
        ("publication_epoch", 999),
    ],
)
def test_transient_or_noncurrent_voice_cannot_supply_capture_authority(
    field: str, value: object
) -> None:
    native, current, exported = native_pair()
    native[field] = value
    assert comparison_pad(native, current, current, exported) is None


def test_export_source_and_exact_scalar_bits_must_match_current() -> None:
    native, current, exported = native_pair()
    assert comparison_pad(native, current, dict(current, revision="replacement"), exported) is None
    value = json.loads(exported)
    value["record"]["evidence"]["binding"]["source_sha256"] = "c" * 64
    assert comparison_pad(native, current, current, json.dumps(value)) is None
    value = json.loads(exported)
    value["record"]["origin"]["seconds_bits"] = struct.pack("!d", 0.125).hex()
    assert comparison_pad(native, current, current, json.dumps(value)) is None
    assert comparison_pad(native, current, current, "{}") is None


def test_six_pad_requests_are_sequential_on_native_single_slot(tmp_path: Path) -> None:
    observer = ProductiveObserver.__new__(ProductiveObserver)
    observer.controller = Mock()
    observer.stop = threading.Event()
    observer.directory = tmp_path
    observer.session_id = "synthetic-session"
    observer.native_artifact = {"path": "synthetic-extension", "sha256": "a" * 64}
    observer.plan = Mock(model_dump=Mock(return_value={"synthetic_fixture": True}))
    audio = observer.controller._audio
    current_demand = [-1]
    requests: list[int] = []

    def demand(pad: int) -> int:
        current_demand[0] = pad
        requests.append(pad)
        return pad + 1

    def retrieve(pad: int, request: int) -> dict[str, object]:
        assert current_demand[0] == pad
        assert request == pad + 1
        return native_pair(pad)[0]

    audio.request_loop_acceptance_snapshot.side_effect = demand
    audio.loop_acceptance_snapshot.side_effect = retrieve
    audio.current_constant_timing.side_effect = lambda pad: native_pair(pad)[1]
    audio.current_input_runtime_pad_binding.side_effect = lambda pad: FakeInputRuntimePadBinding(
        pad, accepted_timing=native_pair(pad)[1], intent="automatic"
    )
    audio.export_current_constant_timing.side_effect = lambda pad, _path: native_pair(pad)[2]
    audio.output_sample_rate.return_value = 48_000
    audio.output_device_descriptor.return_value = {"evidence_kind": "synthetic_fixture"}
    audio.output_clock_snapshot.return_value = {"evidence_kind": "synthetic_fixture"}
    request = SnapshotRequest(
        request_id="a" * 32,
        label="six-pad-synthetic",
        pad_ids=list(range(6)),
        requested_at_utc="2026-10-07T00:00:00Z",
    )
    observer._write_observation(
        request, {"synthetic_fixture": True}, dict.fromkeys(range(6), "source")
    )
    assert requests == list(range(6))
    artifact = json.loads(next(tmp_path.glob("*-productive-run.json")).read_bytes())
    assert len(artifact["capture_comparison"]["pads"]) == 6
    assert artifact["comparison_blockers"] == []
    audio.play_sample.assert_not_called()
    audio.run.assert_not_called()


def test_publication_history_without_current_ack_never_advances() -> None:
    observer = ProductiveObserver.__new__(ProductiveObserver)
    observer.controller = Mock()
    observer.ticket = Mock(publication_status=Mock(return_value="accepted"))
    observer.publication = Future()
    observer.publication.set_result(None)
    observer.request_metadata = {"request_id": 9}
    observer.stage = "awaiting_actual_publication_ack"
    observer.controller._audio.current_constant_timing.return_value = None
    observer._wait_publication_ack(0)
    assert observer.stage == "awaiting_actual_publication_ack"
    observer.controller.accepted_timing.refresh_current.assert_not_called()


def blank_observer(
    controller: AppController, plan: ProductiveRunPlan, ref: QuarterReference, directory: Path
) -> ProductiveObserver:
    observer = ProductiveObserver.__new__(ProductiveObserver)
    observer.controller, observer.plan, observer.reference = controller, plan, ref
    observer.directory = directory
    observer.directory.mkdir()
    observer.pad_index = 0
    observer.stage = "waiting_for_source"
    observer.preparation = observer.mapping = observer.publication = observer.ticket = None
    observer.request_metadata = None
    observer.prior_analysis_errors = {}
    observer.completed = set()
    observer.failure = None
    observer.requests = queue.SimpleQueue()
    observer.worker = ThreadPoolExecutor(max_workers=1)
    return observer


def advance_observer_until(observer: ProductiveObserver, stage: str) -> None:
    deadline = time.monotonic() + 2.0
    while observer.stage != stage and time.monotonic() < deadline:
        observer.controller.poll_runtime_events()
        observer.on_frame()
        time.sleep(0.001)
    assert observer.stage == stage
    assert observer.failure is None


@pytest.fixture
def budgeted_observer(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> Iterator[ProductiveObserver]:
    source = tmp_path / "source.wav"
    source.write_bytes(b"synthetic source for controller-only restored-session regression")
    ref = reference(source)
    ref.source_frame_count = 600 * 48_000
    plan_path = prepare_packet(tmp_path, source, tmp_path / "exports/run", reference=ref, pads=1)
    plan, ref = load_plan(tmp_path, plan_path)
    current = current_timing_metadata()
    current["source_sha256"] = ref.source_sha256
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = FakeInputRuntimePadBinding(
        accepted_timing=current, intent="automatic"
    )
    audio_engine_mock.current_constant_timing.return_value = None
    audio_engine_mock.poll_loader_events.return_value = None
    audio_engine_mock.pad_timing_intent.return_value = "automatic"
    audio_engine_mock.prepare_captured_constant_timing.return_value = Mock()
    controller.project.sample_paths[0] = str(source)
    controller.project.pad_timing_intent[0] = "automatic"
    controller.project.speed = 0.73
    observer = blank_observer(controller, plan, ref, tmp_path / "observations")
    try:
        yield observer
    finally:
        observer.worker.shutdown(wait=True)
        controller.accepted_timing.shut_down()


def test_reopen_waits_for_other_restore_then_fresh_success_reaches_real_ack_ready(
    budgeted_observer: ProductiveObserver, audio_engine_mock: Mock
) -> None:
    observer, controller = budgeted_observer, budgeted_observer.controller
    restorer = controller.loader._accepted_restore
    historical: Future[ConstantTimingTicket] = Future()
    controller.project.sample_paths[4] = "samples/historical-source.wav"
    restorer._pending[4] = accepted_restore._Restore(historical, "samples/historical-source.wav")
    observer._wait_for_source(0)
    assert restorer.has_pending()
    assert observer.preparation is None
    audio_engine_mock.capture_current_constant_timing.assert_not_called()
    assert controller.accepted_timing._worker is None
    historical.set_exception(ValueError("historical default 512 MiB restore failed"))
    restorer.poll()
    assert not restorer.has_pending()
    controller.session.sample_analysis_errors[0] = "earlier selected-pad restore failed"
    current = current_timing_metadata()
    current["source_sha256"] = observer.reference.source_sha256
    ticket = audio_engine_mock.prepare_captured_constant_timing.return_value
    ticket.metadata.return_value = dict(
        current, request_id=current["accepted_request_id"], beat_seconds=[0.503, 1.004, 2.005]
    )
    ticket.publication_status.return_value = "pending"
    refresh = audio_engine_mock.refresh_current_constant_timing.return_value
    refresh.publication_status.return_value = "pending"
    refresh.is_current.return_value = False
    advance_observer_until(observer, "awaiting_actual_publication_ack")
    assert 0 not in controller.session.sample_analysis_errors
    assert (
        controller.session.sample_analysis_errors[4] == "historical default 512 MiB restore failed"
    )
    assert audio_engine_mock.capture_current_constant_timing.call_args.kwargs == {
        "pcm_limit_bytes": 1024 * 1024 * 1024
    }
    ticket.publication_status.return_value = "accepted"
    audio_engine_mock.current_constant_timing.return_value = current
    advance_observer_until(observer, "awaiting_actual_derived_ack")
    refresh.publication_status.return_value = "accepted"
    refresh.is_current.return_value = True
    advance_observer_until(observer, "ready")
    observer.worker.submit(lambda: None).result(timeout=2.0)
    receipt = json.loads((observer.directory / "pad-0-pcm-admission.json").read_text())
    assert receipt["prior_analysis_error_before_explicit_preparation"] == (
        "earlier selected-pad restore failed"
    )
    assert observer.completed == {0}
    audio_engine_mock.play_sample.assert_not_called()


@pytest.mark.parametrize("analyzing", [False, True])
def test_fresh_capture_waits_for_other_planned_pad_startup_work(
    budgeted_observer: ProductiveObserver, audio_engine_mock: Mock, *, analyzing: bool
) -> None:
    observer = budgeted_observer
    observer.plan.pad_ids = [0, 4]
    session = observer.controller.session
    pending = session.analyzing_sample_ids if analyzing else session.loading_sample_ids
    pending.add(4)
    observer._wait_for_source(0)
    assert observer.preparation is None
    audio_engine_mock.capture_current_constant_timing.assert_not_called()
    assert observer.controller.accepted_timing._worker is None
    pending.remove(4)
    observer._wait_for_source(0)
    assert observer.preparation is not None
    audio_engine_mock.capture_current_constant_timing.assert_called_once()


def test_failed_fresh_preparation_retains_old_error_and_new_failure(
    budgeted_observer: ProductiveObserver, audio_engine_mock: Mock
) -> None:
    observer = budgeted_observer
    observer.controller.session.sample_analysis_errors[0] = "old restore failure"
    audio_engine_mock.prepare_captured_constant_timing.side_effect = ValueError("new PCM failure")
    observer._wait_for_source(0)
    assert observer.preparation is not None
    with pytest.raises(ValueError, match="new PCM failure"):
        observer.preparation.result(timeout=2.0)
    observer.on_frame()
    observer.worker.submit(lambda: None).result(timeout=2.0)
    assert observer.failure == "new PCM failure"
    assert observer.controller.session.sample_analysis_errors[0] == "old restore failure"
    assert not (observer.directory / "setup-ready.json").exists()
    receipt = json.loads((observer.directory / "setup-failed.json").read_text())
    assert receipt["error"] == "new PCM failure"


def test_fresh_success_cannot_clear_a_new_analysis_error(
    budgeted_observer: ProductiveObserver, audio_engine_mock: Mock
) -> None:
    observer = budgeted_observer
    observer.controller.session.sample_analysis_errors[0] = "old restore failure"
    ticket = audio_engine_mock.prepare_captured_constant_timing.return_value
    current = current_timing_metadata()
    current["source_sha256"] = observer.reference.source_sha256
    ticket.metadata.return_value = dict(
        current, request_id=current["accepted_request_id"], beat_seconds=[0.503, 1.004, 2.005]
    )
    observer._wait_for_source(0)
    assert observer.preparation is not None
    observer.preparation.result(timeout=2.0)
    observer.controller.session.sample_analysis_errors[0] = "new unrelated analysis failure"
    observer._finish_preparation(0)
    assert observer.controller.session.sample_analysis_errors[0] == "new unrelated analysis failure"


def test_rejected_pcm_geometry_retains_binding_without_capture_or_error_clear(
    budgeted_observer: ProductiveObserver, audio_engine_mock: Mock
) -> None:
    observer = budgeted_observer
    binding = budget_binding(observer.reference)
    binding["channels"] = True
    audio_engine_mock.current_input_runtime_pad_binding.return_value = Mock(
        metadata=lambda: binding
    )
    observer.controller.session.sample_analysis_errors[0] = "historical restore failure"
    with pytest.raises(ValueError, match="actual native channels"):
        observer._wait_for_source(0)
    observer.worker.submit(lambda: None).result(timeout=2.0)
    audio_engine_mock.capture_current_constant_timing.assert_not_called()
    assert observer.controller.accepted_timing._worker is None
    assert observer.controller.session.sample_analysis_errors[0] == "historical restore failure"
    receipt = json.loads((observer.directory / "pad-0-pcm-admission-rejected.json").read_text())
    assert receipt["native_source_binding"] == binding
    assert (
        receipt["prior_analysis_error_before_explicit_preparation"] == "historical restore failure"
    )
    assert not (observer.directory / "setup-ready.json").exists()


def test_adapter_uses_productive_controller_prepare_publish_current_and_derived_ack(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    source = tmp_path / "source.wav"
    source.write_bytes(b"synthetic-fixture-for-real-controller-methods")
    plan_path = prepare_packet(
        tmp_path, source, tmp_path / "exports/run", reference=reference(source), pads=1
    )
    plan, ref = load_plan(tmp_path, plan_path)
    current = current_timing_metadata()
    accepted_period = current["period_seconds_per_quarter"]
    assert isinstance(accepted_period, float)
    current.update(source_sha256=ref.source_sha256, frame_count=3 * 48_000)
    binding = FakeInputRuntimePadBinding(accepted_timing=current, intent="automatic")
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = binding
    audio_engine_mock.current_constant_timing.return_value = None
    audio_engine_mock.poll_loader_events.return_value = None
    audio_engine_mock.set_pad_timing_intent.side_effect = lambda _pad, intent: setattr(
        audio_engine_mock.pad_timing_intent, "return_value", intent
    )
    ticket = Mock()
    ticket.metadata.return_value = dict(
        current, request_id=current["accepted_request_id"], beat_seconds=[0.503, 1.004, 2.005]
    )
    ticket.publication_status.return_value = "pending"
    audio_engine_mock.prepare_captured_constant_timing.return_value = ticket
    refresh = Mock()
    refresh.publication_status.return_value = "pending"
    refresh.is_current.return_value = False
    audio_engine_mock.refresh_current_constant_timing.return_value = refresh
    controller.project.sample_paths[0] = str(source)
    controller.project.bpm_lock = True
    controller.project.speed = 0.73
    controller.project.pad_loop_auto[0] = False

    observer = blank_observer(controller, plan, ref, tmp_path / "observations")

    try:
        advance_observer_until(observer, "awaiting_actual_publication_ack")
        assert not observer.completed
        audio_engine_mock.capture_current_constant_timing.assert_called_once()
        audio_engine_mock.prepare_captured_constant_timing.assert_called_once()
        assert json.loads(audio_engine_mock.publish_constant_timing.call_args.args[1])[0][
            "quarter_counts"
        ] == [1, 2, 4]
        ticket.publication_status.return_value = "accepted"
        audio_engine_mock.current_constant_timing.return_value = current
        advance_observer_until(observer, "awaiting_actual_derived_ack")
        assert not observer.completed
        assert not (observer.directory / "setup-ready.json").exists()
        refresh.publication_status.return_value = "accepted"
        refresh.is_current.return_value = True
        advance_observer_until(observer, "ready")
        assert observer.completed == {0}
        assert controller.session.master_period_seconds == accepted_period / 0.73
        assert controller.project.pad_loop_end_s[0] == accepted_period * 2
        audio_engine_mock.play_sample.assert_not_called()
        audio_engine_mock.play_sample_exclusive.assert_not_called()
    finally:
        observer.worker.shutdown(wait=True)
        controller.accepted_timing.shut_down()


def test_rejected_independent_mapping_preserves_actual_native_preparation(tmp_path: Path) -> None:
    source = tmp_path / "source.wav"
    source.write_bytes(b"synthetic-independent-source")
    observer = ProductiveObserver.__new__(ProductiveObserver)
    observer.reference = reference(source)
    observer.plan = Mock(reference_sha256="b" * 64)
    observer.directory = tmp_path
    actual = dict(metadata(observer.reference), pad_id=0, beat_seconds=[0.2, 1.0, 2.0])
    ticket = Mock(metadata=Mock(return_value=actual))
    with pytest.raises(ValueError, match="unique unused independent"):
        observer._map_ticket(ticket)
    raw_path = tmp_path / "pad-0-actual-preparation.json"
    raw = json.loads(raw_path.read_bytes())
    failure = json.loads((tmp_path / "pad-0-independent-mapping-failed.json").read_bytes())
    assert raw["actual_native_preparation"] == actual
    assert failure["actual_native_preparation_sha256"] == file_sha256(raw_path)
    assert failure["device_acceptance"] == "pending"
    assert failure["human_listening"] == "pending"


def test_user_run_exception_retires_only_its_owned_isolated_controller(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    module = "flitzis_looper.analysis.productive_loop_run"
    project = tmp_path / "isolated"
    project.mkdir()
    plan = Mock(project_directory=str(project), project_config_path=str(project / "config.json"))
    controller, observer = Mock(), Mock()
    monkeypatch.setattr(f"{module}.load_plan", Mock(return_value=(plan, Mock())))
    monkeypatch.setattr(f"{module}.AppController", Mock(return_value=controller))
    monkeypatch.setattr(f"{module}.ProductiveObserver", Mock(return_value=observer))
    monkeypatch.setattr(f"{module}.run_ui", Mock(side_effect=RuntimeError("synthetic UI failure")))
    previous = Path.cwd()
    with pytest.raises(RuntimeError, match="synthetic UI failure"):
        run_packet(tmp_path, project / "run-plan.json")
    observer.close.assert_called_once()
    controller.shut_down.assert_called_once()
    assert Path.cwd() == previous


@pytest.mark.parametrize("launch_directory", ["repo", "scratch"])
@pytest.mark.parametrize("absolute_plan", [False, True])
def test_human_cli_resolves_plan_from_workspace_before_controller_start(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    launch_directory: str,
    *,
    absolute_plan: bool,
) -> None:
    source = tmp_path / "source.wav"
    source.write_bytes(b"hardware-free launcher regression source")
    plan_path = prepare_packet(
        tmp_path, source, tmp_path / "exports/listening073", reference=reference(source), pads=1
    )
    plan, _ref = load_plan(tmp_path, plan_path)
    current_directory = tmp_path / launch_directory
    current_directory.mkdir()
    monkeypatch.chdir(current_directory)
    argument = plan_path if absolute_plan else plan_path.relative_to(tmp_path)
    monkeypatch.setattr(
        "sys.argv",
        ["productive_loop_packet", "--workspace", str(tmp_path), "run", "--plan", str(argument)],
    )
    module = "flitzis_looper.analysis.productive_loop_run"
    constructor = Mock(side_effect=RuntimeError("hardware-free controller boundary"))
    observer, ui = Mock(), Mock()
    monkeypatch.setattr(f"{module}.AppController", constructor)
    monkeypatch.setattr(f"{module}.ProductiveObserver", observer)
    monkeypatch.setattr(f"{module}.run_ui", ui)

    with pytest.raises(RuntimeError, match="hardware-free controller boundary"):
        packet_main()

    constructor.assert_called_once_with(project_config_path=Path(plan.project_config_path))
    observer.assert_not_called()
    ui.assert_not_called()
    assert Path.cwd() == current_directory


@pytest.mark.parametrize("invalid_plan", ["repo/run-plan.json", "../outside/run-plan.json"])
def test_human_run_rejects_nonprivate_plan_before_app_start(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, invalid_plan: str
) -> None:
    current_directory = tmp_path / "repo"
    current_directory.mkdir()
    monkeypatch.chdir(current_directory)
    module = "flitzis_looper.analysis.productive_loop_run"
    constructor, observer, ui = Mock(), Mock(), Mock()
    monkeypatch.setattr(f"{module}.AppController", constructor)
    monkeypatch.setattr(f"{module}.ProductiveObserver", observer)
    monkeypatch.setattr(f"{module}.run_ui", ui)

    with pytest.raises(ValueError, match="outside_workspace_or_inside_repo"):
        run_packet(tmp_path, Path(invalid_plan))

    constructor.assert_not_called()
    observer.assert_not_called()
    ui.assert_not_called()
    assert Path.cwd() == current_directory
