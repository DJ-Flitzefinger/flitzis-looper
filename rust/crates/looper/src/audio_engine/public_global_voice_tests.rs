//! Public GLOBAL acts on independently audible inherited voices, preserving explicit peers.
//! This bounded fixture uses one-frame scheduled callbacks and the production retirement worker.
//! Expected PCM, clocks, Native adapters and EQ state are independently owned by each pad.

#[path = "public_shared_restore_global_tests.rs"]
mod public_shared_restore_global_tests;

#[path = "keylock_paused_controller_tests.rs"]
mod keylock_paused_controller_tests;

use super::*;
use crate::audio_engine::dsp::{DspNodeSlot, DspParameterId, DspParameterSlot, PerPadDspChain};
use crate::audio_engine::mixer::prepared_native_mixer_tests::finite_lifecycle_tests::{
    AlgebraicClock, RawChronology,
};
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::voice_slot::VoiceSlot;
use crate::messages::{ControlParameterMessage, ResidentSourceView};
use std::sync::Weak;

const IDS: [usize; 4] = [0, 36, 72, 215];
const INHERITED: [usize; 2] = [0, 36];
const HISTORY: usize = 4096;
type PreviousOnOwners = (usize, Weak<ResidentSourceView>, Weak<[f32]>);

fn fixture_eq_normalized(db: f32) -> f32 {
    assert!((-12.0..=4.0).contains(&db));
    if db < 0.0 {
        0.5 * 10.0_f32.powf(db / 20.0)
    } else {
        0.5 + 0.5 * db / 6.0
    }
}

fn voice(harness: &Harness, id: usize) -> &VoiceSlot {
    harness
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(id))
        .unwrap_or_else(|| panic!("pad {id} lost its own public-started voice"))
}

fn voice_mut(harness: &mut Harness, id: usize) -> &mut VoiceSlot {
    harness
        .callback
        .mixer
        .voices
        .iter_mut()
        .find(|voice| voice.is_playing_sample(id))
        .unwrap_or_else(|| panic!("pad {id} lost its own public-started voice"))
}

fn identity(harness: &Harness, id: usize) -> [u64; 4] {
    Python::attach(|py| {
        let status = harness
            .engine
            .borrow(py)
            .pad_key_lock_status(py, id)
            .unwrap()
            .unwrap();
        [
            "source_generation",
            "source_identity",
            "window_revision",
            "request_id",
        ]
        .map(|field| {
            status
                .bind(py)
                .get_item(field)
                .unwrap()
                .unwrap()
                .extract()
                .unwrap()
        })
    })
}

fn accepted(ticket: &Py<ResidentWindowTicket>) -> bool {
    Python::attach(|py| ticket.borrow(py).publication_status() == "accepted")
}

fn pending(harness: &Harness, id: usize, enabled: bool) {
    Python::attach(|py| {
        harness
            .probe
            .call_method1(py, "assert_pending", (id, enabled))
            .unwrap();
    });
}

fn native_wet(harness: &Harness, id: usize) {
    let own = identity(harness, id);
    Python::attach(|py| {
        harness
            .probe
            .call_method1(py, "assert_native_wet", (id, own[3]))
            .unwrap();
    });
}

struct FrozenVoice {
    generation: u64,
    playback: SourcePlayback,
    loop_region: Option<crate::audio_engine::source_reader::FrameRange>,
    paused: bool,
    volume: f32,
    native: usize,
    fifo: (usize, usize),
    history: String,
    dsp: String,
    receipt: [u64; 4],
    mode: bool,
}

impl FrozenVoice {
    fn capture(harness: &Harness, id: usize) -> Self {
        let own = voice(harness, id);
        Self {
            generation: own.generation,
            playback: own.source_playback,
            loop_region: own.source_loop_region,
            paused: own.paused,
            volume: own.volume,
            native: own.stretch.native_state_address(),
            fifo: own.stretch.pending_fifo_frames(),
            history: format!("{:?}", own.stretch.productive_history()),
            dsp: format!(
                "{:?}",
                harness.callback.mixer.dsp_source_history_for_test(id)
            ),
            receipt: identity(harness, id),
            mode: harness.callback.mixer.key_lock_for_measurement(id),
        }
    }

    fn assert_source_eq(&self, harness: &Harness, id: usize) {
        let own = voice(harness, id);
        assert_eq!(own.generation, self.generation, "pad {id} retriggered");
        assert!(
            self.playback.matches_exact(&own.source_playback),
            "pad {id} mode ACK moved its source clock"
        );
        assert_eq!(own.source_loop_region, self.loop_region);
        assert_eq!(own.paused, self.paused);
        assert_eq!(own.volume, self.volume);
        assert_eq!(
            format!(
                "{:?}",
                harness.callback.mixer.dsp_source_history_for_test(id)
            ),
            self.dsp,
            "pad {id} mode ACK reset EQ history"
        );
    }

