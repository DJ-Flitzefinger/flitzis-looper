"""Real Python mirrors, Config and custody around a bounded scripted Native façade.

The Native Pair/lease double opens no device and does not certify callback DSP,
musical continuity, Native resource limits or Human listening acceptance.
"""

import hashlib
from concurrent.futures import Future
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller import pair_control_mirror
from flitzis_looper.controller.accepted_publication import AcceptedTimingController
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.pad_selection import record_pad_selection
from flitzis_looper.controller.pair_arrangement import PairArrangementController
from flitzis_looper.controller.pair_arrangement_journal import (
    PairJournalOwner,
    validated_pair_history,
)
from flitzis_looper.controller.pair_arrangement_recovery import inspect_pair_recovery
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_intent import StemGenerationIntent
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from flitzis_looper.ui.context import WaveformEditorActions
from tests.flitzis_looper.controller.test_pair_arrangement import _Audio as _PairNative
from tests.flitzis_looper.controller.test_pair_control_mirror import _pending

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import AssignmentImage
    from flitzis_looper.controller.pair_arrangement import _Pair
    from flitzis_looper.controller.pair_arrangement_journal import PairArrangementJournal
    from flitzis_looper.controller.persistence import ConfigCommitReceipt
    from flitzis_looper.controller.transport import residency
    from flitzis_looper_audio import (
        AudioEngine,
        ConstantTimingTicket,
        OfflineStemSourceLease,
        PadPairAcknowledgement,
        ProjectAssetLease,
    )
    from tests.flitzis_looper.controller.test_pair_arrangement import _Captured, _Ticket


@dataclass(slots=True)
class _Lease:
    path: str
    release_count: int = 0
    release_failures: int = 0

    def release(self) -> None:
        if self.release_failures:
            self.release_failures -= 1
            message = "original assignment cleanup is pending"
            raise RuntimeError(message)
        self.release_count = 1

    @property
    def released(self) -> bool:
        return self.release_count == 1


class _Native(_PairNative):
    def __init__(self) -> None:
        super().__init__()
        self.leases: list[_Lease] = []
        self.retired_paths: list[str] = []
        self.validations: list[tuple[_Ticket, object]] = []
        self.voices = {0: object(), 215: object()}

    def acquire_project_asset_lease(self, path: str) -> ProjectAssetLease:
        lease = _Lease(path)
        self.leases.append(lease)
        return cast("ProjectAssetLease", lease)

    def retire_project_asset(self, path: str, *, recursive: bool = False) -> None:
        assert not recursive
        self.retired_paths.append(path)

    def _publish_pad_pair(self, ticket: _Ticket) -> None:
        super()._publish_pad_pair(ticket)
        source, target = ticket.captured.source, ticket.captured.target
        self.voices[source], self.voices[target] = self.voices[target], self.voices[source]

    def _validate_pad_pair_acknowledgement(self, ticket: _Ticket, ack: object) -> bool:
        self.validations.append((ticket, ack))
        return super()._validate_pad_pair_acknowledgement(ticket, ack)


@dataclass(slots=True)
class _Arrangement:
    active: bool = False


@dataclass(slots=True)
class _AppFixture:
    _project: ProjectState
    _session: SessionState
    _audio: _Native
    _assets: ProjectAssetLifecycle
    project: ProjectState
    session: SessionState
    persistence: ProjectPersistence
    transport: TransportController
    loader: LoaderController
    accepted_timing: AcceptedTimingController
    metering: MeteringController
    stems: StemController
    arrangement: _Arrangement
    _pair_waveform_views: list[WaveformEditorActions]


@dataclass(slots=True)
class _OriginalState:
    identity: PadContentIdentity | None
    voice: object
    pending: residency._Pending
    future: Future[ConstantTimingTicket]
    stem_source: OfflineStemSourceLease
    stem_intent: StemGenerationIntent
    zoom: tuple[float, float]


@dataclass(slots=True)
class _Harness:
    app: _AppFixture
    controller: PairArrangementController
    native: _Native
    assets: ProjectAssetLifecycle
    images: list[AssignmentImage]

    def original_pair(self) -> _Pair:
        pair = self.controller._pair
        assert pair is not None
        return pair

    def complete(self) -> None:
        for _ in range(12):
            self.controller.poll()
            if not self.controller.active:
                break
        assert self.controller.status == "completed", self.controller.error


