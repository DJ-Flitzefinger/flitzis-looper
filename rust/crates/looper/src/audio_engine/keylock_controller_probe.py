"""Real Python controller assertions embedded in the productive Rust callback fixture."""
# This fixture is compiled into Rust tests rather than imported as a Python package.
# ruff: noqa: INP001

from flitzis_looper.controller.transport import TransportController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper.ui.context import PadSelectors


class _ControllerFixture:
    """Productive controller setup, actions, and state queries for the probe."""

    def __init__(self, audio, paths, initial, rate, start, end, frames, message_type):
        self.audio = audio
        # The embedded fixture supplies the real enum from this Rust image.
        # This uses the same variant handling as AppController._handle_message.
        self._messages = message_type
        self.project = ProjectState()
        self.session = SessionState()
        self.project.multi_loop = True
        self.transport = TransportController(self.project, self.session, audio)
        self.selectors = PadSelectors(self, self.project, self.session)
        self.start_s = start / rate
        self.end_s = end / rate
        self.duration = frames / rate
        for sample_id, path in paths:
            self.assign(sample_id, path, initial)

    def assign(self, sample_id, path, initial):
        self.project.sample_paths[sample_id] = path
        self.project.sample_durations[sample_id] = self.duration
        self.project.pad_loop_auto[sample_id] = False
        self.project.pad_loop_start_s[sample_id] = self.start_s
        self.project.pad_loop_end_s[sample_id] = self.end_s
        self.project.pad_key_lock[sample_id] = initial
        self.project.pad_timing_intent[sample_id] = "legacy"

    def initialize(self):
        for sample_id, path in enumerate(self.project.sample_paths):
            if path is not None:
                self.transport.residency.publish_loop(sample_id, self.start_s, self.end_s)

    def global_mode(self, enabled):
        self.transport.global_params.set_key_lock(enabled=enabled)

    def local_mode(self, sample_id, enabled):
        self.transport.pad.set_pad_key_lock(sample_id, enabled=enabled)

    def start(self, sample_id):
        self.transport.playback.trigger_pad_keep_others(sample_id)

    def pause(self, sample_id):
        self.transport.playback.pause_pad(sample_id)

    def resume(self, sample_id):
        self.transport.playback.resume_pad(sample_id)

    def poll(self):
        while (message := self.audio.receive_msg()) is not None:
            if isinstance(message, self._messages.SampleStarted):
                self.transport.playback.handle_sample_started_message(message)
            elif isinstance(message, self._messages.SampleStopped):
                self.transport.playback.handle_sample_stopped_message(message)
        self.transport.on_frame_render()

    def settled(self, sample_id, enabled):
        state = self.transport.residency.key_lock_status(sample_id)
        return state.effective is enabled and not state.pending and not state.error

    def ticket(self, sample_id):
        pending = self.transport.residency._pending.get(sample_id)
        return pending.ticket if pending is not None else None

    def unavailable_timing(self, sample_id):
        self.audio.set_pad_timing_intent(sample_id, "automatic")
        self.project.pad_timing_intent[sample_id] = "automatic"

    def failure_settled(self, sample_id, enabled, requested=None):
        if requested is None:
            requested = enabled
        state = self.transport.residency.key_lock_status(sample_id)
        return (
            state.effective is enabled
            and state.requested is requested
            and not state.pending
            and bool(state.error)
        )

    def unload(self, sample_id):
        self.transport.residency.cancel(sample_id)
        self.audio.unload_sample(sample_id)
        self.project.sample_paths[sample_id] = None

    def scalar_request(self, sample_id):
        pending = self.transport.residency._pending[sample_id]
        assert pending.scalar
        assert pending.scalar_submitted
        assert pending.ticket is None
        assert isinstance(pending.scalar_request, int)
        assert pending.scalar_request > 0
        return pending.scalar_request