    fn assert_exact(&self, harness: &Harness, id: usize) {
        self.assert_source_eq(harness, id);
        let own = voice(harness, id);
        assert_eq!(own.stretch.native_state_address(), self.native);
        assert_eq!(own.stretch.pending_fifo_frames(), self.fifo);
        assert_eq!(
            format!("{:?}", own.stretch.productive_history()),
            self.history
        );
        assert_eq!(identity(harness, id), self.receipt, "pad {id} borrowed ACK");
        assert_eq!(
            harness.callback.mixer.key_lock_for_measurement(id),
            self.mode
        );
    }
}

struct Candidate {
    target: u64,
    raw: RawChronology,
    address: Option<usize>,
}

struct ExpectedVoice {
    id: usize,
    generation: u64,
    receipt: [u64; 4],
    source: SampleBuffer,
    start: usize,
    end: usize,
    clock: AlgebraicClock,
    raw: RawChronology,
    filter: PerPadDspChain,
    paused: bool,
    enabled: bool,
    warm: usize,
    raw_fed: usize,
    consumed: usize,
    candidate_needed: bool,
    candidate: Option<Candidate>,
    native: usize,
    adoptions: usize,
    energy: f32,
    wet_difference: f32,
}

impl ExpectedVoice {
    fn new(harness: &Harness, id: usize, enabled: bool) -> Self {
        Self::new_at_range(harness, id, enabled, START, END)
    }

    fn new_at_range(harness: &Harness, id: usize, enabled: bool, start: usize, end: usize) -> Self {
        assert!(start < end && end <= harness.mono.len());
        let raw = RawChronology::new(RATIO, false);
        let own = voice(harness, id);
        assert_eq!(own.volume, 1.0, "public START must keep this pad audible");
        assert!(!own.paused);
        let receipt = identity(harness, id);
        assert!(receipt[0] != 0 && receipt[1] != 0 && receipt[3] != 0);
        Self {
            id,
            generation: own.generation,
            receipt,
            // Copy serialized original PCM, not the adopted resident reader/Native output.
            source: SampleBuffer {
                channels: 1,
                residency: None,
                samples: harness.mono.clone().into(),
            },
            start,
            end,
            clock: AlgebraicClock::physical(start, end, RATIO),
            warm: if enabled { raw.block_size() } else { 0 },
            raw,
            filter: PerPadDspChain::new(id, RATE as f32, 2),
            paused: false,
            enabled,
            raw_fed: 0,
            consumed: 0,
            candidate_needed: enabled,
            candidate: None,
            native: own.stretch.native_state_address(),
            adoptions: 0,
            energy: 0.0,
            wet_difference: 0.0,
        }
    }

    fn prepare_ordinary_start(&mut self, catchup_frames: usize) {
        if self.enabled {
            // Derive the periodic pre-T past from independent original PCM. The
            // declared .73 rate returns to T after 100 physical loop periods.
            let mut history_clock = AlgebraicClock::physical(self.start, self.end, RATIO);
            history_clock.advance((self.end - self.start) * 100 - HISTORY);
            self.raw = RawChronology::new(RATIO, true);
            self.raw.render(&self.source, &mut history_clock, HISTORY);
            let (frame, fraction, explicit) = history_clock.physical_position(0);
            assert_eq!(frame, self.start);
            assert!(fraction.abs() < 1e-8 && !explicit);
            self.warm = 0;
            self.raw_fed = HISTORY;
            self.candidate_needed = false;
            self.adoptions = 1;
        }
        // Keep the canonical T clock separate from the periodic prehistory clock.
        // The missed suffix reaches E through the same independent Native/FIFO
        // and EQ state; it contributes no claim about actually emitted energy.
        let missed = if self.enabled {
            self.raw_fed += catchup_frames;
            self.raw
                .render(&self.source, &mut self.clock, catchup_frames)
        } else {
            self.clock.dry(&self.source, catchup_frames)
        };
        for sample in missed {
            self.filter.begin_frame();
            for channel in 0..2 {
                self.filter.process_sample(channel, sample);
            }
        }
        self.consumed = catchup_frames;
    }

    fn set_mode(&mut self, harness: &Harness, enabled: bool) {
        assert_ne!(self.enabled, enabled);
        self.enabled = enabled;
        self.receipt = identity(harness, self.id);
        self.candidate = None;
        self.candidate_needed = enabled;
        self.adoptions = 0;
        self.raw_fed = 0;
        self.raw = RawChronology::new(RATIO, false);
        self.warm = if enabled { self.raw.block_size() } else { 0 };
        // The real ON admission can exchange a dirty reserve; it does not move the clock/EQ.
        self.native = voice(harness, self.id).stretch.native_state_address();
    }

