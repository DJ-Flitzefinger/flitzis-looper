"""Scripted own receipts exercise actual StemController frame ordering, without audio."""

from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, TypedDict, cast
from unittest.mock import Mock, call

import pytest

from flitzis_looper.controller.committed_stem_residency import WithdrawnStemIntent
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.models import PadContentIdentity, StemCacheEntry
from flitzis_looper.stem_intent import PadStemIntent
from flitzis_looper.stem_pair_selection import StemPairSelection

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.models import StemMaskDisplayMode, StemMixMode
    from flitzis_looper_audio import PreparedSourceTicket, PreparedStemPair


class _WithdrawnChange(TypedDict, total=False):
    content_id: str | None
    source_path: str | None
    source_version: str
    stem_set_identity: str | None
    mode: StemMixMode
    enabled_mask: int


@dataclass(slots=True)
class _Receipt:
    residency: str = "accepted"
    effective: str = "waiting"
    current: bool = True
    failure_reason: str | None = None
    cancellations: int = 0

    def residency_status(self) -> str:
        return self.residency

    def effective_mode_status(self) -> str:
        return self.effective if self.current else "rejected"

    def effective_failure_reason(self) -> str | None:
        return self.failure_reason

    def is_current(self) -> bool:
        return self.current

    def error(self) -> str | None:
        return "actual Native preparation failed" if self.failure_reason else None

    def claimed(self) -> bool:
        return self.residency == "accepted"

    def cancel(self) -> None:
        self.cancellations += 1


def _bind(
    controller: AppController,
    audio: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    active: bool,
    effective: str = "waiting",
) -> tuple[_Receipt, Mock, Mock]:
    root = f"samples/materials/M{'a' * 32}"
    pair = StemPairSelection(
        descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{'b' * 32}.json",
        stem_set_identity="b" * 64,
        wav_generation=f"{root}/stems/.ready-{'b' * 32}",
        pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{'b' * 32}",
    )
    source_path = f"{root}/original/source.wav"
    entry = StemCacheEntry(
        source_version=f"{source_path}|sha256-v1:{'1' * 64}",
        cache_dir=pair.wav_generation,
        stems=expected_stem_files(pair.wav_generation),
        pair=pair,
        available=True,
    )
    controller.project.sample_paths[0] = source_path
    controller.project.pad_content[0] = PadContentIdentity(instance_id="f" * 32)
    controller.project.stem_cache[0] = entry
    controller.project.pad_stem_intent[0] = PadStemIntent(enabled_mask=10, display_mode="custom")
    controller.project.preload_stem_loops_on_startup = False
    registration = Mock(**{
        "publication_status.return_value": "accepted",
        "is_current.return_value": True,
    })
    for name in ("register_committed_stem_pair", "register_pending_committed_stem_pair"):
        monkeypatch.setattr(audio, name, Mock(return_value=registration), raising=False)
    receipt = _Receipt(effective=effective)
    request = Mock(return_value=receipt)
    monkeypatch.setattr(audio, "request_committed_stem_residency", request, raising=False)
    controller.stems._committed_residency.register(
        0,
        cast("PreparedStemPair", Mock()),
        cast("PreparedSourceTicket", Mock()),
        entry,
        "f" * 32,
        source_path,
        active=active,
    )
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    assert controller.stems._demands[0].explicit
    request.assert_called_once_with(registration, mode="all_stems", enabled_mask=10)
    return receipt, registration, request


@pytest.mark.parametrize("active", [False, True])
@pytest.mark.parametrize("effective", ["waiting", "transitioning", "effective"])
def test_frame_withdrawal_retires_direct_demand_before_periodic_admission(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    active: bool,
    effective: str,
) -> None:
    receipt, registration, request = _bind(
        controller, audio_engine_mock, monkeypatch, active=active, effective=effective
    )
    entry = controller.project.stem_cache[0]
    intent = controller.project.pad_stem_intent[0]
    # An ordinary launch makes observation stale; accepted residency/readers stay owned.
    receipt.current = False

    for _ in range(17):
        controller.stems.on_frame_render()

    status = controller.stems.stem_residency_status(0)
    assert status is not None
    assert status.residency is status.effective is status.requested_mode is None
    assert status.desired_mode is None
    assert not controller.stems._demands[0].explicit
    assert controller.stems._demands[0].state == "verified"
    assert controller.project.stem_cache[0] is entry
    assert controller.project.pad_stem_intent[0] is intent
    assert controller.project.pad_stem_mix_mode[0] == "all_stems"
    assert not controller.session.stem_generation_errors
    request.assert_called_once_with(registration, mode="all_stems", enabled_mask=10)
    registration.cancel.assert_not_called()
    assert receipt.cancellations == 0


