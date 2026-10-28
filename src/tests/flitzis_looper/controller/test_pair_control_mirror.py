"""Python custody projections only; these tests do not certify a real Native Pair."""

import threading
from concurrent.futures import Future
from dataclasses import dataclass
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import numpy as np
import pytest

from flitzis_looper.controller import accepted_publication, loader, pair_control_mirror
from flitzis_looper.controller.current_timing import CurrentPadTiming
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.pad_selection import record_pad_selection
from flitzis_looper.controller.pair_control_mirror import prepare_pair_control_mirror
from flitzis_looper.controller.transport import TransportController, global_playback, residency
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState, StemCacheEntry
from flitzis_looper.pair_snapshot import PairSnapshot, capture_pair_snapshot
from flitzis_looper.ui.context import WaveformEditorActions
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import AssetRetirementReservation
    from flitzis_looper.controller.pair_control_mirror import PreparedPairControlMirror
    from flitzis_looper_audio import (
        AcceptedTimingRefreshTicket,
        AudioEngine,
        CapturedGlobalStart,
        ConstantTimingTicket,
        GlobalPlaybackBatchTicket,
        ResidentWindowTicket,
        WaveFormRenderData,
    )


@dataclass
class _Bundle:
    app: AppController
    view: WaveformEditorActions
    audio: Mock
    acknowledgement: object
    snapshot: PairSnapshot

    def prepare(self) -> PreparedPairControlMirror:
        return prepare_pair_control_mirror(
            self.app,
            self.snapshot,
            validate_acknowledgement=lambda value: value is self.acknowledgement,
            waveform_views=(self.view,),
        )


def _bundle(*, occupied: bool = False) -> _Bundle:
    project = ProjectState()
    for sample_id, content_id in ((0, "a" * 32), (215, "b" * 32)):
        if sample_id == 215 and not occupied:
            continue
        project.pad_content[sample_id] = PadContentIdentity(instance_id=content_id)
        project.sample_paths[sample_id] = f"samples/source-{sample_id}.wav"
        project.stem_cache[sample_id] = StemCacheEntry(
            source_version=f"source-{sample_id}", cache_dir=f"stems/source-{sample_id}"
        )
    session = SessionState()
    audio = Mock(spec=[])
    transport = TransportController(project, session, cast("AudioEngine", audio))
    app = cast(
        "AppController",
        SimpleNamespace(
            _project=project,
            _session=session,
            _audio=audio,
            transport=transport,
            accepted_timing=accepted_publication.AcceptedTimingController(transport),
            loader=loader.LoaderController(
                project, session, cast("AudioEngine", audio), lambda _sample_id: None
            ),
            metering=MeteringController(project, session, cast("AudioEngine", audio)),
        ),
    )
    return _Bundle(
        app,
        WaveformEditorActions(app),
        audio,
        object(),
        capture_pair_snapshot(project, 0, 215, operation="swap" if occupied else "move"),
    )


def _pending() -> residency._Pending:
    intent = residency._Intent(
        path="samples/source-0.wav",
        start=1.0,
        end=5.0,
        auto=False,
        bars=4.0,
        key_lock=True,
        key_lock_override=None,
    )
    return residency._Pending(
        intent,
        intent,
        1.0,
        5.0,
        2.0,
        lambda _ticket: True,
        (object(), "source-0", 7, 11, "legacy", None, None, 0),
        expected_source_generation=7,
        ticket=cast("ResidentWindowTicket", Mock(spec=[])),
        waiting_ticket=cast("ResidentWindowTicket", Mock(spec=[])),
        waiting_intent=intent,
        attempts=4,
        deadline=12345.0,
    )


def _refresh(
    anchor: int, callback: Callable[[int], None] | None = None
) -> accepted_publication._RefreshRequest:
    return accepted_publication._RefreshRequest(
        CurrentPadTiming(0.5, 0.0, 48_000),
        (
            "samples/source-0.wav",
            None,
            "automatic",
            0.0,
            None,
            False,
            4.0,
            None,
            0,
            1.0,
            True,
            True,
            True,
            anchor,
        ),
        (callback,) if callback is not None else (),
        admission_failures=2,
    )