    fn next(&mut self, frame: u64) -> Option<(f32, [f32; 2])> {
        if self.paused {
            return None;
        }
        if self.enabled && self.candidate_needed {
            let mut preparation_clock = self.clock.clone();
            let mut raw = RawChronology::new(RATIO, true);
            raw.render(&self.source, &mut preparation_clock, HISTORY);
            self.candidate = Some(Candidate {
                target: frame + HISTORY as u64,
                raw,
                address: None,
            });
            self.candidate_needed = false;
        }
        if self
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate.target == frame)
        {
            let candidate = self.candidate.take().unwrap();
            self.native = candidate.address.expect("own candidate was not prepared");
            self.raw = candidate.raw;
            self.raw_fed = HISTORY;
            self.warm = 0;
            self.adoptions += 1;
        }
        let dry = self.clock.clone().dry(&self.source, 1)[0];
        let chosen = if self.enabled {
            let native = self.raw.render(&self.source, &mut self.clock, 1)[0];
            self.raw_fed += 1;
            if self.warm != 0 {
                self.warm -= 1;
                dry
            } else {
                self.wet_difference += (native - dry).abs();
                native
            }
        } else {
            self.clock.dry(&self.source, 1)[0]
        };
        self.consumed += 1;
        self.filter.begin_frame();
        let filtered = std::array::from_fn(|channel| self.filter.process_sample(channel, chosen));
        self.energy += filtered[0].abs();
        Some((chosen, filtered))
    }

    fn assert_voice(&self, harness: &Harness, emitted_wet: bool) {
        let own = voice(harness, self.id);
        assert_eq!(
            own.generation, self.generation,
            "pad {} retriggered",
            self.id
        );
        assert_eq!(own.volume, 1.0);
        assert_eq!(own.paused, self.paused);
        assert_eq!(own.stretch.native_state_address(), self.native);
        let (frame, fraction, explicit) = self.clock.physical_position(0);
        let actual = own.source_playback.position();
        assert_eq!(actual.frame, frame, "pad {} own source frame", self.id);
        assert!((actual.fraction - fraction).abs() < 1e-8);
        assert!(!explicit);
        assert_eq!(
            actual.seek_mode,
            crate::audio_engine::source_reader::ExplicitSeekMode::Normal
        );
        assert_eq!(own.source_playback.tempo_ratio(), RATIO);
        assert_eq!(
            harness.callback.mixer.loop_region_frames(self.id),
            (self.start, Some(self.end))
        );
        if !self.paused {
            if self.enabled {
                let block = self.raw.block_size();
                let input = self.raw_fed % block;
                assert_eq!(
                    own.stretch.pending_fifo_frames(),
                    (input, block - 1 - input)
                );
                assert_eq!(own.stretch.wet_render_ready(), self.raw.used);
                if emitted_wet {
                    assert!(own.stretch.wet_output_ready());
                }
                let history = own.stretch.productive_history().unwrap();
                assert_eq!(history.fed_output_frames, self.raw_fed as u64);
                assert_eq!(history.next_position.frame, frame);
                assert!((history.next_position.fraction - fraction).abs() < 1e-8);
                assert_eq!(
                    history.binding.source_address,
                    own.sample.as_ref().unwrap().source_address()
                );
                assert_eq!(
                    history.binding.resident,
                    own.sample.as_ref().unwrap().resident_binding()
                );
                assert_eq!(
                    own.stretch.wet_current_source_output_ready(),
                    self.adoptions != 0 && emitted_wet
                );
            } else {
                assert_eq!(own.stretch.pending_fifo_frames(), (0, 0));
                assert!(own.stretch.productive_history().is_none());
                assert!(!own.stretch.wet_output_ready());
                assert!(!own.stretch.wet_current_source_output_ready());
            }
        }
        let dsp = harness
            .callback
            .mixer
            .dsp_source_history_for_test(self.id)
            .unwrap();
        assert_eq!(dsp.fed_output_frames, self.consumed as u64);
        assert_eq!(dsp.next_position.frame, frame);
        assert!((dsp.next_position.fraction - fraction).abs() < 1e-8);
    }
}

struct GlobalVoices {
    harness: Harness,
    expected: Vec<ExpectedVoice>,
    shared_pcm_pairs: Vec<(usize, usize)>,
}