class Probe(_ControllerFixture):
    """Assertions against real public controller and callback feedback."""

    def assert_pending(self, sample_id, enabled):
        state = self.transport.residency.key_lock_status(sample_id)
        assert state.requested is enabled
        assert state.pending

    def assert_identical_pending(self, sample_id, enabled):
        pending = self.transport.residency._pending[sample_id]
        before = (pending.ticket, pending.deadline, pending.attempts)
        for _ in range(8):
            self.local_mode(sample_id, enabled)
        current = self.transport.residency._pending[sample_id]
        assert current is pending
        assert (current.ticket, current.deadline, current.attempts) == before

    def assert_modes(self, expected):
        for sample_id, enabled in expected:
            assert self.settled(sample_id, enabled), (sample_id, enabled)

    def assert_mixed(self):
        assert self.transport.global_params.key_lock_status().mixed

    def assert_failed_target(self, sample_id):
        state = self.transport.residency.key_lock_status(sample_id)
        assert state.error, state
        assert state.effective is False, state
        assert self.transport.global_params.key_lock_status().error

    def assert_native_failure(self, sample_id, request_id, enabled, requested=None):
        feedback = self.audio.pad_key_lock_status(sample_id)
        assert feedback is not None, feedback
        assert feedback["request_id"] == request_id, feedback
        assert feedback["effective"] is enabled, feedback
        assert feedback["ready"] is False, feedback
        assert feedback["state"] == "error", feedback
        assert feedback["error"], feedback
        assert self.failure_settled(sample_id, enabled, requested)

    def assert_stopped_armed(self, sample_id):
        feedback = self.audio.pad_key_lock_status(sample_id)
        assert feedback is not None, feedback
        assert feedback["request_id"] > 0, feedback
        assert feedback["state"] == "armed", feedback
        assert feedback["error"] is None, feedback
        assert feedback["effective"] is True, feedback
        assert feedback["ready"] is True, feedback
        assert sample_id not in self.session.active_sample_ids
        state = self.transport.residency.key_lock_status(sample_id)
        assert state.requested is True, state
        assert self.settled(sample_id, enabled=True), state

    def assert_native_wet(self, sample_id, request_id):
        feedback = self.audio.pad_key_lock_status(sample_id)
        assert feedback is not None, feedback
        assert feedback["request_id"] == request_id, feedback
        assert feedback["state"] == "wet", feedback
        assert feedback["error"] is None, feedback
        assert feedback["effective"] is True, feedback
        assert feedback["ready"] is True, feedback
        assert self.settled(sample_id, enabled=True)

    def assert_live_waiting(self, sample_id, request_id):
        feedback = self.audio.pad_key_lock_status(sample_id)
        assert feedback is not None, feedback
        assert feedback["request_id"] == request_id, feedback
        assert feedback["state"] == "waiting", feedback
        assert feedback["error"] is None, feedback
        assert feedback["effective"] is False, feedback
        assert feedback["ready"] is False, feedback
        state = self.transport.residency.key_lock_status(sample_id)
        assert state.requested is True, state
        assert state.effective is False, state
        assert state.pending, state
        assert not state.error, state
        assert not state.unconfirmed, state
        # The shared renderer consumes this selector's pending field to show
        # "Preparing ON". Runtime warming needs no synthetic controller request.
        assert self.selectors.key_lock_status(sample_id) == state
        assert sample_id not in self.transport.residency._pending

    def assert_new_source_dry(self, sample_id, old_generation):
        feedback = self.audio.pad_key_lock_status(sample_id)
        assert feedback is not None
        assert feedback["source_generation"] != old_generation
        assert feedback["effective"] is False
        assert feedback["request_id"] == 0
        assert self.transport.residency.key_lock_status(sample_id).effective is False

    def assert_scalar_enqueue_is_pending(self, sample_id):
        request = self.scalar_request(sample_id)
        feedback = self.audio.pad_key_lock_status(sample_id)
        assert feedback is not None
        assert feedback["request_id"] != request
        state = self.transport.residency.key_lock_status(sample_id)
        assert state.pending
        assert state.effective is False
        assert state.requested is True
