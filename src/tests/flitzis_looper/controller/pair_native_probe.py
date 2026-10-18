"""Real Controller/Config owners around a device-free genuine Native Pair fixture.

The enclosing Rust test supplies the actual source publications and callback.
This module neither simulates a Native acknowledgement nor advances audio.
"""

import hashlib
import struct
import wave
from contextlib import ExitStack, chdir
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, cast

from flitzis_looper.controller.accepted_publication import AcceptedTimingController
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.pair_arrangement import PairArrangementController
from flitzis_looper.controller.pair_arrangement_journal import (
    PairJournalOwner,
    validated_pair_history,
)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from flitzis_looper.ui.context import WaveformEditorActions

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.pair_arrangement import _Pair
    from flitzis_looper.controller.transport.waveform import _HoldOwner
    from flitzis_looper.pair_snapshot import PairOperation
    from flitzis_looper_audio import AudioEngine, HoldActionTicket, PadPairTicket


_RATE = 48_000
_FRAMES = 128
_LOOP = (16 / _RATE, 96 / _RATE)
_ZOOM = (-0.001, 0.004)
_EQUALIZERS = ((-8.0, 2.0, -1.0), (1.0, -3.0, -9.0))


@dataclass(slots=True)
class _InactiveArrangement:
    active: bool = False


@dataclass(slots=True)
class _AppFixture:
    """Only the App envelope is synthetic; its components and Native are real."""

    _project: ProjectState
    _session: SessionState
    _audio: AudioEngine
    _assets: ProjectAssetLifecycle
    project: ProjectState
    session: SessionState
    persistence: ProjectPersistence
    transport: TransportController
    loader: LoaderController
    accepted_timing: AcceptedTimingController
    metering: MeteringController
    stems: StemController
    arrangement: _InactiveArrangement
    _pair_waveform_views: list[WaveformEditorActions]


def _write_source(path: Path, level: int) -> None:
    """Write the exact stereo PCM16 constants published by the Rust fixture."""
    with path.open("xb") as stream, wave.open(stream, "wb") as encoded:
        encoded.setnchannels(2)
        encoded.setsampwidth(2)
        encoded.setframerate(_RATE)
        encoded.writeframes(struct.pack("<h", level) * _FRAMES * 2)


def _project(samples: Path) -> ProjectState:
    project = ProjectState(
        multi_loop=True,
        bpm_lock=False,
        key_lock=False,
        trigger_quantization_enabled=False,
        selected_pad=0,
    )
    for slot, level in ((0, 8192), (1, -16384)):
        path = samples / f"source-{slot}.wav"
        _write_source(path, level)
        project.sample_paths[slot] = path.relative_to(samples.parent).as_posix()
        project.sample_durations[slot] = _FRAMES / _RATE
        project.pad_content[slot] = PadContentIdentity(instance_id=f"{slot + 1:032x}")
        project.pad_key_lock[slot] = False
        # LaunchFixture publishes manual Native timing; its control projection
        # must describe that same intent before the durable Pair seals it.
        project.pad_timing_intent[slot] = "manual"
        project.pad_key_intent[slot] = PadKeyIntent(source=SourceKeyVersion(raw_key="C"))
        project.pad_loop_auto[slot] = False
        project.pad_loop_start_s[slot], project.pad_loop_end_s[slot] = _LOOP
        (
            project.pad_eq_low_db[slot],
            project.pad_eq_mid_db[slot],
            project.pad_eq_high_db[slot],
        ) = _EQUALIZERS[slot]
    return project