impl GlobalVoices {
    fn new() -> Self {
        let mut harness = Harness::new(false, &IDS);
        harness.local(72, true);
        harness.settle(&[72], true, false);
        harness.local(215, false);
        harness.settle(&[215], false, false);
        Python::attach(|py| harness.engine.borrow_mut(py).set_speed(RATIO).unwrap());
        harness.drain();
        let mut result = Self {
            harness,
            expected: Vec::new(),
            shared_pcm_pairs: Vec::new(),
        };
        for (id, gap, enabled, eq) in [
            (0, 53, false, [-12.0, -3.0, 4.0]),
            (36, 94, false, [-6.0, 2.0, -4.0]),
            (72, 71, true, [-9.0, -1.0, 3.0]),
            (215, 193, false, [-3.0, -5.0, 1.0]),
        ] {
            let original_frame = result.harness.frame;
            result.harness.target("start", id);
            let original = Python::attach(|py| {
                result
                    .harness
                    .probe
                    .call_method1(py, "original_start", (id,))
                    .unwrap()
                    .extract::<PyRef<crate::audio_engine::pad_start::CapturedPadStart>>(py)
                    .unwrap()
                    .clone()
            });
            let timestamp = original.received_timestamp();
            wait_until(Duration::from_secs(10), || {
                result.harness.drain();
                assert_eq!(result.harness.frame, original_frame);
                assert_ne!(
                    original.state(),
                    "rejected",
                    "pad {id} original START: {:?}, T={:?}, E={:?}",
                    original.reason(),
                    original.original_frame(),
                    original.execution_frame()
                );
                if original.state() == "applied" {
                    return true;
                }
                let runtime = result
                    .harness
                    .callback
                    .mixer
                    .pitch_runtime
                    .as_mut()
                    .unwrap();
                runtime.poll();
                runtime.has_ready_action(original.sequence())
            });
            assert_eq!(original.original_frame(), Some(original_frame));
            let execution = original.execution_frame().unwrap();
            assert_eq!(execution, original_frame + if enabled { 8192 } else { 0 });
            if execution > original_frame {
                assert!(
                    !result
                        .harness
                        .callback
                        .mixer
                        .voices
                        .iter()
                        .any(|voice| voice.is_playing_sample(id)),
                    "pad {id} became active before its original E"
                );
                assert_eq!(
                    result.harness.callback.scheduler.peek_next_target_frame(),
                    Some(execution)
                );
                // Existing owners consume the real preparation lead through their
                // complete PCM/EQ/clock oracles. The new owner has no samples yet.
                result.render((execution - original_frame) as usize);
            }
            let callback = result.harness.callback.as_mut();
            render_scheduled_audio(
                &mut callback.mixer,
                &mut callback.scheduler,
                &mut [],
                &mut [0.0; NUM_SAMPLES],
                execution,
                2,
                &mut callback.transport,
                &mut callback.feedback,
                &mut callback.retirement,
            );
            result.harness.call0("poll");
            assert_eq!(original.original_frame(), Some(original_frame));
            assert_eq!(original.effective_frame(), Some(execution));
            assert_eq!(original.received_timestamp(), timestamp);
            let catchup = (execution - original_frame) as usize;
            let mut expected = ExpectedVoice::new(&result.harness, id, enabled);
            expected.prepare_ordinary_start(catchup);
            let (frame, fraction, explicit) = expected.clock.physical_position(0);
            let actual = voice(&result.harness, id);
            assert_eq!(actual.source_playback.position().frame, frame);
            assert!((actual.source_playback.position().fraction - fraction).abs() < 1e-8);
            assert!(!explicit);
            if enabled {
                let actual = voice(&result.harness, id);
                assert_eq!(
                    actual.stretch.adopted_request_id(),
                    Some(original.sequence())
                );
                assert_eq!(
                    actual
                        .stretch
                        .productive_history()
                        .unwrap()
                        .fed_output_frames,
                    (HISTORY + catchup) as u64
                );
            }
            // Actual typed parameter queue is fixture setup; public Mode/Transport routes
            // remain connected to the real AudioEngine throughout both histories.
            Python::attach(|py| {
                let engine = result.harness.engine.borrow(py);
                engine
                    .test_parameter_producer
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .push(ControlParameterMessage::SetPadEq {
                        id,
                        low_db: eq[0],
                        mid_db: eq[1],
                        high_db: eq[2],
                    })
                    .unwrap();
            });
            for (slot, db) in [
                DspParameterSlot::Slot0,
                DspParameterSlot::Slot1,
                DspParameterSlot::Slot2,
            ]
            .into_iter()
            .zip(eq)
            {
                assert!(expected.filter.set_parameter(
                    DspParameterId::per_pad(id, DspNodeSlot::Slot0, slot).unwrap(),
                    fixture_eq_normalized(db),
                ));
            }
            result.expected.push(expected);
            result.harness.drain();
            result.render(gap);
        }
        let mut sharing = Vec::new();
        for (index, &left) in IDS.iter().enumerate() {
            for &right in &IDS[index + 1..] {
                let banks = result.harness.callback.mixer.bank_for_measurement();
                let shared = Arc::ptr_eq(
                    &banks[left].as_ref().unwrap().samples,
                    &banks[right].as_ref().unwrap().samples,
                );
                sharing.push((left, right, shared));
                if shared {
                    result.shared_pcm_pairs.push((left, right));
                }
            }
        }
        println!("public GLOBAL actual bank PCM sharing: {sharing:?}");
        result.assert_origins();
        result
    }

