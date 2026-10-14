"""App control wiring only; complete Copy adoption and custody are Native-tested."""

import json
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, call

import pytest

from flitzis_looper.controller.current_timing import UNRESOLVED_TIMING, CurrentPadTiming
from flitzis_looper.controller.pad_arrangement import PadArrangementController
from flitzis_looper.models import PadContentIdentity, StemCacheEntry
from flitzis_looper.stem_intent import PadStemIntent
from flitzis_looper.stem_pair_selection import StemPairSelection

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import CommittedStemSetTicket, PadCopyTicket

SOURCE = 0
TARGET = 1


def _metadata(**overrides: object) -> dict[str, object]:
    return {
        "target_id": TARGET,
        "source_generation": 17,
        "content_lifetime": 23,
        "status": "accepted",
        "control_mirrored": True,
        "components_available": False,
        **overrides,
    }


def _ticket(
    metadata: dict[str, object] | None = None,
    *,
    selection: CommittedStemSetTicket | None = None,
) -> PadCopyTicket:
    ticket = Mock(spec=["metadata_json", "committed_stem_ticket"])
    ticket.metadata_json.return_value = json.dumps(_metadata() if metadata is None else metadata)
    ticket.committed_stem_ticket.return_value = selection
    return cast("PadCopyTicket", ticket)


def _selection(*, state: str = "accepted", current: bool = True) -> CommittedStemSetTicket:
    ticket = Mock(spec=["publication_status", "is_current", "cancel", "error"])
    ticket.publication_status.return_value = state
    ticket.is_current.return_value = current
    return cast("CommittedStemSetTicket", ticket)


def _entry() -> StemCacheEntry:
    root = f"samples/materials/M{'b' * 32}"
    pair = StemPairSelection(
        descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{'c' * 32}.json",
        stem_set_identity="d" * 64,
        wav_generation=f"{root}/stems/.ready-{'c' * 32}",
        pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{'c' * 32}",
    )
    return StemCacheEntry(
        source_version=f"{root}/original/source.wav|sha256-v1:{'e' * 64}",
        cache_dir=pair.wav_generation,
        pair=pair,
    )


def _bind_copied_pair(controller: AppController) -> StemCacheEntry:
    entry = _entry()
    controller.project.sample_paths[TARGET] = "samples/materials/copied/original/source.wav"
    controller.project.pad_content[TARGET] = PadContentIdentity(
        instance_id="f" * 32, material_id="b" * 32
    )
    controller.project.stem_cache[TARGET] = entry
    controller.project.pad_stem_mix_mode[TARGET] = "all_stems"
    controller.project.pad_stem_intent[TARGET] = PadStemIntent(
        enabled_mask=0, last_custom_mask=6, display_mode="custom"
    )
    return entry


def test_app_binds_copy_to_existing_assets_runner_and_callback(controller: AppController) -> None:
    arrangement = controller.arrangement
    assert isinstance(arrangement, PadArrangementController)
    assert arrangement._persistence is controller.persistence
    assert arrangement._session is controller.session
    assert arrangement._audio is controller._audio
    assert arrangement._assets is controller._assets
    assert arrangement._runner is controller.stems._stem_task_runner
    assert arrangement._on_copy_committed == controller._on_copy_committed
    assert controller.transport._pad_edit_guard == controller.ensure_not_copy_target
    assert controller.stems._pad_edit_guard == controller.ensure_not_copy_target
    assert (
        controller.transport._global_key_lock_edit_guard
        == controller._ensure_global_key_lock_edit_allowed
    )