def _populate_session(session: SessionState) -> None:
    session.active_sample_ids.add(0)
    session.paused_sample_ids.add(0)
    session.global_stop_restore_sample_ids.add(0)
    session.loading_sample_ids.add(0)
    session.analyzing_sample_ids.add(0)
    session.pending_sample_paths[0] = "new-original.wav"
    session.sample_load_progress[0] = 0.3
    session.sample_analysis_progress[0] = 0.7
    session.pressed_pads[0] = True
    session.pad_peak[0] = 0.6
    session.pad_peak_updated_at[0] = 10.0
    session.pad_clip_hold_until[0] = 11.0
    session.pad_playhead_s[0] = 2.2
    session.pad_playhead_updated_at[0] = 12.0
    session.waveform_editor_open = True
    session.waveform_editor_pad_id = 0
    session.waveform_pause_hold_pad_id = 0
    session.tap_bpm_pad_id = 0
    session.tap_bpm_timestamps = [1.0, 1.5]
    session.bpm_lock_anchor_pad_id = 0


def _retirement() -> AssetRetirementReservation:
    # These identity-only owners must reject every unexpected lifecycle operation.
    return cast("AssetRetirementReservation", object())


def _rendered_waveform() -> WaveFormRenderData:
    return (
        True,
        np.array([0.0, 0.5], dtype=np.float64),
        np.zeros(2, dtype=np.float32),
        None,
    )


def _populate(
    bundle: _Bundle,
) -> tuple[
    residency._Pending,
    accepted_publication._RefreshRequest,
    Future[None],
    AssetRetirementReservation,
]:
    app = bundle.app
    _populate_session(app._session)
    pending = _pending()
    residency_control = app.transport.residency
    residency_control._pending[0] = pending
    assert pending.ticket is not None
    residency_control._launched[0] = pending.ticket
    residency_control._effective[0] = pending.requested
    residency_control._error_sources[0] = pending.owner
    residency_control._confirmed_modes[0] = (pending.owner, True)
    callback = Mock(spec=[])
    refresh = _refresh(0, callback)
    future: Future[None] = Future()
    ticket = cast("ConstantTimingTicket", Mock(spec=[]))
    publication = accepted_publication._Publication(
        future, ticket, "samples/source-0.wav", {"pad_id": 0}
    )
    app.accepted_timing._publications[0] = publication
    app.accepted_timing._preparations[0] = cast("Future[ConstantTimingTicket]", future)
    app.accepted_timing._waiting_refresh[0] = refresh
    app.loader._load_request_ids[0] = 7
    app.loader._analysis_request_ids[0] = 19
    app.loader._incoming_loads[0] = loader._IncomingLoad(
        loader._IncomingIntent(request_id=7, revision=2, override=None, key_lock=True)
    )
    retirement = _retirement()
    app.loader._load_retirements[0, 7] = retirement
    app.transport.waveform._readiness[0] = ("ready", None)
    app.metering._active_peak_sample_ids.add(0)
    bundle.view._pad_view_ranges[0] = (-0.4, 8.6)
    bundle.view._pad_view_sources[0] = ("samples/source-0.wav", 15.0, None, None)
    bundle.view._last_pad_id = 0
    bundle.view._last_width_px = 500
    bundle.view._last_start_s = -0.4
    bundle.view._last_end_s = 8.6
    bundle.view._last_source_identity = bundle.view._pad_view_sources[0]
    bundle.view._last_waveform_value = _rendered_waveform()
    return pending, refresh, future, retirement


def _assert_moved_session(session: SessionState, timestamps: list[float]) -> None:
    assert session.active_sample_ids == {215}
    assert session.paused_sample_ids == {215}
    assert session.loading_sample_ids == {215}
    assert session.analyzing_sample_ids == {215}
    assert session.global_stop_restore_sample_ids == {215}
    assert session.pad_peak[215] == 0.6
    assert session.pad_playhead_s[215] == 2.2
    assert session.pad_peak[0] == 0.0
    assert session.pad_playhead_s[0] is None
    assert session.waveform_editor_pad_id == 215
    assert session.waveform_pause_hold_pad_id == 215
    assert session.tap_bpm_pad_id == 215
    assert session.tap_bpm_timestamps is timestamps