    fn own_mut(&mut self, id: usize) -> &mut ExpectedVoice {
        self.expected.iter_mut().find(|own| own.id == id).unwrap()
    }

    fn assert_origins(&self) {
        for (id, explicit) in [(0, None), (36, None), (72, Some(true)), (215, Some(false))] {
            self.harness.assert_origin(id, explicit);
        }
    }

    fn render(&mut self, frames: usize) {
        for _ in 0..frames {
            let start = self.harness.frame;
            let expected: Vec<_> = self
                .expected
                .iter_mut()
                .map(|own| own.next(start))
                .collect();
            let mut actual = [0.0; 2];
            let mut peaks = [0.0; NUM_SAMPLES];
            let callback = self.harness.callback.as_mut();
            render_scheduled_audio(
                &mut callback.mixer,
                &mut callback.scheduler,
                &mut actual,
                &mut peaks,
                start,
                2,
                &mut callback.transport,
                &mut callback.feedback,
                &mut callback.retirement,
            );
            assert_eq!(
                self.harness
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .filter(|voice| voice.active)
                    .count(),
                self.expected.len(),
                "public mode operation added or stopped a voice"
            );
            let mut sum = [0.0; 2];
            for (own, emitted) in self.expected.iter().zip(expected) {
                assert_eq!(
                    self.harness
                        .callback
                        .mixer
                        .voices
                        .iter()
                        .filter(|voice| voice.is_playing_sample(own.id))
                        .count(),
                    1
                );
                if let Some((raw, filtered)) = emitted {
                    let buffers = voice(&self.harness, own.id).stretch.output_buffers();
                    for channel in 0..2 {
                        assert!(
                            (buffers[channel][0] - raw).abs() < 5e-5,
                            "pad {} raw at absolute {start}: {} != {raw}",
                            own.id,
                            buffers[channel][0]
                        );
                        sum[channel] += filtered[channel];
                    }
                    assert!(
                        (peaks[own.id] - filtered[0].abs().max(filtered[1].abs())).abs() < 5e-5
                    );
                    own.assert_voice(
                        &self.harness,
                        own.enabled && own.raw_fed > own.raw.block_size(),
                    );
                } else {
                    assert_eq!(peaks[own.id], 0.0);
                    own.assert_voice(&self.harness, false);
                }
            }
            for channel in 0..2 {
                assert!(
                    (actual[channel] - sum[channel]).abs() < 2e-4,
                    "independent audible sum at {start}: {} != {}",
                    actual[channel],
                    sum[channel]
                );
            }
            self.harness.frame += 1;
            // Stop output time after the exact first feed. Each worker must prepare its
            // independently computed target before callback time reaches that target.
            self.freeze_new_candidates();
        }
        self.harness.call0("poll");
    }

    fn freeze_new_candidates(&mut self) {
        for own in &mut self.expected {
            let Some(candidate) = own.candidate.as_mut().filter(|item| item.address.is_none())
            else {
                continue;
            };
            let live = voice_mut(&mut self.harness, own.id);
            wait_until(Duration::from_secs(10), || {
                live.stretch.source_preparation_ready()
            });
            assert_eq!(
                live.stretch.prepared_target_output_frame(),
                Some(candidate.target)
            );
            let address = live.stretch.prepared_native_address();
            assert_ne!(address, 0);
            assert_ne!(
                address, own.native,
                "pad {} candidate borrowed its current Native",
                own.id
            );
            candidate.address = Some(address);
            let block = candidate.raw.block_size();
            assert_eq!(
                live.stretch.prepared_fifo_frames(),
                Some((HISTORY % block, block - 1 - HISTORY % block))
            );
            if own.start == 0 && own.end == own.source.frame_count() {
                assert!(live.stretch.prepared_tap_observation().is_none());
            } else {
                let taps = live.stretch.prepared_tap_observation().unwrap();
                assert_eq!(taps.left_reads, HISTORY * 2);
                assert_eq!(taps.missing_reads, 0);
                assert!(taps.min_frame.is_some_and(|frame| frame >= own.start));
                assert!(taps.max_frame.is_some_and(|frame| frame < own.end));
            }
        }
    }

