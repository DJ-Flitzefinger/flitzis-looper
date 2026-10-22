"""Requested ordinary intent waits for content-owned Native observation truth."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.conftest import FakeOrdinaryTransportAdmission

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import OrdinaryTransportObservation


@dataclass(slots=True, kw_only=True)
class PauseObservation:
    """Controller routing double; identity matching substitutes no Native/PCM proof."""

    ordinary_paused: bool
    observation_sequence: int = 1
    sample_id: int = 0
    content_lifetime: int = 1
    transport_revision: int = 1
    active: bool = True
    status_kind: str = "transport"
    requested_effect: str | None = "pause"
    outcome: str | None = "applied"
    settled: FakeOrdinaryTransportAdmission | None = None
    direct: FakeOrdinaryTransportAdmission | None = None
    current: PauseObservation | None = None
    source: FakeOrdinaryTransportAdmission | None = None

    def matches_admission(self, admission: FakeOrdinaryTransportAdmission) -> bool:
        return admission is self.direct

    def settles_admission(self, admission: FakeOrdinaryTransportAdmission) -> bool:
        return admission is self.settled

    def follow_current_source(self, source: FakeOrdinaryTransportAdmission) -> None:
        self.source = source

    def current_control_slot(self) -> int | None:
        if self.source is not None:
            return self.source.current_control_slot()
        return self.sample_id

    def current_observation(self) -> object | None:
        if self.source is not None:
            return self.source.current_observation()
        return self.current.current_observation() if self.current is not None else self


def _project(controller: AppController, observation: PauseObservation) -> None:
    controller._audio._current_ordinary_transport_observation.side_effect = lambda sample_id: (
        observation.current_observation() if sample_id == observation.sample_id else None
    )
    controller.transport.playback.handle_ordinary_transport_observation(
        cast("OrdinaryTransportObservation", observation)
    )


def _admissions(controller: AppController, audio: Mock) -> list[FakeOrdinaryTransportAdmission]:
    controller.project.sample_paths[0] = "/synthetic/pause.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    controller.session.active_sample_ids.add(0)
    receipts: list[FakeOrdinaryTransportAdmission] = []

    def admit(sample_id: int, *, paused: bool) -> FakeOrdinaryTransportAdmission:
        receipt = FakeOrdinaryTransportAdmission(
            slot=sample_id, requested_paused=paused, transport_revision=len(receipts) + 1
        )
        receipts.append(receipt)
        return receipt

    audio.pause_sample.side_effect = lambda sample_id: admit(sample_id, paused=True)
    audio.resume_sample.side_effect = lambda sample_id: admit(sample_id, paused=False)

    def current(sample_id: int) -> object | None:
        receipt = next((row for row in reversed(receipts) if row.slot == sample_id), None)
        return receipt.current_observation() if receipt is not None else None

    audio._current_ordinary_transport_observation.side_effect = current
    return receipts


def test_fast_three_clicks_preserve_requested_intent_without_optimistic_session(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback

    playback.toggle_pause_pad(0)
    playback.toggle_pause_pad(0)
    playback.toggle_pause_pad(0)

    assert [receipt.requested_paused for receipt in receipts] == [True, False, True]
    assert audio_engine_mock.pause_sample.call_count == 2
    audio_engine_mock.resume_sample.assert_called_once_with(0)
    assert controller.session.paused_sample_ids == set()
    assert playback._pause_intent.requested_paused(0) is True


def test_old_matching_boolean_and_intermediate_ack_cannot_settle_newer_resume(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.toggle_pause_pad(0)
    playback.toggle_pause_pad(0)
    latest = receipts[-1]
    latest.observation = PauseObservation(
        ordinary_paused=False,
        settled=FakeOrdinaryTransportAdmission(slot=0, requested_paused=False),
    )
    playback.poll_pause_observations()
    latest.observation = PauseObservation(
        ordinary_paused=True, observation_sequence=2, settled=receipts[0]
    )
    playback.poll_pause_observations()
    assert controller.session.paused_sample_ids == {0}

    playback.toggle_pause_pad(0)

    assert [receipt.requested_paused for receipt in receipts] == [True, False, True]


def test_refused_pause_settles_through_seek_frontier_before_next_toggle(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    refused = receipts[-1]
    seek = PauseObservation(
        ordinary_paused=False, transport_revision=2, requested_effect="seek", settled=refused
    )
    assert seek.matches_admission(refused) is False
    refused.observation = seek

    playback.toggle_pause_pad(0)

    assert [receipt.requested_paused for receipt in receipts] == [True, True]
    assert controller.session.paused_sample_ids == set()


def test_applied_pause_then_seek_projects_before_settlement_and_next_click(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    receipts[0].observation = PauseObservation(
        ordinary_paused=True, transport_revision=2, requested_effect="seek", settled=receipts[0]
    )

    playback.toggle_pause_pad(0)

    assert controller.session.paused_sample_ids == {0}
    assert [receipt.requested_paused for receipt in receipts] == [True, False]


def test_settled_monitor_projects_lifecycle_without_generic_started_stopped_events(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    receipt = receipts[0]
    receipt.observation = PauseObservation(ordinary_paused=True, settled=receipt)
    playback.poll_pause_observations()
    assert controller.session.paused_sample_ids == {0}
    assert len(playback._pause_intent._owners) == 1
    receipt.observation = PauseObservation(
        ordinary_paused=False,
        observation_sequence=2,
        status_kind="started",
        requested_effect=None,
        outcome=None,
        transport_revision=0,
        settled=receipt,
    )

    playback.toggle_pause_pad(0)

    assert controller.session.paused_sample_ids == set()
    assert [owner.requested_paused for owner in receipts] == [True, True]
    receipts[-1].observation = PauseObservation(
        ordinary_paused=False,
        active=False,
        observation_sequence=3,
        status_kind="stopped",
        requested_effect=None,
        outcome=None,
        transport_revision=0,
        settled=receipts[-1],
    )
    playback.poll_pause_observations()
    assert controller.session.active_sample_ids == set()
    assert controller.session.paused_sample_ids == set()


def test_enqueue_failure_keeps_previous_owner_and_confirmed_state(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    audio_engine_mock.resume_sample.side_effect = RuntimeError("control queue full")

    with pytest.raises(RuntimeError, match="control queue full"):
        playback.resume_pad(0)

    assert playback._pause_intent.requested_paused(0) is True
    assert len(receipts) == 1
    assert controller.session.paused_sample_ids == set()


def test_move_pending_then_swap_follow_original_content_without_slot_remap(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    receipt = receipts[0]
    receipt.error = "content feedback placement pending"
    playback.poll_pause_observations()
    identity = controller.project.pad_content[0]
    controller.project.pad_content[0] = None
    controller.project.pad_content[215] = identity
    controller.session.active_sample_ids = {215}
    receipt.error = None
    receipt.slot = 215
    receipt.observation = PauseObservation(ordinary_paused=True, sample_id=215, settled=receipt)
    playback.poll_pause_observations()
    assert controller.session.paused_sample_ids == {215}
    assert set(playback._pause_intent._owners) == {identity.instance_id}

    other = PadContentIdentity(instance_id="b" * 32)
    controller.project.pad_content[0] = identity
    controller.project.pad_content[215] = other
    controller.session.active_sample_ids = {0, 215}
    controller.session.paused_sample_ids = {215}
    receipt.slot = 0
    receipt.observation = PauseObservation(
        ordinary_paused=True, sample_id=0, observation_sequence=2
    )
    playback.poll_pause_observations()
    assert controller.session.paused_sample_ids == {0, 215}
    assert set(playback._pause_intent._owners) == {identity.instance_id}


def test_source_replacement_and_unload_evict_only_original_owner(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    original = controller.project.pad_content[0]
    controller.project.pad_content[1] = PadContentIdentity(instance_id="b" * 32)
    controller.session.active_sample_ids.add(1)
    playback.pause_pad(1)
    controller.project.pad_content[0] = PadContentIdentity(instance_id="c" * 32)
    playback.poll_pause_observations()
    assert set(playback._pause_intent._owners) == {"b" * 32}
    assert original.instance_id not in playback._pause_intent._owners
    controller._on_sample_unloaded(1)
    assert not playback._pause_intent._owners
    # Same content with a new semantic Native source also loses its old request.
    playback.pause_pad(0)
    receipts[-1].slot = None
    playback.poll_pause_observations()
    assert not playback._pause_intent._owners


def test_pending_and_terminal_unavailable_never_invent_completion(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    receipts[0].error = "ordinary transport observation unavailable"
    playback.poll_pause_observations()
    assert controller.session.paused_sample_ids == set()
    assert playback._pause_intent._owners["a" * 32].pending is True
    with pytest.raises(RuntimeError, match="ordinary transport observation unavailable"):
        playback.toggle_pause_pad(0)
    assert len(receipts) == 1


def test_current_source_guard_precedes_native_admission(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _admissions(controller, audio_engine_mock)
    controller.session.loading_sample_ids.add(0)
    with pytest.raises(ValueError, match="Source assignment receipt is pending"):
        controller.transport.playback.toggle_pause_pad(0)
    audio_engine_mock.pause_sample.assert_not_called()


def test_hold_display_does_not_replace_ordinary_pause_intent(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    controller.session.waveform_pause_hold_pad_id = 0
    controller.transport.playback.toggle_pause_pad(0)
    assert [receipt.requested_paused for receipt in receipts] == [True]
    audio_engine_mock.resume_sample.assert_not_called()
    assert controller.session.paused_sample_ids == set()


def test_older_observation_cannot_overwrite_projected_current_state(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    controller.transport.playback.pause_pad(0)
    _project(
        controller,
        PauseObservation(ordinary_paused=True, observation_sequence=2, settled=receipts[0]),
    )
    _project(controller, PauseObservation(ordinary_paused=False, observation_sequence=1))
    assert controller.session.paused_sample_ids == {0}


def test_monitors_are_bounded_by_living_content_and_never_persisted(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    for sample_id in range(len(controller.project.pad_content)):
        controller.project.pad_content[sample_id] = PadContentIdentity(
            instance_id=f"{sample_id + 1:032x}"
        )
        controller.session.active_sample_ids.add(sample_id)
        playback.pause_pad(sample_id)
    assert len(playback._pause_intent._owners) == 216
    controller.project.pad_content[0] = None
    playback.poll_pause_observations()
    assert len(playback._pause_intent._owners) == 215
    assert "pause_intent" not in controller.project.model_dump()
    assert "paused_sample_ids" not in controller.project.model_dump()


@pytest.mark.parametrize("assignment_fence", ["loading", "incoming"])
def test_pending_assignment_fences_durable_queries_and_settlement_before_native_access(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    assignment_fence: str,
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    receipt = receipts[0]
    receipt.observation = PauseObservation(ordinary_paused=True, settled=receipt)
    blocked = {0}
    if assignment_fence == "loading":
        controller.session.loading_sample_ids.add(0)
    else:
        monkeypatch.setattr(
            controller.loader,
            "incoming_load_request_id",
            lambda sample_id: 17 if sample_id in blocked else None,
        )

    controller._poll_audio_messages()

    audio_engine_mock._current_ordinary_transport_observation.assert_not_called()
    assert receipt.slot_queries == 0
    assert receipt.observation_queries == 0
    assert controller.session.paused_sample_ids == set()
    assert playback._pause_intent._owners["a" * 32].pending is True
    controller.session.loading_sample_ids.discard(0)
    blocked.clear()
    controller._poll_audio_messages()
    # The bounded current snapshot query does not use the predecessor receipt;
    # its pending admission membership is independently checked at projection.
    assert receipt.slot_queries == 1
    assert receipt.observation_queries == 1
    assert controller.session.paused_sample_ids == {0}
    assert playback._pause_intent._owners["a" * 32].pending is False


def test_new_source_observation_replaces_old_monitor_and_request_at_same_lifetime(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    old_admission = receipts[0]
    # Native invalidated the old semantic source while preserving the content
    # lifetime. Its new-source observation arrives before the old owner is polled.
    old_admission.slot = None
    current = PauseObservation(
        ordinary_paused=False,
        observation_sequence=2,
        content_lifetime=1,
        status_kind="source_adopted",
        requested_effect=None,
        outcome=None,
        transport_revision=0,
    )

    _project(controller, current)
    playback.poll_pause_observations()

    owner = playback._pause_intent._owners["a" * 32]
    assert owner.monitor is current
    assert owner.admission is None
    assert owner.pending is False
    assert current.content_lifetime == old_admission.content_lifetime
    assert old_admission.observation_queries == 0
    assert playback._pause_intent.requested_paused(0) is False

    current.current = PauseObservation(
        ordinary_paused=True,
        observation_sequence=3,
        requested_effect="seek",
        content_lifetime=1,
    )
    playback.poll_pause_observations()
    assert controller.session.paused_sample_ids == {0}
    latest = current.current
    latest.current = PauseObservation(
        ordinary_paused=False,
        observation_sequence=4,
        status_kind="started",
        requested_effect=None,
        outcome=None,
        transport_revision=0,
    )

    playback.toggle_pause_pad(0)

    assert controller.session.paused_sample_ids == set()
    assert [receipt.requested_paused for receipt in receipts] == [True, True]
    audio_engine_mock.resume_sample.assert_not_called()


def test_refreshed_monitor_keeps_valid_exact_pending_admission_until_own_frontier(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    pending = receipts[0]
    prior = PauseObservation(ordinary_paused=False, observation_sequence=2, requested_effect="seek")

    _project(controller, prior)

    owner = playback._pause_intent._owners["a" * 32]
    assert owner.monitor is prior
    assert owner.admission is pending
    assert owner.pending is True
    assert playback._pause_intent.requested_paused(0) is True
    prior.current = PauseObservation(
        ordinary_paused=False,
        observation_sequence=3,
        requested_effect="pause",
        outcome="refused",
        settled=pending,
    )
    playback.poll_pause_observations()
    assert owner.pending is False
    assert owner.admission is None
    assert controller.session.paused_sample_ids == set()


def test_warm_new_source_snapshot_bootstraps_without_raw_token_or_old_monitor_query(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    predecessor = receipts[0]
    predecessor.slot = None
    warm_source = PauseObservation(
        ordinary_paused=True,
        observation_sequence=3,
        content_lifetime=1,
        status_kind="source_adopted",
        requested_effect=None,
        outcome=None,
        transport_revision=0,
    )
    audio_engine_mock._current_ordinary_transport_observation.side_effect = lambda sample_id: (
        warm_source.current_observation() if sample_id == 0 else None
    )

    controller._poll_audio_messages()

    assert controller.session.paused_sample_ids == {0}
    owner = playback._pause_intent._owners["a" * 32]
    assert owner.monitor is warm_source
    assert owner.admission is None
    assert owner.pending is False
    assert predecessor.observation_queries == 0
    warm_source.current = PauseObservation(
        ordinary_paused=False,
        observation_sequence=4,
        status_kind="started",
        requested_effect=None,
        outcome=None,
        transport_revision=0,
    )
    playback.toggle_pause_pad(0)
    assert controller.session.paused_sample_ids == set()
    assert [receipt.requested_paused for receipt in receipts] == [True, True]
    audio_engine_mock.resume_sample.assert_not_called()


def test_first_seek_and_lost_restart_are_observed_without_prior_owner_or_raw_event(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "/synthetic/first-seek.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    controller.session.active_sample_ids.add(0)
    playback = controller.transport.playback
    seek = PauseObservation(ordinary_paused=True, requested_effect="seek")
    audio_engine_mock._current_ordinary_transport_observation.side_effect = lambda sample_id: (
        seek.current_observation() if sample_id == 0 else None
    )
    assert not playback._pause_intent._owners

    controller._poll_audio_messages()

    assert controller.session.paused_sample_ids == {0}
    assert playback._pause_intent._owners["a" * 32].admission is None
    seek.current = PauseObservation(
        ordinary_paused=False,
        observation_sequence=2,
        status_kind="started",
        requested_effect=None,
        outcome=None,
        transport_revision=0,
    )
    playback.toggle_pause_pad(0)
    assert controller.session.paused_sample_ids == set()
    audio_engine_mock.pause_sample.assert_called_once_with(0)
    audio_engine_mock.resume_sample.assert_not_called()
    audio_engine_mock._resolve_ordinary_transport_feedback.assert_not_called()


def test_no_current_snapshot_preserves_exact_pending_until_membership_is_actually_invalid(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    receipts = _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    playback.pause_pad(0)
    audio_engine_mock._current_ordinary_transport_observation.side_effect = None
    audio_engine_mock._current_ordinary_transport_observation.return_value = None
    owner = playback._pause_intent._owners["a" * 32]

    playback.poll_pause_observations()

    assert playback._pause_intent._owners["a" * 32] is owner
    assert owner.admission is receipts[0]
    assert owner.pending is True
    assert playback._pause_intent.requested_paused(0) is True
    receipts[0].slot = None
    playback.poll_pause_observations()
    assert not playback._pause_intent._owners
    assert controller.session.paused_sample_ids == set()


def test_current_snapshot_rechecks_returned_slot_and_captured_python_content(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _admissions(controller, audio_engine_mock)
    playback = controller.transport.playback
    audio_engine_mock._current_ordinary_transport_observation.side_effect = None
    audio_engine_mock._current_ordinary_transport_observation.return_value = PauseObservation(
        ordinary_paused=True, sample_id=1
    )
    playback.poll_pause_observations()
    assert controller.session.paused_sample_ids == set()
    assert not playback._pause_intent._owners

    def replace_during_query(sample_id: int) -> PauseObservation:
        controller.project.pad_content[sample_id] = PadContentIdentity(instance_id="b" * 32)
        return PauseObservation(ordinary_paused=True, sample_id=sample_id)

    audio_engine_mock._current_ordinary_transport_observation.side_effect = replace_during_query
    playback.poll_pause_observations()
    assert controller.session.paused_sample_ids == set()
    assert not playback._pause_intent._owners
