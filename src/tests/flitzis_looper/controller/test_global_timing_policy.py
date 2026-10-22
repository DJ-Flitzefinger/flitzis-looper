"""Global control, existing runtime publication and actual-feedback fences."""

import json
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.input_mapping.actions import (
    LooperAction,
    global_realtime_bpm_toggle_action,
    global_source_sync_toggle_action,
)
from flitzis_looper.input_mapping.storage import load_midi_mapping_file
from flitzis_looper.source_sync_intent import PadSourceSyncIntent
from flitzis_looper.timing_policy import GlobalTimingPolicy
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.test_source_beatmap_selection import _assigned
from tests.flitzis_looper.controller.test_source_sync import _feedback
from tests.flitzis_looper.controller.transport.test_source_map_loop import _bind_map, _geometry

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def _policy(*, sync: bool | None, realtime: bool | None, revision: int = 2) -> GlobalTimingPolicy:
    return GlobalTimingPolicy(schema_version=1, sync=sync, realtime_bpm=realtime, revision=revision)


def test_global_choices_publish_one_revision_without_touching_pad_or_other_controls(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    before = controller.project.model_dump(exclude={"global_timing_policy"})
    controller.transport.timing_policy.set_enabled("sync", enabled=True)
    first = controller.project.global_timing_policy
    assert first.sync is True
    assert first.realtime_bpm is None
    assert first.revision == 1
    assert controller.project.model_dump(exclude={"global_timing_policy"}) == before
    kwargs = audio_engine_mock.set_input_runtime_state.call_args.kwargs
    assert json.loads(kwargs["timing_policy_json"]) == first.model_dump()
    assert kwargs["source_map_rows"][0]["sync_requested"] is True
    assert kwargs["source_map_rows"][0]["intent_revision"] == 0
    controller.transport.timing_policy.set_enabled("realtime_bpm", enabled=False)
    assert controller.project.global_timing_policy == _policy(sync=True, realtime=False)
    assert controller.project.model_dump(exclude={"global_timing_policy"}) == before


def test_inherited_pad_off_creates_explicit_preference_and_global_mask_does_not_change_toggle(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.transport.timing_policy.set_enabled("sync", enabled=True)
    controller.transport.source_sync.toggle(0)
    assert controller.project.pad_source_sync_intent[0] == PadSourceSyncIntent(
        requested=False, revision=1
    )
    controller.transport.timing_policy.set_enabled("sync", enabled=False)
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=4)
    controller.transport.source_sync.toggle(0)
    assert controller.project.pad_source_sync_intent[0] == PadSourceSyncIntent(
        requested=False, revision=5
    )
    assert controller.project.global_timing_policy.sync is False


@pytest.mark.parametrize(
    ("sync", "realtime", "mode"),
    [
        (False, False, "off"),
        (False, True, "off"),
        (True, False, "fixed"),
        (True, True, "mapped"),
    ],
)
def test_all_explicit_choices_use_same_resolved_runtime_mode_without_control_mutation(
    controller: AppController,
    audio_engine_mock: Mock,
    *,
    sync: bool,
    realtime: bool,
    mode: str,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.global_timing_policy = _policy(sync=sync, realtime=realtime)
    before = controller.project.model_dump()
    assert controller.transport.source_sync.requested_mode(0) == mode
    assert controller.input_mapping._refresh_rust_runtime_state()
    row = audio_engine_mock.set_input_runtime_state.call_args.kwargs["source_map_rows"][0]
    assert row["sync_requested"] is sync
    assert row["intent_revision"] == 0
    assert row["delivery"] is None
    assert controller.project.model_dump() == before


def test_realtime_choice_alone_keeps_unsynchronized_legacy_preference(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.global_timing_policy = _policy(sync=None, realtime=False)
    assert controller.transport.source_sync.requested_mode(0) == "legacy"
    controller.project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=0)
    assert controller.transport.source_sync.requested_mode(0) == "fixed"


def test_fixed_actual_settlement_needs_no_map_and_cannot_borrow_old_global_ack(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.global_timing_policy = _policy(sync=True, realtime=False)
    monkeypatch.setattr(
        controller.loader.source_beatmaps, "effective", lambda _id: pytest.fail("fixed map access")
    )
    feedback = _feedback(
        audio_engine_mock,
        active=True,
        effective=True,
        intent_revision=0,
        map_revision=None,
        policy_mode="fixed",
        policy_revision=2,
        settled=True,
        policy_sync=True,
        policy_realtime_bpm=False,
    )
    assert controller.transport.source_sync.status(0).state == "effective"
    feedback.update(policy_revision=1)
    assert controller.transport.source_sync.status(0).state == "pending"
    feedback.update(policy_revision=2, policy_sync=False)
    assert controller.transport.source_sync.status(0).state == "pending"
    feedback.update(policy_sync=True, effective=False, settled=False)
    assert controller.transport.source_sync.status(0).state == "pending"


def test_global_off_ack_uses_full_actual_policy_while_retaining_old_active_audio(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.global_timing_policy = _policy(sync=False, realtime=False, revision=0)
    feedback = _feedback(
        audio_engine_mock,
        active=True,
        effective=False,
        intent_revision=0,
        map_revision=None,
        policy_mode="off",
        policy_revision=0,
        settled=True,
        policy_sync=None,
        policy_realtime_bpm=None,
    )
    assert controller.transport.source_sync.status(0).state == "pending"
    feedback.update(policy_sync=False, policy_realtime_bpm=False)
    assert controller.transport.source_sync.status(0).state == "off"
    feedback.update(policy_mode="mapped", effective=True, map_revision="a" * 64)
    state = controller.transport.source_sync.status(0)
    assert state.state == "pending"
    assert state.effective is True


@pytest.mark.parametrize("sync", [True, False])
@pytest.mark.parametrize("active", [True, False])
def test_fixed_or_off_preparation_refusal_is_diagnosed_without_changing_actual_audio(
    controller: AppController,
    audio_engine_mock: Mock,
    *,
    sync: bool,
    active: bool,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.global_timing_policy = _policy(sync=sync, realtime=False)
    diagnostics = Mock()
    controller.transport.source_sync.bind_diagnostics(diagnostics)
    feedback = _feedback(
        audio_engine_mock,
        active=active,
        effective=active,
        intent_revision=0,
        map_revision="a" * 64 if active else None,
        policy_mode="mapped" if active else None,
        policy_revision=0,
        policy_sync=None,
        policy_realtime_bpm=None,
        settled=active,
    )
    reason = (
        "source preparation requires an exact finite supported tempo ratio"
        if sync
        else "captured pad start preparation unavailable"
    )
    controller.transport.residency.report_launch_error(0, reason)

    status = controller.transport.source_sync.status(0)

    assert status.state == "refused"
    assert status.detail == reason
    assert status.requested is sync
    assert status.active is active
    assert status.effective is active
    assert status.policy_mode == ("mapped" if active else None)
    assert status.actual_realtime_bpm is (True if active else None)
    diagnostics.observe.assert_called_once_with("source_sync", reason, pad_id=0, request=0)

    # A matching actual settlement takes precedence over a retained earlier launch error.
    feedback.update(
        active=True,
        effective=sync,
        map_revision=None,
        policy_mode="fixed" if sync else "off",
        policy_revision=2,
        policy_sync=sync,
        policy_realtime_bpm=False,
        settled=True,
    )
    assert controller.transport.source_sync.status(0).state == ("effective" if sync else "off")


def test_changed_global_policy_waits_for_whole_reserved_cohort_then_retries(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    assert controller.input_mapping._refresh_rust_runtime_state()
    previous = controller.input_mapping._last_input_runtime_state_signature
    controller.transport._pair_deferred_sample_ids = frozenset({0})
    audio_engine_mock.reset_mock()
    with pytest.raises(RuntimeError, match="not ready"):
        controller.transport.timing_policy.set_enabled("sync", enabled=False)
    assert controller.project.global_timing_policy.sync is False
    assert controller.input_mapping._last_input_runtime_state_signature is previous
    audio_engine_mock.set_input_runtime_state.assert_not_called()
    controller.transport._pair_deferred_sample_ids = frozenset()
    assert controller.input_mapping._refresh_rust_runtime_state()
    assert (
        json.loads(
            audio_engine_mock.set_input_runtime_state.call_args.kwargs["timing_policy_json"]
        )["sync"]
        is False
    )


@pytest.mark.parametrize(
    ("method", "action", "control"),
    [
        ("toggle_source_sync", global_source_sync_toggle_action(), "sync"),
        ("toggle_realtime_bpm", global_realtime_bpm_toggle_action(), "realtime_bpm"),
    ],
)
def test_gui_learn_and_midi_share_each_global_action_without_early_mutation(
    controller: AppController,
    audio_engine_mock: Mock,
    method: str,
    action: LooperAction,
    control: str,
) -> None:
    _assigned(controller, audio_engine_mock)
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)
    ctx.input.toggle_learn()
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
    })
    getattr(ctx.audio.global_, method)()
    assert controller.project.global_timing_policy.is_neutral
    assert load_midi_mapping_file().mappings[0].action == action
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": action.key,
    })
    assert getattr(controller.project.global_timing_policy, control) is (control == "sync")


def test_fixed_auto_loop_uses_initial_period_and_retains_map_without_variable_marker_actions(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.manual_bpm[0] = 120.0
    controller.project.sample_durations[0] = 10.0
    controller.project.pad_loop_start_s[0] = 1.0
    controller.project.pad_loop_bars[0] = 1.0
    controller.project.pad_loop_auto[0] = True
    selection = _bind_map(controller, _geometry())
    controller.project.global_timing_policy = _policy(sync=True, realtime=False)
    assert controller.transport.loop.requested_region(0) == (1.0, 3.0)
    with pytest.raises(ValueError, match="Realtime BPM is OFF"):
        controller.transport.loop.apply_source_map_bars(0)
    assert selection.is_selected(0)
    assert controller.project.pad_loop_auto[0] is True
    controller.project.global_timing_policy = _policy(sync=False, realtime=True, revision=3)
    assert controller.transport.loop.requested_region(0) == (1.0, 3.5)
    selection.effective.return_value = None
    controller.project.pad_loop_end_s[0] = 3.5
    assert controller.transport.loop.requested_region(0) == (1.0, 3.5)
    assert controller.project.pad_loop_auto[0] is True
    controller.project.global_timing_policy = _policy(sync=True, realtime=True, revision=4)
    with pytest.raises(ValueError, match="qualified source map"):
        controller.transport.loop.requested_region(0)