    fn pause(&mut self, id: usize, paused: bool) {
        let before = FrozenVoice::capture(&self.harness, id);
        self.harness
            .target(if paused { "pause" } else { "resume" }, id);
        self.harness.drain();
        assert_eq!(voice(&self.harness, id).paused, paused);
        assert!(
            before
                .playback
                .matches_exact(&voice(&self.harness, id).source_playback)
        );
        assert_eq!(voice(&self.harness, id).generation, before.generation);
        self.own_mut(id).paused = paused;
    }

    fn global_mode(&mut self, enabled: bool) -> Vec<Py<ResidentWindowTicket>> {
        let frozen: Vec<_> = IDS.map(|id| FrozenVoice::capture(&self.harness, id)).into();
        self.harness.callback.retirement.capacity = 0;
        self.harness.global(enabled);
        let tickets: Vec<_> = INHERITED
            .map(|id| {
                pending(&self.harness, id, enabled);
                self.harness.ticket(id)
            })
            .into();
        wait_until(Duration::from_secs(10), || {
            self.harness.drain();
            Python::attach(|py| {
                tickets
                    .iter()
                    .all(|ticket| ticket.borrow(py).publication_status() == "pending")
            })
        });
        for (id, before) in IDS.into_iter().zip(&frozen) {
            before.assert_exact(&self.harness, id);
        }
        for ticket in &tickets {
            assert!(
                !accepted(ticket),
                "request enqueue falsely became its own ACK"
            );
        }
        self.harness.callback.retirement.capacity = usize::MAX;
        wait_until(Duration::from_secs(10), || {
            self.harness.drain();
            tickets.iter().all(accepted)
        });
        for (id, ticket) in INHERITED.into_iter().zip(&tickets) {
            let before = &frozen[IDS.iter().position(|&candidate| candidate == id).unwrap()];
            before.assert_source_eq(&self.harness, id);
            let window: u64 = Python::attach(|py| {
                ticket
                    .bind(py)
                    .getattr("window_revision")
                    .unwrap()
                    .extract()
                    .unwrap()
            });
            let own = identity(&self.harness, id);
            assert_eq!(
                own,
                [
                    before.receipt[0],
                    before.receipt[1],
                    window,
                    self.harness.mode_request(ticket)
                ]
            );
            assert!(Python::attach(|py| ticket.borrow(py).is_current()));
            assert_eq!(
                self.harness.callback.mixer.key_lock_for_measurement(id),
                enabled
            );
            let expected = self.expected.iter_mut().find(|own| own.id == id).unwrap();
            expected.set_mode(&self.harness, enabled);
            if !enabled {
                assert_eq!(
                    voice(&self.harness, id).stretch.pending_fifo_frames(),
                    (0, 0)
                );
                assert!(
                    voice(&self.harness, id)
                        .stretch
                        .productive_history()
                        .is_none()
                );
                assert!(!voice(&self.harness, id).stretch.wet_output_ready());
            }
        }
        frozen[2].assert_exact(&self.harness, 72);
        frozen[3].assert_exact(&self.harness, 215);
        self.assert_origins();
        tickets
    }

    fn assert_receipts(&self) {
        for own in &self.expected {
            assert_eq!(
                identity(&self.harness, own.id),
                own.receipt,
                "pad {} lost its own receipt",
                own.id
            );
        }
    }

    fn old_on_views(&self) -> Vec<PreviousOnOwners> {
        INHERITED
            .map(|id| {
                let sample = self.harness.callback.mixer.bank_for_measurement()[id]
                    .as_ref()
                    .unwrap();
                assert_eq!(
                    sample.residency.as_ref().unwrap().context,
                    ResidentContext::KeyLockFiniteLoop
                );
                assert!(Arc::ptr_eq(
                    &sample.samples,
                    &voice(&self.harness, id).sample.as_ref().unwrap().samples
                ));
                (
                    id,
                    Arc::downgrade(sample.residency.as_ref().unwrap()),
                    Arc::downgrade(&sample.samples),
                )
            })
            .into()
    }