class ControllerProbe:
    """Join real physical Hold ownership to genuine Pair/durable settlement."""

    def __init__(self, audio: AudioEngine, root: str, target: int) -> None:
        assert target in {1, 2}
        self.target = target
        self._closed = False
        self._storage = ExitStack()
        self.pair: _Pair | None = None
        self.hold: HoldActionTicket | None = None
        self.owner: _HoldOwner | None = None
        with ExitStack() as setup:
            directory = Path(root).resolve(strict=True)
            setup.enter_context(chdir(directory))
            samples = directory / "samples"
            samples.mkdir(exist_ok=True)
            self.project = _project(samples)
            for slot in (0, 1):
                assert audio.pad_timing_intent(slot) == self.project.pad_timing_intent[slot]
            self.session = SessionState(active_sample_ids={0, 1}, paused_sample_ids={1})
            self.session.waveform_editor_open = True
            self.session.waveform_editor_pad_id = 0
            self.persistence = ProjectPersistence(self.project)
            self.persistence.config_path = samples / "pair-controller.config.json"
            self.persistence.bind_audio(audio)
            self.assets = ProjectAssetLifecycle(self.project, audio)
            self.assets.sync_assignments()
            self.transport = TransportController(
                self.project, self.session, audio, self.persistence.mark_dirty
            )
            self.app = _AppFixture(
                _project=self.project,
                _session=self.session,
                _audio=audio,
                _assets=self.assets,
                project=self.project,
                session=self.session,
                persistence=self.persistence,
                transport=self.transport,
                loader=LoaderController(self.project, self.session, audio, lambda _slot: None),
                accepted_timing=AcceptedTimingController(self.transport),
                metering=MeteringController(self.project, self.session, audio),
                stems=StemController(
                    self.project,
                    self.session,
                    audio,
                    stem_task_runner=lambda run: run(),
                    asset_lifecycle=self.assets,
                ),
                arrangement=_InactiveArrangement(),
                _pair_waveform_views=[],
            )
            self.view = WaveformEditorActions(cast("AppController", self.app))
            self.view._pad_view_ranges[0] = _ZOOM
            self.view._last_pad_id = 0
            self.pairs = PairArrangementController(cast("AppController", self.app))
            assert not self.pairs.active, self.pairs.error
            self.persistence.mark_dirty()
            self.persistence.flush()
            self.before = self.persistence.config_path.read_bytes()
            self.original_contents = tuple(self.project.pad_content[:2])
            self.original_intents = tuple(self.project.pad_key_intent[:2])
            self.original_gains = tuple(self.project.pad_gain_db[:2])
            self.original_assignments = self.assets._assignments.copy()
            assert set(self.original_assignments) == {("original", 0), ("original", 1)}
            self._assert_before_custody_is_rejected()
            self._storage = setup.pop_all()

    def press(self) -> HoldActionTicket:
        """Admit the physical press without draining its actual callback command."""
        waveform = self.transport.waveform
        assert not waveform.has_pending_holds
        waveform.press_pause_hold(0)
        assert waveform._physical_hold_press is not None
        assert len(waveform._hold_owners) == 1, waveform.hold_status()
        owner = next(iter(waveform._hold_owners.values()))
        assert owner.physical_press_id == waveform._physical_hold_press
        self.owner = owner
        self.hold = owner.ticket
        return self.hold

    def observe_owned(self) -> None:
        """Observe the real pause acknowledgement after the enclosing callback drain."""
        waveform = self.transport.waveform
        waveform.poll_holds()
        assert waveform.hold_status() == ("paused_owned", None)
        assert waveform._physical_hold_press is not None
        assert len(waveform._hold_owners) == 1
        assert next(iter(waveform._hold_owners.values())) is self.owner
        assert self.session.waveform_pause_hold_pad_id == 0

    def begin(self) -> None:
        """Capture once; synchronous preparation still awaits the next control poll."""
        assert self.pair is None
        operation: PairOperation = "swap" if self.target == 1 else "move"
        assert self.pairs.begin(0, self.target, operation=operation), self.pairs.error
        self.pair = self.pairs._pair
        assert self.pair is not None
        assert self.pair.captured is not None
        assert self.pair.ticket is None
        assert not self.pair.worker_done
        assert self.pair.acknowledgement is None
        assert self.pairs.active
        assert self.persistence.config_path.read_bytes() == self.before

    def publish(self) -> PadPairTicket:
        """Publish the original prepared payload without inventing an effect ACK."""
        assert self.pair is not None
        self.pairs.poll()
        assert self.pair.worker_done
        assert self.pair.published
        assert self.pair.ticket is not None, self.pairs.error
        assert self.pair.acknowledgement is None
        assert self.pair.ticket.own_acknowledgement() is None
        assert self.pairs.active
        assert self.persistence.config_path.read_bytes() == self.before
        return self.pair.ticket

    def release(self) -> None:
        """Retain one sticky matching release through the real Hold controller."""
        assert self.hold is not None
        assert self.owner is not None
        waveform = self.transport.waveform
        waveform.request_hold_release("button_up")
        waveform.poll_holds()
        assert waveform._physical_hold_press is None
        assert self.owner.ticket is self.hold
        assert self.owner.release_requested
        if waveform.has_pending_holds:
            assert next(iter(waveform._hold_owners.values())) is self.owner

    def settle(self) -> None:
        """Complete one genuine ACK through durable Config, both mirrors and finish."""
        assert self.pair is not None
        self.pairs.poll()
        assert self.pairs.status == "completed", self.pairs.error
        assert not self.pairs.active
        assert self.pairs.error is None
        pair = self.pair
        assert pair.native_finished
        assert pair.writer_released
        assert pair.receipt is not None
        assert pair.acknowledgement is not None
        assert pair.mirror is not None
        assert pair.mirror.applied
        assert pair.stems is not None
        assert pair.stems.applied
        assert pair.mirror._acknowledgement is pair.acknowledgement
        assert pair.stems._acknowledgement is pair.acknowledgement
        assert pair.assignment_publication is not None
        assert pair.assignment_publication.settled
        written = self.persistence.config_path.read_bytes()
        assert written == pair.receipt.written_text.encode("utf-8")
        assert hashlib.sha256(written).hexdigest() == pair.receipt.sha256
        assert pair.receipt.revision == self.persistence.revision
        history = PairJournalOwner.open(Path.cwd() / "samples", pair.owner)
        records = validated_pair_history(history.encoded, pair.owner)
        assert records[-1].phase == "settled"
        assert records[-1].committed_revision == pair.receipt.revision
        assert records[-1].committed_config_sha256 == pair.receipt.sha256
        assert self.persistence._migration_owner is None
        assert pair.retirement.remaining == 0
        for component in (
            self.transport,
            self.app.loader,
            self.app.accepted_timing,
            self.app.stems,
        ):
            assert component._pair_deferred_sample_ids == frozenset()
        waveform = self.transport.waveform
        assert waveform.has_pending_holds
        assert waveform._physical_hold_press is None
        assert len(waveform._hold_owners) == 1
        assert next(iter(waveform._hold_owners.values())) is self.owner
        self._assert_projected_content(pair.receipt.projected())
        self._assert_assignment_custody()

    def _assert_projected_content(self, saved: ProjectState) -> None:
        destinations = (self.target, 0 if self.target == 1 else 1)
        for original, destination in enumerate(destinations):
            assert self.project.pad_content[destination] is self.original_contents[original]
            assert self.project.pad_key_intent[destination] is self.original_intents[original]
            assert saved.pad_content[destination] == self.original_contents[original]
            assert saved.pad_key_intent[destination] == self.original_intents[original]
            for image in (self.project, saved):
                assert image.sample_paths[destination] == f"samples/source-{original}.wav"
                assert image.pad_gain_db[destination] == self.original_gains[original]
                assert (
                    image.pad_eq_low_db[destination],
                    image.pad_eq_mid_db[destination],
                    image.pad_eq_high_db[destination],
                ) == _EQUALIZERS[original]
                assert (
                    image.pad_loop_start_s[destination],
                    image.pad_loop_end_s[destination],
                ) == _LOOP
        assert self.project.selected_pad == saved.selected_pad == self.target
        assert self.session.waveform_editor_pad_id == self.target
        assert self.session.active_sample_ids == set(destinations)
        assert self.session.paused_sample_ids == {destinations[1]}
        assert self.view._pad_view_ranges[self.target] is _ZOOM
        assert 0 not in self.view._pad_view_ranges
        if self.target == 2:
            assert self.project.pad_content[0] is saved.pad_content[0] is None
            assert self.project.sample_paths[0] is saved.sample_paths[0] is None

    def _assert_before_custody_is_rejected(self) -> None:
        """An unchanged live before map cannot certify after-layout custody."""
        try:
            self._assert_assignment_custody()
        except AssertionError:
            return
        message = "Unchanged before-layout assignments incorrectly certify after-layout custody"
        raise AssertionError(message)

    def _assert_assignment_custody(self) -> None:
        samples = Path.cwd() / "samples"
        expected = {
            ("original", self.target): samples / "source-0.wav",
            ("original", 0 if self.target == 1 else 1): samples / "source-1.wav",
        }
        assert {key: path for key, (path, _) in self.assets._assignments.items()} == expected
        assert all(not lease.released for _, lease in self.assets._assignments.values())
        for key, original in self.original_assignments.items():
            if self.target == 2 and key == ("original", 1):
                assert self.assets._assignments[key] is original
                assert not original[1].released
            else:
                assert original[1].released

    def poll_terminal(self) -> None:
        """Retire only the actual Native terminal Hold acknowledgement."""
        waveform = self.transport.waveform
        waveform.poll_holds()
        assert not waveform.has_pending_holds, waveform.hold_status()
        assert waveform._physical_hold_press is None
        assert waveform.hold_status() == (None, None)
        assert self.session.waveform_pause_hold_pad_id is None

    def close(self) -> None:
        """Release only saved test assignment leases and restore the original cwd."""
        if self._closed:
            return
        try:
            self.assets.release_saved_assignments()
        finally:
            self._storage.close()
            self._closed = True