@pytest.mark.parametrize("active", [False, True])
def test_fresh_explicit_gesture_keeps_its_capacity_retry_after_old_withdrawal(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    active: bool,
) -> None:
    receipt, registration, request = _bind(
        controller, audio_engine_mock, monkeypatch, active=active
    )
    receipt.current = False
    fresh = _Receipt(residency="pending")
    request.side_effect = [
        RuntimeError("cold source queue full (2 workers, 32 queued jobs)"),
        fresh,
    ]

    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    for _ in range(3):
        controller.stems.on_frame_render()

    assert controller.stems._demands[0].explicit
    status = controller.stems.stem_residency_status(0)
    assert status is not None
    assert status.residency == "pending"
    assert status.desired_mode == "all_stems"
    assert request.call_args_list == [
        call(registration, mode="all_stems", enabled_mask=10),
        call(registration, mode="all_stems", enabled_mask=10),
        call(registration, mode="all_stems", enabled_mask=10),
    ]
    assert receipt.cancellations == 0


def test_withdrawal_preserves_preload_interest_without_a_new_all_request(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    receipt, registration, request = _bind(controller, audio_engine_mock, monkeypatch, active=False)
    demand = controller.stems._demands[0]
    demand.policy = True
    policy = _Receipt(effective="not_requested")
    request.return_value = policy
    receipt.current = False

    controller.stems.on_frame_render()

    assert not demand.explicit
    assert demand.policy
    assert demand.state == "acked"
    assert request.call_args_list == [
        call(registration, mode="all_stems", enabled_mask=10),
        call(registration, mode="full_mix", interest="preload"),
    ]
    assert not controller.session.stem_generation_errors


@pytest.mark.parametrize("active", [False, True])
def test_fresh_mask_before_poll_is_not_retired_with_the_old_receipt(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    active: bool,
) -> None:
    receipt, registration, request = _bind(
        controller, audio_engine_mock, monkeypatch, active=active
    )
    receipt.current = False
    fresh = _Receipt(residency="pending")
    request.return_value = fresh
    monkeypatch.setattr(audio_engine_mock, "capture_prepared_source", Mock(return_value=object()))

    assert controller.stems.set_stem_enabled_mask(0, 6)
    for _ in range(3):
        controller.stems.on_frame_render()

    assert controller.stems._demands[0].explicit
    assert controller.project.pad_stem_intent[0].enabled_mask == 6
    status = controller.stems.stem_residency_status(0)
    assert status is not None
    assert status.residency == "pending"
    assert status.desired_mode == "all_stems"
    assert request.call_args_list == [
        call(registration, mode="all_stems", enabled_mask=10),
        call(registration, mode="all_stems", enabled_mask=6),
    ]
    assert not controller.session.stem_generation_errors


@pytest.mark.parametrize("active", [False, True])
@pytest.mark.parametrize(("mask", "display_mode"), [(6, "custom"), (14, "instrumental")])
@pytest.mark.parametrize(
    ("pressure", "waiting_state"),
    [
        ("cold source queue full (2 workers, 32 queued jobs)", "queued"),
        ("[resident_pcm_budget_exceeded] selected backing exceeds allowance", "deferred"),
    ],
)
def test_changed_mask_after_completed_withdrawal_keeps_own_pressure_retry(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    active: bool,
    mask: int,
    display_mode: StemMaskDisplayMode,
    pressure: str,
    waiting_state: str,
) -> None:
    receipt, registration, request = _bind(
        controller, audio_engine_mock, monkeypatch, active=active
    )
    receipt.current = False
    for _ in range(17):
        controller.stems.on_frame_render()
    demand = controller.stems._demands[0]
    assert not demand.explicit
    assert demand.state == "verified"
    assert request.call_count == 1
    fresh = _Receipt(residency="pending")
    request.side_effect = [RuntimeError(pressure), fresh]
    monkeypatch.setattr(audio_engine_mock, "capture_prepared_source", Mock(return_value=object()))

    admitted = controller.stems.set_stem_enabled_mask(0, mask, display_mode)

    assert demand.explicit, "the fresh mask gesture must restore its own retry interest"
    assert admitted
    assert demand.state == waiting_state
    assert controller.project.pad_stem_intent[0].enabled_mask == mask
    assert controller.project.pad_stem_intent[0].display_mode == display_mode
    assert controller.project.pad_stem_mix_mode[0] == "all_stems"
    for _ in range(3):
        controller.stems.on_frame_render()
    status = controller.stems.stem_residency_status(0)
    assert status is not None
    assert status.residency == "pending"
    assert status.effective == "waiting"
    assert demand.explicit
    assert request.call_args_list == [
        call(registration, mode="all_stems", enabled_mask=10),
        call(registration, mode="all_stems", enabled_mask=mask),
        call(registration, mode="all_stems", enabled_mask=mask),
    ]
    fresh.residency = "accepted"
    fresh.effective = "effective"
    controller.stems.on_frame_render()
    assert demand.state == "acked"
    assert not controller.session.stem_generation_errors
    assert request.call_count == 3
    assert receipt.cancellations == 0


def test_shared_mask_publisher_does_not_rearm_withdrawn_direct_demand(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    receipt, _, request = _bind(controller, audio_engine_mock, monkeypatch, active=False)
    receipt.current = False
    controller.stems.on_frame_render()
    demand = controller.stems._demands[0]
    assert not demand.explicit
    request.side_effect = RuntimeError("cold source queue full (2 workers, 32 queued jobs)")
    monkeypatch.setattr(audio_engine_mock, "capture_prepared_source", Mock(return_value=object()))

    assert not controller.stems.publish_stem_enabled_mask_if_available(0)
    for _ in range(3):
        controller.stems.on_frame_render()

    assert not demand.explicit
    assert demand.state == "verified"
    assert request.call_count == 2


@pytest.mark.parametrize("active", [False, True])
def test_known_native_failure_is_not_a_withdrawn_success(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    active: bool,
) -> None:
    receipt, _, request = _bind(controller, audio_engine_mock, monkeypatch, active=active)
    receipt.current = False
    receipt.failure_reason = "native-preparation-failed"

    for _ in range(3):
        controller.stems.on_frame_render()

    assert controller.stems._demands[0].explicit
    assert controller.session.stem_generation_errors == {0: "actual Native preparation failed"}
    assert request.call_count == 1


@pytest.mark.parametrize(
    "changed",
    [
        {"content_id": "e" * 32},
        {"source_path": "samples/other.wav"},
        {"source_version": "other-source"},
        {"stem_set_identity": "c" * 64},
        {"mode": "full_mix"},
        {"enabled_mask": 6},
    ],
)
def test_withdrawal_cannot_retire_a_different_binding_or_newer_intent(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    changed: _WithdrawnChange,
) -> None:
    receipt, registration, request = _bind(controller, audio_engine_mock, monkeypatch, active=False)
    entry = controller.project.stem_cache[0]
    original_intent = controller.project.pad_stem_intent[0]
    original_source = controller.project.sample_paths[0]
    assert entry is not None
    assert entry.pair is not None
    event = WithdrawnStemIntent(
        0,
        "f" * 32,
        controller.project.sample_paths[0],
        entry.source_version,
        entry.pair.stem_set_identity,
        "all_stems",
        10,
    )
    polls: list[frozenset[int]] = []

    def poll(
        *, deferred_sample_ids: frozenset[int] = frozenset()
    ) -> tuple[WithdrawnStemIntent, ...]:
        polls.append(deferred_sample_ids)
        if event.sample_id in deferred_sample_ids:
            return ()
        return (replace(event, **changed),)

    monkeypatch.setattr(controller.stems._committed_residency, "poll", poll)
    controller.stems._pair_deferred_sample_ids = frozenset((0,))
    controller.stems._poll_committed_residency()
    assert controller.stems._demands[0].explicit
    assert request.call_count == 1
    controller.stems._pair_deferred_sample_ids = frozenset()

    controller.stems._poll_committed_residency()

    assert controller.stems._demands[0].explicit
    assert request.call_count == 1
    assert polls == [frozenset((0,)), frozenset()]
    assert controller.project.sample_paths[0] == original_source
    assert controller.project.stem_cache[0] is entry
    assert controller.project.pad_stem_intent[0] is original_intent
    registration.cancel.assert_not_called()
    assert receipt.cancellations == 0