    fn retire_old_views(&mut self, old: &[PreviousOnOwners]) {
        // FIFO marker proves the production audio-retirement queue drained; the mode
        // view can additionally be held by its genuine off-thread preparation lane.
        let marker = Arc::new(AtomicU8::new(0));
        let marker_done = Arc::downgrade(&marker);
        self.harness
            .callback
            .retirement
            .retire_cold_adoption(marker);
        wait_until(Duration::from_secs(10), || {
            self.harness.drain();
            Python::attach(|py| {
                crate::audio_engine::resident_relocation::reconcile(&self.harness.engine.borrow(py))
                    .unwrap()
            });
            marker_done.strong_count() == 0
                && old.iter().all(|(_, view, _)| view.strong_count() == 0)
        });
        for (id, view, pcm) in old {
            assert_eq!(
                view.strong_count(),
                0,
                "pad {id} retained its obsolete ON view"
            );
            let bank = self.harness.callback.mixer.bank_for_measurement()[*id]
                .as_ref()
                .unwrap();
            assert_eq!(
                bank.residency.as_ref().unwrap().context,
                ResidentContext::FiniteLoop
            );
            assert!(Arc::ptr_eq(
                &bank.samples,
                &voice(&self.harness, *id).sample.as_ref().unwrap().samples
            ));
            assert!(
                Weak::ptr_eq(pcm, &Arc::downgrade(&bank.samples)),
                "pad {id} copied shared immutable PCM on OFF"
            );
            assert!(pcm.strong_count() > 0, "own dry source remains in use");
            // Only assert cross-pad sharing when it was actually observed at public
            // births. Distinct source loads are allowed to own distinct PCM allocations.
            for &(left, right) in &self.shared_pcm_pairs {
                let other = if left == *id {
                    right
                } else if right == *id {
                    left
                } else {
                    continue;
                };
                if [72, 215].contains(&other) {
                    let peer = self.harness.callback.mixer.bank_for_measurement()[other]
                        .as_ref()
                        .unwrap();
                    assert!(Arc::ptr_eq(&bank.samples, &peer.samples));
                    assert!(Arc::ptr_eq(
                        &peer.samples,
                        &voice(&self.harness, other).sample.as_ref().unwrap().samples
                    ));
                    assert!(Weak::ptr_eq(pcm, &Arc::downgrade(&peer.samples)));
                }
            }
        }
    }

    fn assert_audible(&self) {
        for own in &self.expected {
            assert!(
                own.energy > 1.0,
                "pad {} had no independently audible contribution",
                own.id
            );
        }
        let peer = self.expected.iter().find(|own| own.id == 72).unwrap();
        assert!(peer.wet_difference > 10.0 && peer.adoptions >= 1);
    }
}