def _assert_swap_candidate_contents(
    candidate: ProjectState,
    original: tuple[PadContentIdentity | None, PadContentIdentity | None],
) -> None:
    source, target = original
    projected_source, projected_target = candidate.pad_content[215], candidate.pad_content[0]
    assert source is not None
    assert target is not None
    assert projected_source is not None
    assert projected_target is not None
    assert projected_source.instance_id == source.instance_id
    assert projected_target.instance_id == target.instance_id


def test_move_retains_original_rows_and_independent_empty_defaults() -> None:
    bundle = _bundle()
    app = bundle.app
    app._project.pad_gain_db[215] = -4.0
    app._project.pad_loop_start_s[215] = 3.0
    bundle.snapshot = capture_pair_snapshot(app._project, 0, 215, operation="move")
    pending, refresh, future, retirement = _populate(bundle)
    original = SimpleNamespace(
        content=app._project.pad_content[0],
        key_intent=app._project.pad_key_intent[0],
        stem=app._project.stem_cache[0],
        publication=app.accepted_timing._publications[0],
        incoming=app.loader._incoming_loads[0],
        timestamps=app._session.tap_bpm_timestamps,
        view_range=bundle.view._pad_view_ranges[0],
        source_identity=bundle.view._pad_view_sources[0],
        after_ack=pending.after_ack,
    )
    plan = bundle.prepare()
    assert dict(plan.projected_entries)[215] is original.stem
    plan.apply(bundle.acknowledgement)
    assert plan.applied
    assert app._project.pad_content[215] is original.content
    assert app._project.pad_key_intent[215] is original.key_intent
    assert app._project.stem_cache[215] is original.stem
    assert app._project.pad_content[0] is None
    assert app._project.pad_gain_db[0] == 0.0
    assert app._project.pad_loop_start_s[0] == 0.0
    _assert_moved_session(app._session, original.timestamps)
    assert app.transport.residency._pending[215] is pending
    assert pending.after_ack is original.after_ack
    assert pending.attempts == 4
    assert pending.deadline == 12345.0
    assert app.accepted_timing._publications[215] is original.publication
    assert original.publication.future is future
    assert app.accepted_timing._waiting_refresh[215] is refresh
    assert refresh.control_signature[-1] == 215
    assert refresh.admission_failures == 2
    assert app.loader._incoming_loads[215] is original.incoming
    assert app.loader._load_retirements[215, 7] is retirement
    assert bundle.view._pad_view_ranges[215] is original.view_range
    assert bundle.view._pad_view_sources[215] is original.source_identity
    assert bundle.view._last_waveform_value is None
    assert app.transport.waveform._view_revision == 0
    assert not future.cancelled()
    assert not bundle.audio.mock_calls


def test_latest_unrelated_rows_navigation_and_global_replacement_survive() -> None:
    bundle = _bundle(occupied=True)
    _populate(bundle)
    app = bundle.app
    global_control = app.transport.playback._global_playback
    prior_ticket = cast("GlobalPlaybackBatchTicket", Mock(spec=[]))
    global_control._pending = global_playback._PendingGlobalBatch(prior_ticket, frozenset({0}), 3)
    plan = bundle.prepare()
    newer = _pending()
    app.transport.residency._pending[36] = newer
    app._session.sample_load_progress[36] = 0.9
    app._session.loading_sample_ids.add(36)
    app._project.pad_gain_db[36] = -8.0
    record_pad_selection(app._project, app._session, 36)
    app._session.waveform_editor_pad_id = 36
    app._session.tap_bpm_pad_id = 36
    app._session.tap_bpm_timestamps = [10.0, 10.5]
    app._session.master_output_peak = 0.88
    latest_request = _refresh(0)
    app.accepted_timing._waiting_refresh[36] = latest_request
    replacement = global_playback._PendingGlobalBatch(
        cast("GlobalPlaybackBatchTicket", Mock(spec=[])), frozenset({36}), 12
    )
    global_control._pending = replacement
    global_control._restore_revision = 12
    view_range = (40.0, 60.0)
    bundle.view._pad_view_ranges[36] = view_range
    bundle.view._last_pad_id = 36
    rendered = _rendered_waveform()
    bundle.view._last_waveform_value = rendered
    plan.apply(bundle.acknowledgement)
    assert app.transport.residency._pending[36] is newer
    assert app._session.loading_sample_ids == {36, 215}
    assert app._session.sample_load_progress[36] == 0.9
    assert app._project.pad_gain_db[36] == -8.0
    assert app._project.selected_pad == 36
    assert app._session.waveform_editor_pad_id == 36
    assert app._session.tap_bpm_pad_id == 36
    assert app._session.tap_bpm_timestamps == [10.0, 10.5]
    assert app._session.master_output_peak == 0.88
    assert app.accepted_timing._waiting_refresh[36] is latest_request
    assert latest_request.control_signature[-1] == 215
    assert global_control._pending is replacement
    assert global_control._restore_revision == 12
    assert bundle.view._pad_view_ranges[36] is view_range
    assert bundle.view._last_waveform_value is rendered