def test_runtime_poll_settles_pairs_and_copy_before_observers_and_frame_controllers(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    events: list[str] = []
    monkeypatch.setattr(controller.transport.waveform, "poll_holds", lambda: events.append("hold"))
    monkeypatch.setattr(controller.pairs, "poll", lambda: events.append("pair"))
    monkeypatch.setattr(controller.loader, "poll_loader_events", lambda: events.append("loader"))
    monkeypatch.setattr(controller.material_migration, "poll", lambda: events.append("migration"))
    monkeypatch.setattr(controller, "_poll_audio_messages", lambda: events.append("audio"))
    monkeypatch.setattr(controller.accepted_timing, "poll", lambda: events.append("timing"))
    monkeypatch.setattr(controller.transport.residency, "poll", lambda: events.append("residency"))
    monkeypatch.setattr(controller.arrangement, "poll", lambda: events.append("copy"))

    controller.poll_runtime_events()

    assert events == [
        "hold",
        "pair",
        "copy",
        "loader",
        "migration",
        "audio",
        "timing",
        "residency",
        "copy",
    ]
    events.clear()
    for component in controller._controllers:
        monkeypatch.setattr(component, "on_frame_render", lambda: events.append("frame"))

    controller.on_frame_render()

    assert events == ["pair", "copy", "frame", "frame", "frame", "frame", "frame"]


@pytest.mark.parametrize("action", ["gain", "eq", "mask", "mode", "delete", "generate"])
def test_target_guard_rejects_before_project_or_native_edits(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    action: str,
) -> None:
    monkeypatch.setattr(
        PadArrangementController, "fenced_sample_ids", property(lambda _self: frozenset({TARGET}))
    )
    edits = {
        "gain": lambda: controller.transport.pad.set_pad_gain(TARGET, -6.0),
        "eq": lambda: controller.transport.pad.set_pad_eq(TARGET, -3.0, 2.0, 1.0),
        "mask": lambda: controller.stems.set_stem_enabled_mask(TARGET, 0),
        "mode": lambda: controller.stems.set_stem_mix_mode(TARGET, "full_mix"),
        "delete": lambda: controller.stems.delete_stems(TARGET),
        "generate": lambda: controller.stems.generate_stems_async(TARGET),
    }
    before = controller.project.model_dump()
    session = controller.session.model_dump()
    revision = controller.persistence.revision
    audio_engine_mock.reset_mock()

    with pytest.raises(ValueError, match="Complete Copy target receipt is pending"):
        edits[action]()

    assert controller.project.model_dump() == before
    assert controller.session.model_dump() == session
    assert controller.persistence.revision == revision
    assert audio_engine_mock.mock_calls == []
    controller.ensure_not_copy_target(SOURCE)


def test_target_guard_precedes_current_source_generation_capture(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(
        PadArrangementController, "fenced_sample_ids", property(lambda _self: frozenset({TARGET}))
    )
    audio_engine_mock.reset_mock()

    with pytest.raises(ValueError, match="Complete Copy target receipt is pending"):
        controller.transport.capture_current_pad_edit_generation(TARGET)

    audio_engine_mock.current_input_runtime_pad_binding.assert_not_called()


@pytest.mark.parametrize("reservation", ["arrangement", "pairs"])
def test_global_key_lock_guard_precedes_generation_capture_and_project_change(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    reservation: str,
) -> None:
    def reserved_inheritance() -> None:
        message = "Key Lock inheritance is reserved by a pending Copy"
        raise RuntimeError(message)

    monkeypatch.setattr(
        getattr(controller, reservation),
        "ensure_global_key_lock_edit_allowed",
        reserved_inheritance,
    )
    capture = Mock()
    monkeypatch.setattr(
        controller.transport.global_params, "_capture_inherited_generations", capture
    )
    before = controller.project.model_dump()
    revision = controller.persistence.revision
    audio_engine_mock.reset_mock()

    with pytest.raises(RuntimeError, match="Key Lock inheritance is reserved"):
        controller.transport.global_params.set_key_lock(enabled=not controller.project.key_lock)

    capture.assert_not_called()
    assert controller.project.model_dump() == before
    assert controller.persistence.revision == revision
    assert audio_engine_mock.mock_calls == []


@pytest.mark.parametrize("target_projected", [False, True])
def test_bpm_lock_rejects_reserved_selected_anchor_before_any_state_change(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    target_projected: bool,
) -> None:
    monkeypatch.setattr(
        PadArrangementController, "fenced_sample_ids", property(lambda _self: frozenset({TARGET}))
    )
    if target_projected:
        _bind_copied_pair(controller)
    controller.project.selected_pad = TARGET
    controller.project.bpm_lock = False
    controller.session.bpm_lock_anchor_pad_id = SOURCE
    controller.session.bpm_lock_anchor_bpm = 123.0
    effective_bpm = Mock()
    monkeypatch.setattr(controller.transport.bpm, "effective_bpm", effective_bpm)
    before = controller.project.model_dump()
    session = controller.session.model_dump()
    revision = controller.persistence.revision
    audio_engine_mock.reset_mock()

    with pytest.raises(ValueError, match="Complete Copy target receipt is pending"):
        controller.transport.global_params.set_bpm_lock(enabled=True)

    assert controller.project.model_dump() == before
    assert controller.session.model_dump() == session
    assert controller.persistence.revision == revision
    effective_bpm.assert_not_called()
    assert audio_engine_mock.mock_calls == []


@pytest.mark.parametrize("provided_timing", [False, True])
def test_speed_rejects_reserved_active_bpm_anchor_before_publication(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    provided_timing: bool,
) -> None:
    monkeypatch.setattr(
        PadArrangementController, "fenced_sample_ids", property(lambda _self: frozenset({TARGET}))
    )
    _bind_copied_pair(controller)
    controller.project.bpm_lock = True
    controller.project.selected_pad = SOURCE
    controller.session.bpm_lock_anchor_pad_id = TARGET
    current_timing = Mock()
    monkeypatch.setattr(controller.transport.bpm, "current_timing", current_timing)
    before = controller.project.model_dump()
    session = controller.session.model_dump()
    revision = controller.persistence.revision
    audio_engine_mock.reset_mock()
    timing = (
        CurrentPadTiming(
            period_seconds=0.5,
            origin_seconds=0.125,
            sample_rate_hz=48_000,
            accepted_revision="scripted-own-target",
        )
        if provided_timing
        else UNRESOLVED_TIMING
    )

    with pytest.raises(ValueError, match="Complete Copy target receipt is pending"):
        controller.transport.global_params.set_speed(1.25, anchor_timing=timing)

    assert controller.project.model_dump() == before
    assert controller.session.model_dump() == session
    assert controller.persistence.revision == revision
    current_timing.assert_not_called()
    assert audio_engine_mock.mock_calls == []


def test_global_speed_and_bpm_anchor_on_other_pad_remain_editable(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(
        PadArrangementController, "fenced_sample_ids", property(lambda _self: frozenset({TARGET}))
    )
    controller.project.selected_pad = SOURCE
    controller.project.sample_paths[SOURCE] = "samples/source.wav"
    controller.project.manual_bpm[SOURCE] = 120.0
    controller.project.bpm_lock = False
    audio_engine_mock.reset_mock()

    controller.transport.global_params.set_bpm_lock(enabled=True)
    controller.transport.global_params.set_speed(1.25)

    assert controller.project.bpm_lock is True
    assert controller.session.bpm_lock_anchor_pad_id == SOURCE
    assert controller.project.speed == 1.25
    audio_engine_mock.set_bpm_lock.assert_called_once_with(enabled=True)
    audio_engine_mock.set_speed.assert_called_once_with(1.25)


def test_disabling_bpm_lock_releases_reserved_anchor_without_target_edit(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(
        PadArrangementController, "fenced_sample_ids", property(lambda _self: frozenset({TARGET}))
    )
    controller.project.bpm_lock = True
    controller.project.selected_pad = TARGET
    controller.session.bpm_lock_anchor_pad_id = TARGET
    audio_engine_mock.reset_mock()

    controller.transport.global_params.set_bpm_lock(enabled=False)

    assert controller.project.bpm_lock is False
    assert controller.session.bpm_lock_anchor_pad_id is None
    audio_engine_mock.set_bpm_lock.assert_called_once_with(enabled=False)
    audio_engine_mock.bootstrap_transport_from_pad.assert_not_called()


def test_own_copy_callback_resets_only_target_runtime_without_native_setters(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _bind_copied_pair(controller)
    expected = controller.project.model_copy(deep=True)
    selection = _selection()
    session = controller.session
    session.active_sample_ids = {SOURCE, TARGET}
    session.paused_sample_ids = {SOURCE, TARGET}
    session.global_stop_restore_sample_ids = {SOURCE, TARGET}
    session.global_stop_engaged = True
    session.waveform_editor_open = True
    session.waveform_editor_pad_id = SOURCE
    session.waveform_pause_hold_pad_id = SOURCE
    session.stem_generation_progress[SOURCE] = 0.75
    session.loading_sample_ids.add(TARGET)
    for sample_id in (SOURCE, TARGET):
        session.pressed_pads[sample_id] = True
        session.pad_peak[sample_id] = 0.5
        session.pad_peak_updated_at[sample_id] = 42.0
        session.pad_clip_hold_until[sample_id] = 43.0
        session.pad_playhead_s[sample_id] = 1.25
        session.pad_playhead_updated_at[sample_id] = 42.0
        controller.metering._active_peak_sample_ids.add(sample_id)
    audio_engine_mock.reset_mock()

    controller._on_copy_committed(TARGET, _ticket(selection=selection))

    assert controller.project.model_dump() == expected.model_dump()
    assert session.active_sample_ids == session.paused_sample_ids == {SOURCE}
    assert session.global_stop_restore_sample_ids == {SOURCE}
    assert session.global_stop_engaged is True
    assert session.pressed_pads[TARGET] is False
    assert session.pad_peak[TARGET] == session.pad_peak_updated_at[TARGET] == 0.0
    assert session.pad_clip_hold_until[TARGET] == session.pad_playhead_updated_at[TARGET] == 0.0
    assert session.pad_playhead_s[TARGET] is None
    assert session.pressed_pads[SOURCE] is True
    assert session.pad_peak[SOURCE] == 0.5
    assert session.pad_peak_updated_at[SOURCE] == session.pad_playhead_updated_at[SOURCE] == 42.0
    assert session.pad_clip_hold_until[SOURCE] == 43.0
    assert session.pad_playhead_s[SOURCE] == 1.25
    assert session.waveform_editor_open is True
    assert session.waveform_editor_pad_id == session.waveform_pause_hold_pad_id == SOURCE
    assert session.stem_generation_progress[SOURCE] == 0.75
    assert session.loading_sample_ids == {TARGET}
    assert controller.metering._active_peak_sample_ids == {SOURCE}
    assert audio_engine_mock.mock_calls == []


@pytest.mark.parametrize("components_available", [False, True])
def test_copy_projects_only_own_component_availability_and_registration(
    controller: AppController, audio_engine_mock: Mock, *, components_available: bool
) -> None:
    entry = _bind_copied_pair(controller)
    expected = controller.project.model_copy(deep=True)
    expected.stem_cache[TARGET] = entry.model_copy(update={"available": components_available})
    selection = _selection()
    audio_engine_mock.reset_mock()

    controller._on_copy_committed(
        TARGET, _ticket(_metadata(components_available=components_available), selection=selection)
    )
    controller._on_copy_committed(
        TARGET, _ticket(_metadata(components_available=components_available), selection=selection)
    )

    assert controller.project.model_dump() == expected.model_dump()
    assert controller.stems.stems_available(TARGET) is components_available
    status = controller.stems.stem_residency_status(TARGET)
    assert status is not None
    assert status.registration == "accepted"
    assert status.residency is status.effective is status.requested_mode is None
    assert audio_engine_mock.mock_calls == []


def test_later_mask_uses_own_copied_target_registration(controller: AppController) -> None:
    _bind_copied_pair(controller)
    selection = _selection()
    audio = cast("Mock", controller._audio)
    audio.register_committed_stem_pair = Mock()
    audio.request_committed_stem_residency = Mock()
    controller._on_copy_committed(
        TARGET, _ticket(_metadata(components_available=True), selection=selection)
    )
    audio.reset_mock()

    assert controller.stems.set_stem_enabled_mask(TARGET, 6) is True

    audio.request_committed_stem_residency.assert_called_once_with(
        selection, mode="all_stems", enabled_mask=6
    )
    audio.register_committed_stem_pair.assert_not_called()
    audio.set_stem_enabled_mask.assert_not_called()


def test_disk_only_copy_can_request_existing_residency_without_claiming_components(
    controller: AppController,
) -> None:
    _bind_copied_pair(controller)
    controller.project.pad_stem_mix_mode[TARGET] = "full_mix"
    selection = _selection()
    audio = cast("Mock", controller._audio)
    audio.register_committed_stem_pair = Mock()
    audio.request_committed_stem_residency = Mock()
    controller._on_copy_committed(TARGET, _ticket(selection=selection))
    audio.reset_mock()

    assert controller.stems.set_stem_mix_mode(TARGET, "all_stems") is True

    audio.request_committed_stem_residency.assert_called_once_with(
        selection, mode="all_stems", enabled_mask=0
    )
    assert controller.project.pad_stem_mix_mode[TARGET] == "all_stems"
    assert controller.stems.stems_available(TARGET) is False
    audio.register_committed_stem_pair.assert_not_called()
    audio.capture_prepared_source.assert_not_called()
    audio.set_stem_mix_mode.assert_not_called()


def test_unavailable_pair_without_own_registration_cannot_request_residency(
    controller: AppController,
) -> None:
    _bind_copied_pair(controller)
    controller.project.pad_stem_mix_mode[TARGET] = "full_mix"
    before = controller.project.model_dump()
    audio = cast("Mock", controller._audio)
    audio.register_committed_stem_pair = Mock()
    audio.request_committed_stem_residency = Mock()
    audio.reset_mock()

    assert controller.stems.set_stem_mix_mode(TARGET, "all_stems") is False

    assert controller.project.model_dump() == before
    assert audio.mock_calls == []


def test_disk_only_copy_unlocks_mask_only_after_own_current_component_receipt(
    controller: AppController,
) -> None:
    _bind_copied_pair(controller)
    controller.project.pad_stem_mix_mode[TARGET] = "full_mix"
    selection = _selection()
    receipt = Mock(
        spec=[
            "is_current",
            "residency_status",
            "effective_mode_status",
            "effective_failure_reason",
            "error",
        ]
    )
    receipt.is_current.return_value = True
    receipt.residency_status.return_value = "pending"
    receipt.effective_mode_status.return_value = "waiting"
    receipt.effective_failure_reason.return_value = None
    receipt.error.return_value = None
    audio = cast("Mock", controller._audio)
    audio.register_committed_stem_pair = Mock()
    audio.request_committed_stem_residency = Mock(return_value=receipt)
    controller._on_copy_committed(TARGET, _ticket(selection=selection))

    assert controller.stems.set_stem_mix_mode(TARGET, "all_stems") is True
    assert controller.stems.stem_mask_controls_enabled(TARGET) is False

    receipt.residency_status.return_value = "accepted"
    assert controller.stems.stem_mask_controls_enabled(TARGET) is True
    receipt.is_current.return_value = False
    assert controller.stems.stem_mask_controls_enabled(TARGET) is False
    receipt.is_current.return_value = True
    assert controller.stems.set_stem_enabled_mask(TARGET, 6) is True

    assert audio.request_committed_stem_residency.call_args_list == [
        call(selection, mode="all_stems", enabled_mask=0),
        call(selection, mode="all_stems", enabled_mask=6),
    ]
    current = controller.project.stem_cache[TARGET]
    assert current is not None
    assert current.available is False
    audio.register_committed_stem_pair.assert_not_called()
    audio.set_stem_enabled_mask.assert_not_called()


@pytest.mark.parametrize(
    "override",
    [
        {"target_id": SOURCE},
        {"target_id": True},
        {"source_generation": 0},
        {"source_generation": True},
        {"content_lifetime": 0},
        {"status": "pending"},
        {"control_mirrored": False},
        {"components_available": 1},
    ],
)
def test_copy_metadata_failure_cannot_project_runtime_or_availability(
    controller: AppController, audio_engine_mock: Mock, override: dict[str, object]
) -> None:
    _bind_copied_pair(controller)
    controller.session.active_sample_ids = {SOURCE, TARGET}
    before = controller.project.model_dump()
    session = controller.session.model_dump()
    audio_engine_mock.reset_mock()

    with pytest.raises(RuntimeError, match="no matching acknowledged Native image"):
        controller._on_copy_committed(TARGET, _ticket(_metadata(**override)))

    assert controller.project.model_dump() == before
    assert controller.session.model_dump() == session
    assert not controller.stems._committed_residency.contains(TARGET)
    assert audio_engine_mock.mock_calls == []


def test_missing_copy_metadata_is_a_retained_callback_failure(controller: AppController) -> None:
    controller.session.active_sample_ids = {TARGET}
    missing = cast("PadCopyTicket", Mock(spec=[]))

    with pytest.raises(RuntimeError, match="acknowledgement metadata is unavailable"):
        controller._on_copy_committed(TARGET, missing)

    assert controller.session.active_sample_ids == {TARGET}


@pytest.mark.parametrize(
    "selection", [_selection(state="pending"), _selection(current=False), None]
)
def test_copied_pair_requires_own_current_accepted_registration(
    controller: AppController, selection: CommittedStemSetTicket | None
) -> None:
    _bind_copied_pair(controller)
    controller.session.active_sample_ids = {TARGET}
    before = controller.project.model_dump()

    with pytest.raises(RuntimeError, match="target registration"):
        controller._on_copy_committed(
            TARGET, _ticket(_metadata(components_available=True), selection=selection)
        )

    assert controller.project.model_dump() == before
    assert controller.session.active_sample_ids == {TARGET}
    assert not controller.stems._committed_residency.contains(TARGET)


def test_no_stems_copy_needs_no_selection_receipt(controller: AppController) -> None:
    before = controller.project.model_dump()
    controller.session.active_sample_ids = {TARGET}

    controller._on_copy_committed(TARGET, _ticket())

    assert controller.session.active_sample_ids == set()
    assert controller.project.model_dump() == before
    assert not controller.stems._committed_residency.contains(TARGET)


def _observe_shutdown(controller: AppController, monkeypatch: pytest.MonkeyPatch) -> Mock:
    """Observe controller orchestration without substituting a Native settlement proof."""
    observed = Mock()
    hooks = (
        ("disable_input", controller._audio, "set_input_mapping_enabled"),
        ("prepare", controller.arrangement, "prepare_shutdown"),
        ("migration", controller.material_migration, "shut_down"),
        ("residency", controller.transport.residency, "shut_down"),
        ("timing", controller.accepted_timing, "shut_down"),
        ("loader", controller.loader, "shut_down"),
        ("stems", controller.stems, "shut_down"),
        ("flush", controller.persistence, "flush"),
        ("stop", controller._audio, "stop_all"),
        ("release_assets", controller._assets, "release_saved_assignments"),
        ("native_shutdown", controller._audio, "shut_down"),
    )
    for name, owner, method in hooks:
        hook = Mock(name=name)
        observed.attach_mock(hook, name)
        monkeypatch.setattr(owner, method, hook)
    return observed


def _shutdown_order() -> list[object]:
    return [
        call.migration(),
        call.residency(),
        call.timing(),
        call.loader(),
        call.stems(),
        call.flush(),
        call.stop(),
        call.release_assets(),
        call.native_shutdown(),
    ]


def test_shutdown_retains_every_owner_until_copy_settles_then_tears_down_once(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    observed = _observe_shutdown(controller, monkeypatch)
    mapping_enabled = False
    observed.prepare.return_value = False
    controller.project.input_mapping_enabled = True
    project = controller.project.model_dump()
    session = controller.session.model_dump()

    assert controller.prepare_shutdown() is False
    with pytest.raises(RuntimeError, match="waiting for complete Copy settlement"):
        controller.shut_down()

    assert observed.mock_calls == [
        call.disable_input(mapping_enabled),
        call.prepare(),
        call.prepare(),
    ]
    assert controller.project.model_dump() == project
    assert controller.session.model_dump() == session
    assert controller._shutdown_running is False
    assert controller._shutdown_preparing is False
    assert controller._shutdown_complete is False

    observed.prepare.return_value = True
    controller.shut_down()

    assert observed.mock_calls == [
        call.disable_input(mapping_enabled),
        call.prepare(),
        call.prepare(),
        call.prepare(),
        *_shutdown_order(),
    ]
    assert controller._shutdown_complete is True
    assert controller._shutdown_running is False
    assert controller.project.input_mapping_enabled is True

    observed.reset_mock()
    assert controller.prepare_shutdown() is True
    controller.shut_down()

    assert observed.mock_calls == []


@pytest.mark.parametrize("reentry_phase", ["prepare", "migration"])
def test_reentrant_shutdown_cannot_start_another_settlement_or_teardown(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, reentry_phase: str
) -> None:
    observed = _observe_shutdown(controller, monkeypatch)
    mapping_enabled = False
    observed.prepare.return_value = True

    def reenter_shutdown() -> bool:
        with pytest.raises(RuntimeError, match="shutdown is already running"):
            controller.shut_down()
        return True

    getattr(observed, reentry_phase).side_effect = reenter_shutdown

    controller.shut_down()

    assert observed.mock_calls == [
        call.disable_input(mapping_enabled),
        call.prepare(),
        *_shutdown_order(),
    ]
    assert controller._shutdown_complete is True
    assert controller._shutdown_running is False


@pytest.mark.parametrize("nested_call", ["prepare_shutdown", "shut_down"])
def test_direct_shutdown_preparation_rejects_nested_preparation_and_keeps_owners_alive(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, nested_call: str
) -> None:
    observed = _observe_shutdown(controller, monkeypatch)
    mapping_enabled = False

    def reenter_preparation() -> bool:
        with pytest.raises(RuntimeError, match="shutdown preparation is already running"):
            getattr(controller, nested_call)()
        return False

    observed.prepare.side_effect = reenter_preparation

    assert controller.prepare_shutdown() is False

    assert observed.mock_calls == [call.disable_input(mapping_enabled), call.prepare()]
    assert controller._shutdown_complete is False
    assert controller._shutdown_running is False
    assert controller._shutdown_preparing is False