#[test]
fn actual_public_global_inherited_running_paused_voices_preserve_explicit_audible_peers() {
    let mut voices = GlobalVoices::new();
    voices.render(5000);
    native_wet(&voices.harness, 72);
    voices.assert_receipts();
    voices.pause(36, true);
    let paused_source = voice(&voices.harness, 36).source_playback;
    let tickets = voices.global_mode(true);
    // Paused ON has its own armed ACK; that does not claim real wet output.
    Python::attach(|py| {
        let status = voices
            .harness
            .engine
            .borrow(py)
            .pad_key_lock_status(py, 36)
            .unwrap()
            .unwrap();
        let status = status.bind(py);
        assert_eq!(
            status
                .get_item("state")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "armed"
        );
        assert!(
            status
                .get_item("effective")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert!(
            status
                .get_item("ready")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
    });
    assert!(voices.harness.settled(36, true));
    assert!(!voice(&voices.harness, 36).stretch.wet_output_ready());
    assert!(
        !voice(&voices.harness, 36)
            .stretch
            .wet_current_source_output_ready()
    );
    let paused_on = FrozenVoice::capture(&voices.harness, 36);
    voices.render(513);
    paused_on.assert_exact(&voices.harness, 36);
    assert!(paused_source.matches_exact(&voice(&voices.harness, 36).source_playback));
    voices.pause(36, false);
    voices.render(6000);
    for id in [0, 36, 72] {
        native_wet(&voices.harness, id);
    }
    for own in voices
        .expected
        .iter()
        .filter(|own| INHERITED.contains(&own.id))
    {
        assert!(own.adoptions >= 1 && own.wet_difference > 10.0);
    }
    voices.assert_receipts();
    voices.pause(36, true);
    let old = voices.old_on_views();
    let off_tickets = voices.global_mode(false);
    // The actual OFF ACK resets this paused Native FIFO, while source and EQ stay fixed.
    let paused_off = FrozenVoice::capture(&voices.harness, 36);
    voices.render(257);
    paused_off.assert_exact(&voices.harness, 36);
    drop(tickets);
    drop(off_tickets);
    voices.retire_old_views(&old);
    voices.pause(36, false);
    voices.render(2048);
    voices.assert_receipts();
    assert!(voices.harness.settled(0, false) && voices.harness.settled(36, false));
    native_wet(&voices.harness, 72);
    assert!(voices.harness.settled(215, false));
    voices.assert_audible();
    voices.assert_origins();
}

#[test]
fn actual_public_global_failed_inherited_reserve_and_superseded_ticket_keep_other_voices_audible() {
    let mut voices = GlobalVoices::new();
    voices.render(5000);
    let before: Vec<_> = IDS
        .map(|id| FrozenVoice::capture(&voices.harness, id))
        .into();
    voices.harness.callback.retirement.capacity = 0;
    voices.harness.global(true);
    let stale = voices.harness.ticket(0);
    let failed_ticket = voices.harness.ticket(36);
    let failed_request = voices.harness.mode_request(&failed_ticket);
    pending(&voices.harness, 0, true);
    pending(&voices.harness, 36, true);
    // Real local override supersedes only 0's unclaimed global intent; clearing it
    // creates 0's fresh inherited ON. Other targets cannot use either stale receipt.
    voices.harness.local(0, false);
    voices.harness.assert_origin(0, Some(false));
    voices.harness.target("clear_mode", 0);
    let current = voices.harness.ticket(0);
    assert!(voices.harness.mode_request(&current) > voices.harness.mode_request(&stale));
    assert!(!Python::attach(|py| stale.borrow(py).is_current()));
    wait_until(Duration::from_secs(10), || {
        voices.harness.drain();
        Python::attach(|py| {
            current.borrow(py).publication_status() == "pending"
                && failed_ticket.borrow(py).publication_status() == "pending"
        })
    });
    for (id, frozen) in IDS.into_iter().zip(&before) {
        frozen.assert_exact(&voices.harness, id);
    }
    // Exhaust only this voice's actual warmed reserve. A failed shared worker would
    // deliberately affect every mixer lane and could not model one target failure.
    voice_mut(&mut voices.harness, 36)
        .stretch
        .exhaust_warmed_reserve();
    voices.harness.callback.retirement.capacity = usize::MAX;
    wait_until(Duration::from_secs(10), || {
        voices.harness.drain();
        accepted(&current)
            && Python::attach(|py| {
                matches!(
                    failed_ticket.borrow(py).publication_status(),
                    "rejected" | "failed"
                )
            })
    });
    before[0].assert_source_eq(&voices.harness, 0);
    let request = voices.harness.mode_request(&current);
    let window: u64 = Python::attach(|py| {
        current
            .bind(py)
            .getattr("window_revision")
            .unwrap()
            .extract()
            .unwrap()
    });
    assert_eq!(
        identity(&voices.harness, 0),
        [before[0].receipt[0], before[0].receipt[1], window, request]
    );
    assert!(!Python::attach(|py| stale.borrow(py).is_current()));
    let own = voices.expected.iter_mut().find(|own| own.id == 0).unwrap();
    own.set_mode(&voices.harness, true);
    // Failed 36 keeps its source/EQ and dry Native state; its own error may be
    // published on the first render boundary, never interpreted as 0's success.
    before[1].assert_source_eq(&voices.harness, 36);
    assert!(!accepted(&failed_ticket));
    let failed_identity = identity(&voices.harness, 36);
    assert_eq!(
        failed_identity,
        [
            before[1].receipt[0],
            before[1].receipt[1],
            before[1].receipt[2],
            failed_request
        ]
    );
    voices.own_mut(36).receipt = failed_identity;
    before[2].assert_exact(&voices.harness, 72);
    before[3].assert_exact(&voices.harness, 215);
    voices.render(6000);
    Python::attach(|py| {
        voices
            .harness
            .probe
            .call_method1(
                py,
                "assert_native_failure",
                (36, failed_request, false, true),
            )
            .unwrap();
        voices
            .harness
            .probe
            .call_method1(py, "assert_failed_target", (36,))
            .unwrap();
    });
    assert_eq!(identity(&voices.harness, 36), failed_identity);
    assert!(!voices.harness.callback.mixer.key_lock_for_measurement(36));
    assert!(!voice(&voices.harness, 36).stretch.wet_mode_armed());
    assert!(!voice(&voices.harness, 0).stretch.wet_mode_failed());
    assert!(!voice(&voices.harness, 72).stretch.wet_mode_failed());
    native_wet(&voices.harness, 0);
    native_wet(&voices.harness, 72);
    let successful = voices.expected.iter().find(|own| own.id == 0).unwrap();
    assert!(successful.wet_difference > 10.0 && successful.adoptions >= 1);
    voices.assert_receipts();
    voices.assert_origins();
    voices.assert_audible();
    drop(stale);
    drop(current);
    drop(failed_ticket);
    // Continue real output after failure and settled feedback, without retrigger.
    voices.render(1024);
    voices.assert_receipts();
}