def test_global_ticket_and_deferred_accepted_tail_keep_exact_owners() -> None:
    bundle = _bundle(occupied=True)
    app = bundle.app
    app._session.bpm_lock_anchor_pad_id = 0
    global_control = app.transport.playback._global_playback
    original = SimpleNamespace(
        ticket=cast("GlobalPlaybackBatchTicket", Mock(spec=[])),
        accepted=object(),
        signature=(0, object(), 4, "digest", 48_000, 100, 2, "automatic", 8),
        refresh_ticket=cast("AcceptedTimingRefreshTicket", Mock(spec=[])),
    )
    pending = global_playback._PendingGlobalBatch(
        original.ticket, frozenset({0, 36}), 7, start=True, start_ids=frozenset({215})
    )
    deferred = global_playback._DeferredStart(
        cast("CapturedGlobalStart", Mock(spec=[])), frozenset({0}), 7, 500.0, 3
    )
    global_control._pending = pending
    global_control._deferred = deferred
    request = _refresh(0)
    refresh = accepted_publication._Refresh(request, original.refresh_ticket, 9, 0.5)
    app.accepted_timing._refreshes[36] = refresh
    plan = bundle.prepare()
    plan.apply(bundle.acknowledgement)
    projected = global_control._pending
    assert projected.ticket is original.ticket
    assert projected.restore_revision == 7
    assert projected.restore_ids == frozenset({215, 36})
    assert projected.start_ids == frozenset({0})
    projected_deferred = global_control._deferred
    assert projected_deferred.capture is deferred.capture
    assert projected_deferred.deadline == deferred.deadline
    assert projected_deferred.attempts == deferred.attempts
    assert projected_deferred.sample_ids == frozenset({215})
    assert app.accepted_timing._refreshes[36] is refresh
    assert refresh.ticket is original.refresh_ticket
    assert request.control_signature[-1] == 215


@pytest.mark.parametrize(
    ("owner_name", "field_name"),
    [
        ("project", "pad_content"),
        ("session", "loading_sample_ids"),
        ("residency", "_pending"),
        ("global", "_pending"),
        ("request", "control_signature"),
        ("view", "_last_pad_id"),
    ],
)
def test_interrupted_swap_retries_same_plan_once_with_latest_unrelated_rows(
    monkeypatch: pytest.MonkeyPatch, owner_name: str, field_name: str
) -> None:
    bundle = _bundle(occupied=True)
    pending, request, future = _populate(bundle)[:3]
    app = bundle.app
    original = (app._project.pad_content[0], app._project.pad_content[215])
    target_pending = _pending()
    app.transport.residency._pending[215] = target_pending
    app.transport.playback._global_playback._pending = global_playback._PendingGlobalBatch(
        cast("GlobalPlaybackBatchTicket", Mock(spec=[])), frozenset({0}), 1
    )
    interrupted_owner = {
        "project": app._project,
        "session": app._session,
        "residency": app.transport.residency,
        "global": app.transport.playback._global_playback,
        "request": request,
        "view": bundle.view,
    }[owner_name]
    store = pair_control_mirror._store
    failed = False

    def interrupt_after_store(owner: object, name: str, image: object) -> None:
        nonlocal failed
        store(owner, name, image)
        if owner is interrupted_owner and name == field_name and not failed:
            failed = True
            message = "interrupted after own store"
            raise RuntimeError(message)

    monkeypatch.setattr(pair_control_mirror, "_store", interrupt_after_store)
    plan = bundle.prepare()
    with pytest.raises(RuntimeError, match="interrupted after own store"):
        plan.apply(bundle.acknowledgement)
    assert not plan.applied
    with pytest.raises(RuntimeError, match="another transaction acknowledgement"):
        plan.apply(object())
    latest = _pending()
    app.transport.residency._pending[36] = latest
    app._session.sample_analysis_stage[36] = "Newest unrelated analysis"
    app._project.pad_gain_db[36] = -7.0
    candidate = plan.projected_project()
    _assert_swap_candidate_contents(candidate, original)
    assert candidate.pad_gain_db[36] == -7.0
    plan.apply(bundle.acknowledgement)
    assert plan.applied
    assert failed
    assert app._project.pad_content[215] is original[0]
    assert app._project.pad_content[0] is original[1]
    assert app.transport.residency._pending[215] is pending
    assert app.transport.residency._pending[0] is target_pending
    assert app.transport.residency._pending[36] is latest
    assert app._session.sample_analysis_stage[36] == "Newest unrelated analysis"
    assert app._project.pad_gain_db[36] == -7.0
    assert bundle.view._last_waveform_value is None
    assert not future.cancelled()
    assert not bundle.audio.mock_calls
    containers = (app._project.pad_content, app.transport.residency._pending)
    plan.apply(bundle.acknowledgement)
    assert app._project.pad_content is containers[0]
    assert app.transport.residency._pending is containers[1]