def _project(tmp_path: Path) -> ProjectState:
    project = ProjectState()
    for sample_id, identity in ((0, "1" * 32), (215, "2" * 32)):
        path = tmp_path / "samples" / f"source-{sample_id}.wav"
        path.write_bytes(b"scripted Native fixture")
        project.sample_paths[sample_id] = path.relative_to(tmp_path).as_posix()
        project.pad_content[sample_id] = PadContentIdentity(instance_id=identity)
        project.sample_durations[sample_id] = 12.0
    project.pad_gain_db[0], project.pad_gain_db[215] = -6.0, -12.0
    return project


def _harness(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> _Harness:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    project = _project(tmp_path)
    session = SessionState(active_sample_ids={0, 215}, paused_sample_ids={215})
    native = _Native()
    # Only the controller-facing view is Native; tests retain the scripted handle above.
    audio = cast("AudioEngine", native)
    assets = ProjectAssetLifecycle(project, audio)
    assets.sync_assignments()
    transport = TransportController(project, session, audio)
    persistence = ProjectPersistence(project)
    persistence.config_path = samples / "owning.config.json"
    persistence.flush()
    app = _AppFixture(
        _project=project,
        _session=session,
        _audio=native,
        _assets=assets,
        project=project,
        session=session,
        persistence=persistence,
        transport=transport,
        loader=LoaderController(project, session, audio, lambda _sample_id: None),
        accepted_timing=AcceptedTimingController(transport),
        metering=MeteringController(project, session, audio),
        stems=StemController(
            project, session, audio, stem_task_runner=lambda run: run(), asset_lifecycle=assets
        ),
        arrangement=_Arrangement(),
        _pair_waveform_views=[],
    )
    # This fixture supplies the AppController components exercised by the real mirrors.
    WaveformEditorActions(cast("AppController", app))
    images: list[AssignmentImage] = []
    prepare = assets.prepare_assignment_image

    def prepare_image(candidate: ProjectState) -> AssignmentImage:
        image = prepare(candidate)
        images.append(image)
        return image

    monkeypatch.setattr(assets, "prepare_assignment_image", prepare_image)
    return _Harness(
        app, PairArrangementController(cast("AppController", app)), native, assets, images
    )


def _native_ticket(pair: _Pair) -> _Ticket:
    """Recover the scripted Native handle held behind the production Native view."""
    ticket = pair.ticket
    assert ticket is not None
    return cast("_Ticket", ticket)


def _native_capture(pair: _Pair) -> _Captured:
    """Recover the scripted Native state used by this fixture's acknowledgement oracle."""
    captured = pair.captured
    assert captured is not None
    return cast("_Captured", captured)


def _receipt(pair: _Pair) -> ConfigCommitReceipt:
    """Read and validate the receipt in the current controller phase."""
    receipt = pair.receipt
    assert receipt is not None
    return receipt


def _acknowledgement(pair: _Pair) -> PadPairAcknowledgement:
    """Read the acknowledgement afresh after a controller transition."""
    acknowledgement = pair.acknowledgement
    assert acknowledgement is not None
    return acknowledgement


def _journal(pair: _Pair) -> PairArrangementJournal:
    """Read the current journal after a controller transition."""
    owner = pair.journal
    assert owner is not None
    journal = owner.journal
    assert journal is not None
    return journal


def _fail_config_writes(harness: _Harness, monkeypatch: pytest.MonkeyPatch, count: int) -> None:
    write = harness.app.persistence._atomic_write_text
    remaining = count

    def fail_before_replace(text: str, *, before_replace: Callable[[], None] | None = None) -> None:
        nonlocal remaining
        if remaining:
            remaining -= 1
            message = "actual Config replacement is unavailable"
            raise OSError(message)
        write(text, before_replace=before_replace)

    monkeypatch.setattr(harness.app.persistence, "_atomic_write_text", fail_before_replace)


def _assert_complete_receipt(harness: _Harness, pair: _Pair, revision: int) -> None:
    receipt = pair.receipt
    assert receipt is not None
    written = harness.app.persistence.config_path.read_bytes()
    assert receipt.revision == harness.app.persistence.revision == revision
    assert receipt.written_text.encode("utf-8") == written
    assert receipt.sha256 == hashlib.sha256(written).hexdigest()
    history = PairJournalOwner.open(Path.cwd() / "samples", pair.owner)
    records = validated_pair_history(history.encoded, pair.owner)
    assert records[-1].phase == "settled"
    assert records[-1].committed_revision == revision
    assert records[-1].committed_config_sha256 == receipt.sha256
    assert harness.app.persistence._migration_owner is None
    assert pair.retirement.remaining == 0


def _assert_live_assignment_leases(native: _Native) -> None:
    assert all(lease.release_count == 1 for lease in native.leases[:2])
    assert all(lease.release_count == 0 for lease in native.leases[2:])


def _seed_original_state(harness: _Harness) -> _OriginalState:
    """Install concrete original owners whose identities the live mirror must preserve."""
    app = harness.app
    original = _OriginalState(
        identity=app.project.pad_content[0],
        voice=harness.native.voices[0],
        pending=_pending(),
        future=Future(),
        # The mirror moves this opaque Native owner without invoking source operations.
        stem_source=cast("OfflineStemSourceLease", object()),
        stem_intent=StemGenerationIntent(0, 0, 0, "scripted pair mirror fixture", ()),
        zoom=(-0.4, 8.6),
    )
    app.transport.residency._pending[0] = original.pending
    app.accepted_timing._preparations[0] = original.future
    app.stems._generation_sources[0] = original.stem_source
    app.stems._generation_intents[0] = original.stem_intent
    app.session.stem_generating_sample_ids.add(0)
    app._pair_waveform_views[0]._pad_view_ranges[0] = original.zoom
    return original


def test_original_live_owner_crosses_real_mirrors_config_journal_and_assets_once(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    app = harness.app
    original = _seed_original_state(harness)
    harness.native.publish_failures = 1
    assert harness.controller.begin(0, 215, operation="swap")
    pair = harness.original_pair()
    harness.controller.poll()
    assert harness.controller.active
    assert pair.acknowledgement is None
    app.project.pad_gain_db[36] = -18.0
    app.project.selected_pad = 36
    app.project.volume = 0.4
    app.session.waveform_editor_pad_id = 36
    app.session.tap_bpm_pad_id = 36
    app.persistence.mark_dirty()
    harness.complete()
    ack = _acknowledgement(pair)
    assert ack is _native_capture(pair).ack
    assert pair.mirror is not None
    assert pair.stems is not None
    assert pair.mirror.applied
    assert pair.stems.applied
    assert pair.mirror._acknowledgement is ack
    assert pair.stems._acknowledgement is ack
    assert app.project.pad_content[215] is original.identity
    assert harness.native.voices[215] is original.voice
    assert app.transport.residency._pending[215] is original.pending
    assert app.accepted_timing._preparations[215] is original.future
    assert app.stems._generation_sources[215] is original.stem_source
    assert app.stems._generation_intents[215] is original.stem_intent
    assert app._pair_waveform_views[0]._pad_view_ranges[215] is original.zoom
    assert app.session.active_sample_ids == {0, 215}
    assert app.session.paused_sample_ids == {0}
    assert app.session.waveform_editor_pad_id == app.session.tap_bpm_pad_id == 36
    assert len(harness.native.captures) == len(harness.native.tickets) == 1
    assert len(harness.native.publish_calls) == 2
    assert all(
        ticket is _native_ticket(pair) and actual is ack
        for ticket, actual in harness.native.validations
    )
    assert harness.native.finish_calls == [(_native_ticket(pair), ack)]
    assert len(harness.images) == 1
    assert pair.assignment_publication is not None
    assert pair.assignment_publication.settled
    assert harness.assets._assignments["original", 215][0].name == "source-0.wav"
    _assert_live_assignment_leases(harness.native)
    _assert_complete_receipt(harness, pair, 1)
    saved = _receipt(pair).projected()
    assert saved.pad_gain_db[36] == -18.0
    assert saved.volume == 0.4


def test_postclaim_config_failures_retain_exact_candidate_image_and_ack_until_same_revision_commit(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    _fail_config_writes(harness, monkeypatch, 2)
    before = harness.app.persistence.config_path.read_bytes()
    before_assignments = harness.assets._assignments
    assert harness.controller.begin(0, 215, operation="swap")
    harness.controller.poll()
    pair = harness.original_pair()
    retained = (pair.candidate, pair.assignment, pair.assignment_publication, pair.acknowledgement)
    assert pair.receipt is None
    assert pair.published
    assert _journal(pair).phase == "native_ack_observed"
    assert harness.assets._assignments is before_assignments
    assert harness.app.persistence.config_path.read_bytes() == before
    harness.controller.poll()
    assert all(
        actual is expected
        for actual, expected in zip(
            (pair.candidate, pair.assignment, pair.assignment_publication, pair.acknowledgement),
            retained,
            strict=True,
        )
    )
    assert len(harness.images) == 1
    assert pair.images == [pair.assignment]
    assert pair.assignment_publication is not None
    assert pair.assignment_publication.owns_unpublished_image
    assert all(lease.release_count == 0 for lease in harness.native.leases)
    assert harness.app.persistence._migration_owner == pair.owner
    assert harness.native.finish_calls == []
    harness.complete()
    assert pair.assignment is retained[1]
    assert pair.assignment_publication is retained[2]
    assert pair.acknowledgement is retained[3]
    assert len(harness.images) == 1
    assert pair.images == []
    assert len(harness.native.captures) == len(harness.native.publish_calls) == 1
    _assert_complete_receipt(harness, pair, 0)


def test_interrupted_real_mirror_keeps_same_ack_and_merges_newer_unrelated_config(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    original_identity = harness.app.project.pad_content[0]
    store = pair_control_mirror._store
    failed = False

    def interrupt_after_store(owner: object, name: str, image: object) -> None:
        nonlocal failed
        store(owner, name, image)
        if owner is harness.app.project and name == "sample_paths" and not failed:
            failed = True
            message = "interrupted complete own Project store"
            raise RuntimeError(message)

    monkeypatch.setattr(pair_control_mirror, "_store", interrupt_after_store)
    assert harness.controller.begin(0, 215, operation="swap")
    harness.controller.poll()
    pair = harness.original_pair()
    mirror, stem_mirror, ack = pair.mirror, pair.stems, pair.acknowledgement
    assert pair.receipt is not None
    assert mirror is not None
    assert stem_mirror is not None
    assert not mirror.applied
    assert mirror._acknowledgement is ack
    assert harness.native.finish_calls == []
    harness.app.project.pad_gain_db[36] = -22.0
    harness.app.project.volume = 0.5
    harness.app.persistence.mark_dirty()
    harness.complete()
    assert pair.mirror is mirror
    assert pair.stems is stem_mirror
    assert pair.acknowledgement is ack
    assert mirror.applied
    assert stem_mirror.applied
    assert harness.app.project.pad_content[215] is original_identity
    assert len(harness.native.captures) == len(harness.native.publish_calls) == 1
    assert harness.native.finish_calls == [(_native_ticket(pair), ack)]
    assert all(
        ticket is _native_ticket(pair) and actual is ack
        for ticket, actual in harness.native.validations
    )
    _assert_complete_receipt(harness, pair, 1)
    saved = _receipt(pair).projected()
    assert saved.pad_gain_db[36] == -22.0
    assert saved.volume == 0.5


@pytest.mark.parametrize(("selected", "expected"), [(0, 215), (215, 0), (36, 36)])
def test_selected_content_has_identical_live_config_and_journal_placement(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, selected: int, expected: int
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    record_pad_selection(harness.app.project, harness.app.session, selected)
    harness.app.persistence.mark_dirty()
    assert harness.controller.begin(0, 215, operation="swap")
    pair = harness.original_pair()
    harness.complete()
    assert harness.app.project.selected_pad == expected
    assert _receipt(pair).projected().selected_pad == expected
    assert (
        ProjectState.model_validate_json(
            harness.app.persistence.config_path.read_text(encoding="utf-8")
        ).selected_pad
        == expected
    )
    _assert_complete_receipt(harness, pair, 1)


def test_navigation_during_actual_config_write_recaptures_content_and_regenerates_receipt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    app = harness.app
    record_pad_selection(app.project, app.session, 0)
    app.persistence.mark_dirty()
    write = app.persistence._atomic_write_text
    navigated = False

    def write_then_navigate(text: str, *, before_replace: Callable[[], None] | None = None) -> None:
        nonlocal navigated
        write(text, before_replace=before_replace)
        if not navigated:
            navigated = True
            for selected in (0, 36, 0):
                record_pad_selection(app.project, app.session, selected)
                app.persistence.mark_dirty()

    monkeypatch.setattr(app.persistence, "_atomic_write_text", write_then_navigate)
    assert harness.controller.begin(0, 215, operation="swap")
    pair = harness.original_pair()
    harness.complete()
    assert navigated
    assert app.session.pad_selection_content_id == "1" * 32
    assert app.project.selected_pad == _receipt(pair).projected().selected_pad == 215
    assert len(harness.native.captures) == len(harness.native.publish_calls) == 1
    _assert_complete_receipt(harness, pair, 4)


@pytest.mark.parametrize(("selected", "expected"), [(0, 215), (215, 0)])
def test_lost_config_replace_return_recognizes_actual_selected_pair_mirror_image(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, selected: int, expected: int
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    app = harness.app
    record_pad_selection(app.project, app.session, selected)
    app.persistence.mark_dirty()
    write = app.persistence._atomic_write_text
    failed = False

    def write_then_fail(text: str, *, before_replace: Callable[[], None] | None = None) -> None:
        nonlocal failed
        write(text, before_replace=before_replace)
        if not failed:
            failed = True
            message = "lost selected Config receipt after actual replacement"
            raise OSError(message)

    monkeypatch.setattr(app.persistence, "_atomic_write_text", write_then_fail)
    assert harness.controller.begin(0, 215, operation="swap")
    harness.controller.poll()
    pair = harness.original_pair()
    assert pair.receipt is None
    assert pair.mirror is not None
    assert not pair.mirror.applied
    assert app.project.selected_pad == selected
    decision = inspect_pair_recovery(_journal(pair), app.persistence.config_path.read_bytes())
    assert decision.disposition == "recognized_after"
    assert decision.receipt is not None
    assert decision.receipt.projected().selected_pad == expected
    assert app.persistence._migration_owner == pair.owner
    harness.complete()
    assert app.project.selected_pad == _receipt(pair).projected().selected_pad == expected
    assert len(harness.native.captures) == len(harness.native.publish_calls) == 1
    _assert_complete_receipt(harness, pair, 1)


@pytest.mark.parametrize(
    ("interruption", "expected"),
    [("sample_paths", 215), ("pad_content", 0), ("selected_pad", 0)],
)
def test_selection_aba_during_interrupted_pair_rewrites_the_same_own_config_receipt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, interruption: str, expected: int
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    app = harness.app
    record_pad_selection(app.project, app.session, 0)
    app.persistence.mark_dirty()
    store = pair_control_mirror._store
    interrupted = False

    def interrupt_after_store(owner: object, name: str, image: object) -> None:
        nonlocal interrupted
        store(owner, name, image)
        if owner is app.project and name == interruption and not interrupted:
            interrupted = True
            message = "interrupted selection settlement"
            raise RuntimeError(message)

    monkeypatch.setattr(pair_control_mirror, "_store", interrupt_after_store)
    assert harness.controller.begin(0, 215, operation="swap")
    harness.controller.poll()
    pair = harness.original_pair()
    mirror, ack = pair.mirror, pair.acknowledgement
    assert interrupted
    assert mirror is not None
    assert not mirror.applied
    for selected in (0, 36, 0):
        record_pad_selection(app.project, app.session, selected)
        app.persistence.mark_dirty()
    harness.complete()
    chosen = "1" * 32 if interruption == "sample_paths" else "2" * 32
    assert app.session.pad_selection_content_id == chosen
    assert app.project.selected_pad == _receipt(pair).projected().selected_pad == expected
    assert pair.mirror is mirror
    assert pair.acknowledgement is ack
    assert len(harness.native.captures) == len(harness.native.publish_calls) == 1
    _assert_complete_receipt(harness, pair, 4)


def test_newer_intent_replaces_only_unpublished_image_and_keeps_original_native_ack(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    _fail_config_writes(harness, monkeypatch, 1)
    assert harness.controller.begin(0, 215, operation="swap")
    harness.controller.poll()
    pair = harness.original_pair()
    obsolete = pair.assignment
    assert obsolete is not None
    ack = pair.acknowledgement
    harness.app.project.pad_gain_db[36] = -23.0
    harness.app.project.volume = 0.25
    harness.app.persistence.mark_dirty()
    harness.complete()
    assert len(harness.images) == 2
    assert pair.assignment is not obsolete
    assert pair.assignment is not None
    assert all(cast("_Lease", lease).release_count == 1 for _, lease in obsolete[1].values())
    assert all(cast("_Lease", lease).release_count == 0 for _, lease in pair.assignment[1].values())
    assert pair.images == []
    assert pair.acknowledgement is ack
    assert len(harness.native.captures) == len(harness.native.publish_calls) == 1
    _assert_complete_receipt(harness, pair, 1)
    saved = _receipt(pair).projected()
    assert saved.pad_gain_db[36] == -23.0
    assert saved.volume == 0.25


def test_published_asset_cleanup_exception_retains_plan_before_native_and_writer_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    harness.native.leases[0].release_failures = 1
    assert harness.controller.begin(0, 215, operation="swap")
    harness.controller.poll()
    pair = harness.original_pair()
    publication = pair.assignment_publication
    assert publication is not None
    assert publication.applied
    assert not publication.settled
    assert not publication.owns_unpublished_image
    assert pair.images == []
    assert pair.receipt is not None
    assert pair.mirror is not None
    assert pair.stems is not None
    assert pair.mirror.applied
    assert pair.stems.applied
    assert harness.native.finish_calls == []
    assert harness.app.persistence._migration_owner == pair.owner
    assert pair.retirement.remaining > 0
    current = harness.assets._assignments
    assert current["original", 215][0].name == "source-0.wav"
    harness.complete()
    assert pair.assignment_publication is publication
    assert publication.settled
    assert harness.assets._assignments is current
    assert len(harness.images) == 1
    assert harness.native.finish_calls == [(_native_ticket(pair), pair.acknowledgement)]
    _assert_complete_receipt(harness, pair, 0)


def test_claimed_cancellation_keeps_actual_owner_and_polls_ack_without_republication(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    harness.native.auto_ack = False
    before = harness.app.persistence.config_path.read_bytes()
    assert harness.controller.begin(0, 215, operation="swap")
    harness.controller.poll()
    pair = harness.original_pair()
    assert _native_capture(pair).claimed
    assert pair.acknowledgement is None
    assert not harness.controller.prepare_shutdown()
    assert not harness.controller.prepare_shutdown()
    assert harness.controller._pair is pair
    assert not pair.cancelled
    assert all(actual is _native_capture(pair) for actual in harness.native.cancel_calls)
    assert harness.app.persistence._migration_owner == pair.owner
    assert pair.retirement.remaining > 0
    assert harness.images == []
    assert harness.app.persistence.config_path.read_bytes() == before
    _native_capture(pair).acknowledged = True
    harness.complete()
    assert _acknowledgement(pair) is _native_capture(pair).ack
    assert len(harness.native.captures) == len(harness.native.publish_calls) == 1
    _assert_complete_receipt(harness, pair, 0)


def test_preclaim_captured_journal_failure_cancels_safely_without_native_capture(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    append = PairJournalOwner.append
    fail = True

    def fail_initial(
        owner: PairJournalOwner, record: PairArrangementJournal
    ) -> PairArrangementJournal:
        nonlocal fail
        if record.phase == "captured" and fail:
            fail = False
            message = "initial journal append unavailable"
            raise OSError(message)
        return append(owner, record)

    monkeypatch.setattr(PairJournalOwner, "append", fail_initial)
    before = harness.app.persistence.config_path.read_bytes()
    assert harness.controller.begin(0, 215, operation="swap")
    pair = harness.original_pair()
    assert pair.captured is None
    assert pair.journal is not None
    assert pair.journal.journal is None
    assert harness.app.persistence._migration_owner == pair.owner
    assert harness.controller.prepare_shutdown()
    assert harness.controller.status == "cancelled"
    assert _journal(pair).phase == "cancelled"
    assert harness.native.captures == []
    assert harness.native.publish_calls == []
    assert harness.native.finish_calls == []
    assert harness.app.persistence.config_path.read_bytes() == before
    assert harness.app.persistence._migration_owner is None
    assert pair.retirement.remaining == 0
    assert all(lease.release_count == 0 for lease in harness.native.leases)


def test_settled_journal_return_loss_releases_retained_writer_without_backward_append(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    append = PairJournalOwner.append
    failed = False

    def lose_settled_return(
        owner: PairJournalOwner, record: PairArrangementJournal
    ) -> PairArrangementJournal:
        nonlocal failed
        written = append(owner, record)
        if record.phase == "settled" and not failed:
            failed = True
            message = "lost settled journal return"
            raise OSError(message)
        return written

    monkeypatch.setattr(PairJournalOwner, "append", lose_settled_return)
    assert harness.controller.begin(0, 215, operation="swap")
    harness.controller.poll()
    pair = harness.original_pair()
    assert _journal(pair).phase == "settled"
    assert pair.native_finished
    assert pair.journal is not None
    receipt, assignment, encoded = pair.receipt, pair.assignment, pair.journal.encoded.copy()
    assert harness.app.persistence._migration_owner == pair.owner
    assert pair.retirement.remaining > 0
    harness.complete()
    assert pair.receipt is receipt
    assert pair.assignment is assignment
    assert pair.journal.encoded == encoded
    assert harness.native.finish_calls == [(_native_ticket(pair), pair.acknowledgement)]
    assert len(harness.images) == 1
    _assert_complete_receipt(harness, pair, 0)