def test_historical_retirements_keep_original_keys_and_current_receipt_moves() -> None:
    bundle = _bundle()
    app = bundle.app
    app.loader._load_request_ids[0] = 7
    current, predecessor, old_target = _retirement(), _retirement(), _retirement()
    app.loader._load_retirements = {(0, 7): current, (0, 3): predecessor, (215, 99): old_target}
    plan = bundle.prepare()
    plan.apply(bundle.acknowledgement)
    assert app.loader._load_retirements == {
        (215, 7): current,
        (0, 3): predecessor,
        (215, 99): old_target,
    }


def test_retirement_destination_collision_rejects_before_any_exchange() -> None:
    bundle = _bundle()
    app = bundle.app
    app.loader._load_request_ids[0] = 7
    app.loader._load_retirements = {(0, 7): _retirement(), (215, 7): _retirement()}
    original_content = app._project.pad_content
    plan = bundle.prepare()
    with pytest.raises(RuntimeError, match="_load_retirements changed"):
        plan.apply(bundle.acknowledgement)
    assert app._project.pad_content is original_content
    assert not plan.applied


def test_replaced_participant_or_component_rejects_before_any_exchange(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    bundle = _bundle()
    app = bundle.app
    pending = _pending()
    app.transport.residency._pending[0] = pending
    plan = bundle.prepare()
    app.transport.residency._pending[0] = _pending()
    original_content = app._project.pad_content
    with pytest.raises(RuntimeError, match="_pending changed"):
        plan.apply(bundle.acknowledgement)
    assert app._project.pad_content is original_content
    app.transport.residency._pending[0] = pending
    monkeypatch.setattr(app.transport, "residency", SimpleNamespace())
    with pytest.raises(RuntimeError, match="component or model ownership changed"):
        plan.apply(bundle.acknowledgement)
    assert app._project.pad_content is original_content


def test_opaque_ack_and_original_thread_are_required() -> None:
    bundle = _bundle()
    plan = bundle.prepare()
    with pytest.raises(RuntimeError, match="opaque actual own Native ACK"):
        plan.apply({"status": "accepted"})
    errors = []

    def on_other_thread() -> None:
        try:
            plan.apply(bundle.acknowledgement)
        except RuntimeError as error:
            errors.append(str(error))

    thread = threading.Thread(target=on_other_thread)
    thread.start()
    thread.join(5.0)
    assert not thread.is_alive()
    assert errors == ["Pair control mirror must apply on its original Controller thread"]
    assert not plan.applied


def test_later_hold_terminal_and_unrelated_anchor_are_preserved() -> None:
    bundle = _bundle()
    _populate(bundle)
    plan = bundle.prepare()
    bundle.app._session.waveform_pause_hold_pad_id = None
    bundle.app._session.bpm_lock_anchor_pad_id = 36
    latest_request = _refresh(36)
    bundle.app.accepted_timing._waiting_refresh[36] = latest_request
    signature = latest_request.control_signature
    plan.apply(bundle.acknowledgement)
    assert bundle.app._session.waveform_pause_hold_pad_id is None
    assert bundle.app._session.bpm_lock_anchor_pad_id == 36
    assert latest_request.control_signature is signature


@pytest.mark.parametrize(
    ("occupied", "selected", "expected"),
    [
        (False, 0, 215),
        (False, 215, 215),
        (False, 36, 36),
        (True, 0, 215),
        (True, 215, 0),
        (True, 36, 36),
    ],
)
def test_config_candidate_and_live_mirror_select_the_same_surviving_content(
    *, occupied: bool, selected: int, expected: int
) -> None:
    bundle = _bundle(occupied=occupied)
    record_pad_selection(bundle.app._project, bundle.app._session, selected)
    plan = bundle.prepare()
    candidate = plan.projected_project()
    assert candidate.selected_pad == expected
    assert bundle.app._project.selected_pad == selected
    plan.apply(bundle.acknowledgement)
    assert bundle.app._project.selected_pad == expected
    assert plan.projected_project().selected_pad == expected
    plan.apply(bundle.acknowledgement)
    assert bundle.app._project.selected_pad == expected


@pytest.mark.parametrize(
    ("interruption", "expected"),
    [("sample_paths", 215), ("pad_content", 0), ("selected_pad", 0)],
)
def test_navigation_aba_binds_content_chosen_before_or_after_partial_pair_projection(
    monkeypatch: pytest.MonkeyPatch, interruption: str, expected: int
) -> None:
    bundle = _bundle(occupied=True)
    app = bundle.app
    record_pad_selection(app._project, app._session, 0)
    plan = bundle.prepare()
    store = pair_control_mirror._store
    interrupted = False

    def interrupt_after_store(owner: object, name: str, image: object) -> None:
        nonlocal interrupted
        store(owner, name, image)
        if owner is app._project and name == interruption and not interrupted:
            interrupted = True
            message = "selection projection interrupted"
            raise RuntimeError(message)

    monkeypatch.setattr(pair_control_mirror, "_store", interrupt_after_store)
    with pytest.raises(RuntimeError, match="selection projection interrupted"):
        plan.apply(bundle.acknowledgement)
    record_pad_selection(app._project, app._session, 0)
    record_pad_selection(app._project, app._session, 36)
    record_pad_selection(app._project, app._session, 0)
    chosen = "a" * 32 if interruption == "sample_paths" else "b" * 32
    assert app._session.pad_selection_content_id == chosen
    assert app._session.pad_selection_revision == 4
    assert plan.projected_project().selected_pad == expected
    plan.apply(bundle.acknowledgement)
    assert app._project.selected_pad == expected
    assert plan.projected_project().selected_pad == expected


def test_fresh_next_pair_follows_prior_selected_content_without_reusing_its_old_placement() -> None:
    bundle = _bundle(occupied=True)
    app = bundle.app
    record_pad_selection(app._project, app._session, 0)
    first = bundle.prepare()
    first.apply(bundle.acknowledgement)
    assert app._project.selected_pad == 215
    bundle.snapshot = capture_pair_snapshot(app._project, 0, 215, operation="swap")
    bundle.acknowledgement = object()
    second = bundle.prepare()
    assert second.projected_project().selected_pad == 0
    second.apply(bundle.acknowledgement)
    assert app._project.selected_pad == 0
    assert app._session.pad_selection_content_id == "a" * 32


def _install_bpm_queue(audio: Mock) -> None:
    """Controller-only queued receipt; no actual Native completion is simulated."""
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

    def admit(*, enabled: bool, anchor_binding: object = None) -> None:
        assert enabled is True
        assert anchor_binding is not None
        receipt.update(request_revision=1, state="queued", requested_enabled=True)

    audio.bpm_lock_status = Mock(side_effect=receipt.copy)
    audio.set_bpm_lock = Mock(side_effect=admit)
    audio.current_constant_timing = Mock(return_value=None)
    audio.pad_timing_intent = Mock(return_value="legacy")
    audio.output_sample_rate = Mock(return_value=48_000)
    audio.current_input_runtime_pad_binding = Mock(side_effect=FakeInputRuntimePadBinding)


@pytest.mark.parametrize("swap", [False, True])
def test_unbound_empty_anchor_stays_at_pair_fill_slot_then_admits_that_first_content_once(
    *, swap: bool
) -> None:
    bundle = _bundle(occupied=swap)
    app = bundle.app
    project, session = app._project, app._session
    target = (project.pad_content[215], project.sample_paths[215], project.stem_cache[215])
    project.pad_content[215] = None
    project.sample_paths[215] = None
    project.stem_cache[215] = None
    project.multi_loop = project.bpm_lock = True
    project.selected_pad = 215
    parameters = app.transport.global_params
    parameters._begin_bpm_lock_application(capture_anchor=True)
    application = parameters._bpm_application
    assert application is not None
    assert application.original_content_id is None
    # Swap requires an occupied target. Its first assignment arrives under the
    # existing reservation before the BPM owner may observe or bind it.
    project.pad_content[215], project.sample_paths[215], project.stem_cache[215] = target
    project.manual_bpm[0] = 120.0
    guard = Mock(side_effect=ValueError("Pair assignment is held."))
    app.transport.bind_pad_edit_guard(guard)
    with pytest.raises(ValueError, match="held"):
        parameters._bind_first_bpm_content()
    assert application.original_content_id is None
    bundle.snapshot = capture_pair_snapshot(project, 0, 215, operation="swap" if swap else "move")
    record_pad_selection(project, session, 36)
    refresh = _refresh(215)
    app.accepted_timing._waiting_refresh[0] = refresh
    plan = bundle.prepare()
    assert plan._anchor_after == 215
    plan.apply(bundle.acknowledgement)
    plan.apply(bundle.acknowledgement)
    assert session.bpm_lock_anchor_pad_id == 215
    assert project.selected_pad == 36
    assert app.accepted_timing._waiting_refresh[215] is refresh
    assert refresh.control_signature[-1] == 215
    assert project.pad_content[215].instance_id == "a" * 32
    assert application.original_content_id is None
    guard.side_effect = None
    _install_bpm_queue(bundle.audio)
    for _ in range(4):
        parameters.poll_bpm_lock_application()
    assert application.original_content_id == "a" * 32
    bundle.audio.set_bpm_lock.assert_called_once()
    binding = bundle.audio.set_bpm_lock.call_args.kwargs["anchor_binding"]
    assert binding.metadata()["pad_id"] == 215


@pytest.mark.parametrize(
    ("swap", "anchor", "expected"),
    [(False, 0, 215), (True, 0, 215), (True, 215, 0)],
)
def test_bound_bpm_original_keeps_existing_pair_content_and_signature_projection(
    *, swap: bool, anchor: int, expected: int
) -> None:
    bundle = _bundle(occupied=swap)
    app = bundle.app
    project = app._project
    project.multi_loop = project.bpm_lock = True
    project.selected_pad = anchor
    parameters = app.transport.global_params
    parameters._begin_bpm_lock_application(capture_anchor=True)
    application = parameters._bpm_application
    assert application is not None
    original = application.original_content_id
    assert original is not None
    refresh = _refresh(anchor)
    app.accepted_timing._waiting_refresh[0] = refresh
    plan = bundle.prepare()
    assert plan._anchor_after == expected
    plan.apply(bundle.acknowledgement)
    assert app._session.bpm_lock_anchor_pad_id == expected
    assert refresh.control_signature[-1] == expected
    assert project.pad_content[expected].instance_id == original
    assert application.original_content_id == original
    assert not parameters._bpm_anchor_waiting_for_first_content(expected)


@pytest.mark.parametrize(("swap", "expected"), [(False, None), (True, 0)])
def test_legacy_anchor_without_an_unbound_application_keeps_existing_pair_projection(
    *, swap: bool, expected: int | None
) -> None:
    bundle = _bundle(occupied=swap)
    bundle.app._session.bpm_lock_anchor_pad_id = 215
    plan = bundle.prepare()
    assert plan._anchor_after == expected
    plan.apply(bundle.acknowledgement)
    assert bundle.app._session.bpm_lock_anchor_pad_id == expected
