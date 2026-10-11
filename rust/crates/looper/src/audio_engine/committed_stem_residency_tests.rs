//! Actual committed-set ACKs and active first component residency, without a device.
//! The complete PCM reference is prepared independently before the finite live
//! source is admitted. It never supplies publication authority to the real pad.

use super::*;
use crate::audio_engine::committed_stem_residency::{CommittedStemSetTicket, ResidentStemTicket};
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::dsp::{DspNodeSlot, DspParameterId, DspParameterSlot, PerPadDspChain};
use crate::audio_engine::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::resident_relocation::{
    WindowRequest, prepare_window_with_producer, reconcile,
};
use crate::audio_engine::rubberband_backend::{RubberBandLiveShifter, pitch_scale_for_tempo_ratio};
use crate::audio_engine::source_reader::{
    reset_tap_observation_for_test, tap_observation_for_test,
};
use crate::messages::StemMixMode;
use std::collections::VecDeque;

#[path = "l3_controller_productive_tests.rs"]
mod l3_controller_productive_tests;
#[path = "l3_native_tests.rs"]
mod l3_native_tests;
#[path = "offline_stem_productive_tests.rs"]
mod offline_stem_productive_tests;

const LOOP_START: usize = 17;
const LOOP_END: usize = 1517;

fn signal_wav(frames: usize, layer: usize) -> Vec<u8> {
    let mut encoded = stereo_wav(frames);
    for frame in 0..frames {
        for channel in 0..2 {
            let phase = frame as f64 * (0.031 + layer as f64 * 0.017) + channel as f64 * 0.41;
            let value = (phase.sin() * (1700.0 + layer as f64 * 500.0)).round() as i16;
            let offset = 44 + (frame * 2 + channel) * 2;
            encoded[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
    }
    encoded
}

fn signal_sum_wav(frames: usize, layers: &[usize]) -> Vec<u8> {
    let components: Vec<_> = layers
        .iter()
        .map(|layer| signal_wav(frames, *layer))
        .collect();
    let mut encoded = stereo_wav(frames);
    for offset in (44..encoded.len()).step_by(2) {
        let sum: i32 = components
            .iter()
            .map(|component| {
                i32::from(i16::from_le_bytes(
                    component[offset..offset + 2].try_into().unwrap(),
                ))
            })
            .sum();
        let sum = i16::try_from(sum).expect("the genuine component mix fits PCM16");
        encoded[offset..offset + 2].copy_from_slice(&sum.to_le_bytes());
    }
    encoded
}

fn write_signal_set(path: &Path, frames: usize, version: &str, variation: usize) {
    fs::create_dir_all(path).unwrap();
    let mut hashes = serde_json::Map::new();
    for (index, name) in STEM_FILE_NAMES.iter().enumerate() {
        let encoded = if index == 4 {
            signal_sum_wav(frames, &[1 + variation, 2 + variation, 3 + variation])
        } else {
            signal_wav(frames, index + variation)
        };
        fs::write(path.join(format!("{name}.wav")), &encoded).unwrap();
        hashes.insert(
            (*name).to_owned(),
            json!(format!("{:x}", Sha256::digest(encoded))),
        );
    }
    fs::write(
        path.join(".complete.json"),
        serde_json::to_vec(&json!({
            "schema":"stem-set-sha256-v1", "source_version":version, "stems":hashes,
        }))
        .unwrap(),
    )
    .unwrap();
}

fn long_harness() -> Harness {
    let mut h = Harness::new();
    let original = h.root.join("committed-residency.wav");
    // The real complete-pair alignment compares the source with the sum of
    // vocals/melody/bass/drums. Supply actual matching mix content, rather than
    // an unrelated tone whose best correlation can move this finite loop into
    // aligned zero padding.
    fs::write(&original, signal_sum_wav(4096, &[0, 1, 2, 3])).unwrap();
    let material = prepare_material(&h.root, &original, RATE, 2, &|| false).unwrap();
    let version = canonical_version(&material);
    let wav_reference = canonical_wav_reference(&material);
    let wav_path = h.root.parent().unwrap().join(&wav_reference);
    write_signal_set(&wav_path, material.sample.frame_count(), &version, 0);
    let preparation = control::prepared_for_test(
        &h.engine,
        PreparedMigrationMaterial::from_current(
            &h.root,
            material.sample.clone(),
            material.lease.clone(),
        )
        .unwrap(),
        &h.root,
    )
    .unwrap();
    let requests: [_; 2] = [0, 215].map(|id| {
        control::adopt_for_format(
            &h.engine,
            id,
            &preparation,
            h.producer.clone(),
            (2, RATE, h.root.clone()),
        )
        .unwrap()
    });
    wait_until(|| h.consumer.slots() >= 2);
    assert_eq!(h.callback.drain(&mut h.consumer), 2);
    source_successes(
        &h.engine,
        &[requests[0].request_id(), requests[1].request_id()],
    );
    preparation.release_preparation().unwrap();
    h._original_owner = h
        .engine
        .project_assets
        .acquire(&h.root, &material.lease.original_path)
        .unwrap();
    h.material = material;
    h.version = version;
    h.wav_reference = wav_reference;
    h.wav_path = wav_path;
    h
}

fn voice(mixer: &RtMixer) -> &crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn voice_mut(mixer: &mut RtMixer) -> &mut crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter_mut()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn register(h: &mut Harness, pair: &PreparedStemPair) -> CommittedStemSetTicket {
    let source = h.ticket(0);
    let selected = h
        .engine
        .register_committed_stem_pair_with_producer(pair, &source, &h.producer)
        .unwrap();
    assert_eq!(selected.publication_status(), "pending");
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(
        selected.publication_status(),
        "accepted",
        "{:?}",
        selected.error()
    );
    assert!(selected.is_current());
    assert!(h.callback.mixer.stems_for_measurement()[0].is_none());
    selected
}

struct Productive {
    h: Harness,
    _saved: crate::audio_engine::project_assets::ProjectAssetLease,
    pair: PreparedStemPair,
    selected: CommittedStemSetTicket,
    reference: Box<RtMixer>,
    reference_ownership: Arc<InputRuntimeOwnership>,
    frame: u64,
    key_lock: bool,
}

impl Productive {
    fn new(key_lock: bool, ratio: f64) -> Self {
        let mut h = long_harness();
        let source = h.ticket(0);
        let oracle_pair = h.prepare(0, &source, true, None);
        assert_eq!(
            oracle_pair
                .pair_reader
                .descriptor
                .content
                .alignment
                .offset_frames,
            0
        );
        for component in oracle_pair.component_views_for_test().unwrap() {
            assert!(
                component.samples[LOOP_START * 2..LOOP_END * 2]
                    .iter()
                    .any(|sample| sample.abs() > 0.04),
                "actual aligned loop component must contain signal"
            );
        }
        let selection = selection(&oracle_pair);
        let saved = h.save(&oracle_pair);
        let full = h.material.sample.clone();
        let full_stems = PreparedStemSet {
            complete_set_identity: Arc::new([71; 32]),
            reference_samples: full.samples.clone(),
            publication: PreparedSourcePermit::unrestricted(),
            accepted_timing: None,
            source_version_hash: crate::audio_engine::stem_cache::source_version_hash(&h.version),
            sample_rate_hz: RATE,
            channels: 2,
            frame_count: full.frame_count(),
            available_mask: 0b1111,
            stems: oracle_pair.component_views_for_test().unwrap().clone(),
        };
        drop(oracle_pair);

        let window = prepare_window_with_producer(
            &h.engine,
            0,
            WindowRequest {
                key_lock: Some(key_lock),
                loop_region: Some((
                    LOOP_START as f64 / f64::from(RATE),
                    Some(LOOP_END as f64 / f64::from(RATE)),
                )),
                ..WindowRequest::default()
            },
            h.producer.clone(),
        )
        .unwrap();
        wait_until(|| h.consumer.peek().is_ok() || window.error().unwrap().is_some());
        assert_eq!(
            window.publication_status(),
            "pending",
            "{:?}",
            window.error()
        );
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert_eq!(window.publication_status(), "accepted");
        reconcile(&h.engine).unwrap();
        let resident = h.callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
        assert_eq!(resident.resident_start(), LOOP_START);
        assert_eq!(resident.resident_end(), LOOP_END);
        assert_eq!(resident.samples.len(), (LOOP_END - LOOP_START) * 2);
        assert_eq!(h.callback.mixer.key_lock_for_measurement(0), key_lock);
        let source = h.ticket(0);
        let pair = h.prepare(0, &source, false, Some(&selection));
        assert!(!pair.has_components());
        let selected = register(&mut h, &pair);

        let mut reference = Box::new(RtMixer::new(2, RATE as f32));
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        ownership.publish_source(0, &full, RATE, 1);
        reference.set_input_runtime_ownership(ownership.clone());
        reference.load_sample(0, full);
        assert!(reference.publish_prepared_stems(0, full_stems));
        reference.set_loop_region_frames(0, LOOP_START, Some(LOOP_END));
        for mixer in [&mut h.callback.mixer, reference.as_mut()] {
            mixer.set_speed(ratio);
            mixer.set_pad_key_lock(0, key_lock);
            mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
            assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
        }
        let mut fixture = Self {
            h,
            _saved: saved,
            pair,
            selected,
            reference,
            reference_ownership: ownership,
            frame: 0,
            key_lock,
        };
        if key_lock {
            fixture.render(13, true);
            fixture.adopt_native();
        } else {
            fixture.render(137, false);
        }
        fixture
    }

    fn render(&mut self, frames: usize, timestamp: bool) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        let mut expected = vec![0.0; frames * 2];
        let callback = self.h.callback.as_mut();
        reset_tap_observation_for_test();
        if timestamp {
            callback.mixer.render_rt_at_output_frame(
                &mut output,
                &mut [0.0; NUM_SAMPLES],
                self.frame,
                &mut callback.retirement,
            );
        } else {
            callback.mixer.render_rt(
                &mut output,
                &mut [0.0; NUM_SAMPLES],
                &mut callback.retirement,
            );
        }
        let taps = tap_observation_for_test();
        assert_eq!(taps.missing_reads, 0);
        // Checked finite addressing observes real taps even when ALL's
        // enabled component sum is empty. Zero feed is asserted separately.
        assert!(taps.min_frame.is_some_and(|frame| frame >= LOOP_START));
        assert!(taps.max_frame.is_some_and(|frame| frame < LOOP_END));
        if timestamp {
            self.reference.render_at_output_frame(
                self.frame,
                &mut expected,
                &mut [0.0; NUM_SAMPLES],
            );
        } else {
            self.reference
                .render(&mut expected, &mut [0.0; NUM_SAMPLES]);
        }
        assert_eq!(
            output, expected,
            "actual finite continuation at {}",
            self.frame
        );
        assert!(
            voice(&callback.mixer)
                .source_playback
                .matches_exact(&voice(&self.reference).source_playback)
        );
        assert_eq!(
            voice(&callback.mixer).stretch.pending_fifo_frames(),
            voice(&self.reference).stretch.pending_fifo_frames()
        );
        self.frame += frames as u64;
        output
    }

    fn adopt_native(&mut self) {
        wait_until(|| {
            voice_mut(&mut self.h.callback.mixer)
                .stretch
                .source_preparation_ready()
                && voice_mut(&mut self.reference)
                    .stretch
                    .source_preparation_ready()
        });
        let target = voice_mut(&mut self.h.callback.mixer)
            .stretch
            .prepared_target_output_frame()
            .unwrap();
        assert_eq!(
            voice_mut(&mut self.reference)
                .stretch
                .prepared_target_output_frame(),
            Some(target)
        );
        let prepared = voice_mut(&mut self.h.callback.mixer)
            .stretch
            .prepared_native_address();
        let previous = voice(&self.h.callback.mixer).stretch.native_state_address();
        assert_ne!(prepared, previous);
        assert_ne!(prepared, 0);
        let taps = voice_mut(&mut self.h.callback.mixer)
            .stretch
            .prepared_tap_observation()
            .unwrap();
        assert_eq!(taps.missing_reads, 0);
        assert!(taps.min_frame.is_some_and(|frame| frame >= LOOP_START));
        assert!(taps.max_frame.is_some_and(|frame| frame < LOOP_END));
        let mut partitions = [31_usize, 777, 1, 257, 512].into_iter().cycle();
        while self.frame < target {
            let count = partitions
                .next()
                .unwrap()
                .min((target - self.frame) as usize);
            self.render(count, true);
        }
        let mut output = self.render(137, true);
        assert_eq!(
            voice(&self.h.callback.mixer).stretch.native_state_address(),
            prepared
        );
        // A successful wet swap proves ownership, while the backend's first
        // short suffix may still contain its startup envelope. Keep the exact
        // comparison for that suffix and prove productive signal across eight
        // actual Native blocks without resetting Native, FIFO, source or EQ.
        let mut remaining = voice(&self.h.callback.mixer)
            .stretch
            .rubberband_block_size()
            * 8;
        for frames in [1_usize, 31, 137, 257, 512].into_iter().cycle() {
            let frames = frames.min(remaining);
            output.extend(self.render(frames, true));
            assert_eq!(
                voice(&self.h.callback.mixer).stretch.native_state_address(),
                prepared
            );
            remaining -= frames;
            if remaining == 0 {
                break;
            }
        }
        assert!(
            output.iter().any(|value| value.abs() > 0.001),
            "actual Native post-adoption signal: key_lock={} ratio={} demand={:?} fifos={:?} wet={} source_wet={} peak={} block={} nominal_delay={} actual_feed_peak={}",
            self.key_lock,
            voice(&self.h.callback.mixer).source_playback.tempo_ratio(),
            self.h.callback.mixer.stem_demand_for_measurement(0),
            voice(&self.h.callback.mixer).stretch.pending_fifo_frames(),
            voice(&self.h.callback.mixer).stretch.wet_output_ready(),
            voice(&self.h.callback.mixer)
                .stretch
                .wet_current_source_output_ready(),
            output
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs())),
            voice(&self.h.callback.mixer)
                .stretch
                .rubberband_block_size(),
            voice(&self.h.callback.mixer)
                .stretch
                .rubberband_start_delay(),
            voice(&self.h.callback.mixer)
                .stretch
                .varispeed_buffers()
                .iter()
                .flat_map(|channel| channel.iter())
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs())),
        );
    }

    fn request(&mut self, mode: &str, mask: u8) -> ResidentStemTicket {
        self.h
            .engine
            .request_committed_stem_residency_with_producer(
                &self.selected,
                mode,
                mask,
                &self.h.producer,
            )
            .unwrap()
    }

    fn capture_then_wait(&mut self, ticket: &ResidentStemTicket) {
        assert_eq!(ticket.effective_mode_status(), "waiting");
        assert_eq!(self.h.callback.drain(&mut self.h.consumer), 1);
        wait_until(|| self.h.consumer.peek().is_ok() || ticket.error().is_some());
        assert_eq!(ticket.residency_status(), "pending", "{:?}", ticket.error());
        assert_eq!(ticket.effective_mode_status(), "waiting");
    }

    fn select_reference(&mut self, mode: &str, mask: u8) {
        let hash = crate::audio_engine::stem_cache::source_version_hash(&self.h.version);
        if mode == "all_stems" {
            assert!(self.reference.set_stem_enabled_mask(0, mask, hash));
            assert!(
                self.reference
                    .set_stem_mix_mode(0, StemMixMode::AllStems, hash)
            );
        } else {
            assert!(self.reference.set_stem_mix_mode(0, StemMixMode::FullMix, 0));
        }
    }

    fn render_paused(&mut self, frames: usize) {
        let before = voice(&self.h.callback.mixer).source_playback;
        let transition = self.h.callback.mixer.stem_transition_for_measurement(0);
        let native = voice(&self.h.callback.mixer).stretch.native_state_address();
        let fifos = voice(&self.h.callback.mixer).stretch.pending_fifo_frames();
        let mut output = vec![0.0; frames * 2];
        let mut expected = output.clone();
        reset_tap_observation_for_test();
        let callback = self.h.callback.as_mut();
        callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            self.frame,
            &mut callback.retirement,
        );
        let taps = tap_observation_for_test();
        assert_eq!(taps.missing_reads, 0);
        assert_eq!(taps.min_frame, None);
        assert_eq!(taps.max_frame, None);
        self.reference
            .render_at_output_frame(self.frame, &mut expected, &mut [0.0; NUM_SAMPLES]);
        assert_eq!(output, expected);
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert!(voice(&callback.mixer).paused);
        assert!(
            voice(&callback.mixer)
                .source_playback
                .matches_exact(&before)
        );
        assert!(
            voice(&callback.mixer)
                .source_playback
                .matches_exact(&voice(&self.reference).source_playback,)
        );
        assert!(
            callback
                .mixer
                .stem_transition_for_measurement(0)
                .matches_exact(transition)
        );
        assert_eq!(
            voice(&callback.mixer).stretch.native_state_address(),
            native
        );
        assert_eq!(voice(&callback.mixer).stretch.pending_fifo_frames(), fifos);
        self.frame += frames as u64;
    }
}

#[test]
fn committed_selection_uses_own_callback_ack_and_keeps_full_mix_components_absent() {
    let mut h = Harness::new();
    let first_source = h.ticket(0);
    let pair = h.prepare(0, &first_source, false, None);
    let _saved = h.save(&pair);
    let first = h
        .engine
        .register_committed_stem_pair_with_producer(&pair, &first_source, &h.producer)
        .unwrap();
    assert_eq!(first.publication_status(), "pending");
    assert!(h.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(first.publication_status(), "accepted");
    assert!(first.is_current());
    let other_source = h.ticket(215);
    let other_pair = h.prepare(215, &other_source, false, Some(&selection(&pair)));
    let other = h
        .engine
        .register_committed_stem_pair_with_producer(&other_pair, &other_source, &h.producer)
        .unwrap();
    assert_eq!(other.publication_status(), "pending");
    assert_eq!(first.publication_status(), "accepted");
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(other.publication_status(), "accepted");
    assert!(first.is_current() && other.is_current());
    assert!(h.callback.mixer.stems_for_measurement()[0].is_none());
    assert!(h.callback.mixer.stems_for_measurement()[215].is_none());
}

#[test]
fn active_first_committed_residency_keeps_source_native_fifos_and_real_128_frame_ramp() {
    for key_lock in [false, true] {
        for ratio in [0.73, 1.371_234_567_890_123] {
            let mut p = Productive::new(key_lock, ratio);
            let sample = p.h.callback.mixer.bank_for_measurement()[0]
                .clone()
                .unwrap();
            let generation = voice(&p.h.callback.mixer).generation;
            let native = voice(&p.h.callback.mixer).stretch.native_state_address();
            let pending = p.request("all_stems", 0b1010);
            assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
            p.capture_then_wait(&pending);
            // A prepared worker return is insufficient: FullMix remains productive
            // until this exact request receives its own callback residency ACK.
            p.render(31, false);
            assert_eq!(
                p.h.callback.mixer.stem_demand_for_measurement(0).0,
                StemMixMode::FullMix
            );
            assert_eq!(pending.effective_mode_status(), "waiting");
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            assert_eq!(pending.residency_status(), "accepted");
            assert_eq!(pending.effective_mode_status(), "transitioning");
            assert!(pending.is_current());
            assert!(p.selected.is_current());
            let actual = p.h.callback.mixer.bank_for_measurement()[0]
                .as_ref()
                .unwrap();
            assert!(Arc::ptr_eq(&sample.samples, &actual.samples));
            assert_eq!(sample.resident_binding(), actual.resident_binding());
            assert_eq!(voice(&p.h.callback.mixer).generation, generation);
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                native
            );
            let resident = p.h.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap();
            assert!(resident.stems.iter().all(|stem| stem.same_window(&sample)));
            assert_eq!(resident.stems.len(), 4);
            p.select_reference("all_stems", 0b1010);
            let mut elapsed = 0;
            let mut partitions = [1_usize, 31, 17, 7].into_iter().cycle();
            while elapsed as f64 * ratio < 128.0 {
                let remaining = (128.0 / ratio).ceil() as usize - elapsed;
                let frames = partitions.next().unwrap().min(remaining);
                p.render(frames, false);
                elapsed += frames;
                assert_eq!(
                    voice(&p.h.callback.mixer).stretch.native_state_address(),
                    native
                );
                if elapsed as f64 * ratio < 128.0 {
                    assert_eq!(pending.effective_mode_status(), "transitioning");
                }
            }
            assert_eq!(
                pending.effective_mode_status(),
                if p.key_lock {
                    "transitioning"
                } else {
                    "effective"
                }
            );
            assert!(
                !p.h.callback
                    .mixer
                    .stem_transition_for_measurement(0)
                    .is_active()
            );
            let output = p.render(777, false);
            assert!(output.iter().any(|value| value.abs() > 0.001));
            if p.key_lock {
                // The next genuine own history request and Native/FIFO exchange
                // must still work after active None->Some plus its real ramp.
                p.render(13, true);
                p.adopt_native();
                assert_eq!(pending.effective_mode_status(), "effective");
            }
        }
    }
}

#[test]
fn paused_same_selection_residency_waits_for_real_source_frames_after_resume() {
    for key_lock in [false, true] {
        let ratio = 0.75;
        let mut p = Productive::new(key_lock, ratio);
        let sample = p.h.callback.mixer.bank_for_measurement()[0]
            .clone()
            .unwrap();
        let generation = voice(&p.h.callback.mixer).generation;
        let native = voice(&p.h.callback.mixer).stretch.native_state_address();
        p.h.callback.mixer.pause_sample_at_output_frame(0, p.frame);
        p.reference.pause_sample_at_output_frame(0, p.frame);
        let pending = p.request("all_stems", 0b1010);
        assert!(p.selected.is_current());
        assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
        p.capture_then_wait(&pending);
        for frames in [1, 127, 384] {
            for _ in 0..32 {
                assert_eq!(pending.residency_status(), "pending");
                assert_eq!(pending.effective_mode_status(), "waiting");
            }
            p.render_paused(frames);
            assert_eq!(
                p.h.callback.mixer.stem_demand_for_measurement(0).0,
                StemMixMode::FullMix
            );
            assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
        }
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        assert_eq!(pending.residency_status(), "accepted");
        assert_eq!(pending.effective_mode_status(), "transitioning");
        p.select_reference("all_stems", 0b1010);
        assert!(
            p.h.callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
        for frames in [1, 127, 384, 512] {
            for _ in 0..32 {
                assert_eq!(pending.residency_status(), "accepted");
                assert_eq!(pending.effective_mode_status(), "transitioning");
                assert!(pending.is_current());
            }
            p.render_paused(frames);
            assert_eq!(pending.effective_mode_status(), "transitioning");
            assert!(
                p.h.callback
                    .mixer
                    .stem_transition_for_measurement(0)
                    .is_active()
            );
            assert_eq!(voice(&p.h.callback.mixer).generation, generation);
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                native
            );
            let current = p.h.callback.mixer.bank_for_measurement()[0]
                .as_ref()
                .unwrap();
            assert!(Arc::ptr_eq(&sample.samples, &current.samples));
            assert_eq!(sample.resident_binding(), current.resident_binding());
        }
        p.h.callback.mixer.resume_sample_at_output_frame(0, p.frame);
        p.reference.resume_sample_at_output_frame(0, p.frame);
        let mut elapsed = 0;
        let total = (128.0 / ratio).ceil() as usize;
        for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
            let frames = frames.min(total - elapsed);
            p.render(frames, false);
            elapsed += frames;
            assert_eq!(
                voice(&p.h.callback.mixer).stretch.native_state_address(),
                native
            );
            if elapsed == total {
                break;
            }
            assert_eq!(pending.effective_mode_status(), "transitioning");
        }
        assert!(
            !p.h.callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
        assert_eq!(
            pending.effective_mode_status(),
            if key_lock {
                "transitioning"
            } else {
                "effective"
            }
        );
        if key_lock {
            p.render(13, true);
            p.adopt_native();
            assert_eq!(pending.effective_mode_status(), "effective");
        }
        assert_eq!(voice(&p.h.callback.mixer).generation, generation);
        assert!(p.selected.is_current());
    }
}

#[test]
fn active_full_mix_release_then_all_requires_fresh_residency_ack_for_same_selection() {
    let mut p = Productive::new(false, 0.73);
    let source = p.h.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let first = p.request("all_stems", 0b1111);
    p.capture_then_wait(&first);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    p.select_reference("all_stems", 0b1111);
    p.render(257, false);
    assert_eq!(first.effective_mode_status(), "effective");
    let old = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .stems
        .each_ref()
        .map(|stem| Arc::downgrade(&stem.samples));
    let full_mix = p.request("full_mix", 0b1111);
    p.capture_then_wait(&full_mix);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(full_mix.effective_mode_status(), "transitioning");
    p.select_reference("full_mix", 0b1111);
    p.h.callback.retirement.slots = 0;
    p.render(257, false);
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_some());
    assert!(old.iter().all(|reader| reader.strong_count() > 0));
    p.h.callback.retirement.slots = usize::MAX;
    p.render(1, false);
    assert_eq!(full_mix.effective_mode_status(), "effective");
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    p.h.callback.retirement.retired.clear();
    reconcile(&p.h.engine).unwrap();
    assert!(old.iter().all(|reader| reader.strong_count() == 0));
    assert!(p.selected.is_current());
    let next = p.request("all_stems", 0b0101);
    assert_eq!(next.effective_mode_status(), "waiting");
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    p.capture_then_wait(&next);
    p.render(31, false);
    assert_eq!(next.effective_mode_status(), "waiting");
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(next.residency_status(), "accepted");
    assert_eq!(next.effective_mode_status(), "transitioning");
    let resident = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    for (previous, current) in old.iter().zip(&resident.stems) {
        assert!(!previous.ptr_eq(&Arc::downgrade(&current.samples)));
    }
    let current = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&source.samples, &current.samples));
    assert_eq!(source.resident_binding(), current.resident_binding());
    p.select_reference("all_stems", 0b0101);
    p.render(257, false);
    assert_eq!(next.effective_mode_status(), "effective");
    assert!(p.selected.is_current());
}

#[test]
fn committed_registration_queue_pressure_and_cancel_cannot_grant_native_selection() {
    let mut h = Harness::new();
    let source = h.ticket(0);
    let pair = h.prepare(0, &source, false, None);
    let _saved = h.save(&pair);
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    producer.push(ControlMessage::Ping()).unwrap();
    let producer = Arc::new(Mutex::new(producer));
    assert!(
        h.engine
            .register_committed_stem_pair_with_producer(&pair, &source, &producer)
            .is_err()
    );
    assert_eq!(source.publication_status(), "captured");
    assert!(matches!(consumer.pop().unwrap(), ControlMessage::Ping()));
    assert!(consumer.pop().is_err());
    let cancelled = h
        .engine
        .register_committed_stem_pair_with_producer(&pair, &source, &h.producer)
        .unwrap();
    assert_eq!(cancelled.publication_status(), "pending");
    assert!(cancelled.cancel());
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(cancelled.publication_status(), "rejected");
    assert!(!cancelled.is_current());
    assert!(h.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(
        h.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    assert_complete_selection(&h.root, &selection(&pair));
    let retry = register(&mut h, &pair);
    assert!(retry.is_current());
    assert!(!cancelled.is_current());
}

#[test]
fn active_residency_cancel_and_stop_reject_late_own_worker_return() {
    for stop in [false, true] {
        let mut p = Productive::new(false, 0.73);
        let source = p.h.callback.mixer.bank_for_measurement()[0]
            .clone()
            .unwrap();
        let pending = p.request("all_stems", 0b1111);
        p.capture_then_wait(&pending);
        if stop {
            assert!(
                crate::audio_engine::input_runtime_binding::enqueue_stop_with_producer(
                    &p.h.engine.input_runtime_ownership,
                    &mut p.h.producer.lock().unwrap(),
                    Some(0),
                )
            );
        } else {
            assert!(pending.cancel());
        }
        // STOP revokes the request before its queued component publication can
        // run, even though the older worker message precedes STOP in the ring.
        assert!(p.h.callback.drain(&mut p.h.consumer) >= 1);
        assert_eq!(pending.residency_status(), "rejected");
        assert_eq!(pending.effective_mode_status(), "rejected");
        assert!(!pending.is_current());
        assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
        let current = p.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(&source.samples, &current.samples));
        assert!(p.selected.is_current());
        assert_eq!(
            p.h.callback.mixer.stem_demand_for_measurement(0).0,
            StemMixMode::FullMix
        );
        if stop {
            assert!(
                p.h.callback
                    .mixer
                    .voices
                    .iter()
                    .all(|voice| !voice.is_playing_sample(0))
            );
        } else {
            p.render(257, false);
        }
    }
}

#[test]
fn captured_voice_or_history_epoch_change_rejects_late_components_without_touching_current_owners()
{
    for history_change in [false, true] {
        let mut p = Productive::new(false, 0.75);
        let launches = if history_change {
            None
        } else {
            // Admit both actual launch configurations before the residency
            // reservation fences new starts. Applying that already held launch
            // after Capture is the genuine serial generation-change race.
            let callback = p.h.callback.as_mut();
            let actual = callback
                .mixer
                .prepare_play_sample_rt(0, 1.0, p.frame, &mut callback.retirement)
                .unwrap();
            let expected = p
                .reference
                .prepare_play_sample_rt(
                    0,
                    1.0,
                    p.frame,
                    &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
                )
                .unwrap();
            Some((actual, expected))
        };
        let pending = p.request("all_stems", 0b1010);
        p.capture_then_wait(&pending);
        if history_change {
            voice_mut(&mut p.h.callback.mixer)
                .stretch
                .invalidate_prepared();
            voice_mut(&mut p.reference).stretch.invalidate_prepared();
        } else {
            let (actual, expected) = launches.unwrap();
            let callback = p.h.callback.as_mut();
            assert!(
                callback
                    .mixer
                    .play_prepared_sample_rt(actual, &mut callback.retirement)
            );
            assert!(p.reference.play_prepared_sample_rt(
                expected,
                &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement
            ));
        }
        let source = p.h.callback.mixer.bank_for_measurement()[0]
            .clone()
            .unwrap();
        let generation = voice(&p.h.callback.mixer).generation;
        let playback = voice(&p.h.callback.mixer).source_playback;
        let native = voice(&p.h.callback.mixer).stretch.native_state_address();
        let fifos = voice(&p.h.callback.mixer).stretch.pending_fifo_frames();
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        assert_eq!(pending.residency_status(), "rejected");
        assert_eq!(pending.effective_mode_status(), "rejected");
        assert!(pending.error().is_some());
        assert!(p.selected.is_current());
        assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
        assert_eq!(
            p.h.callback.mixer.stem_demand_for_measurement(0).0,
            StemMixMode::FullMix
        );
        let current = p.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(&source.samples, &current.samples));
        assert_eq!(source.resident_binding(), current.resident_binding());
        assert_eq!(voice(&p.h.callback.mixer).generation, generation);
        assert!(
            voice(&p.h.callback.mixer)
                .source_playback
                .matches_exact(&playback)
        );
        assert_eq!(
            voice(&p.h.callback.mixer).stretch.native_state_address(),
            native
        );
        assert_eq!(
            voice(&p.h.callback.mixer).stretch.pending_fifo_frames(),
            fifos
        );
        p.render(257, false);
    }
}

#[test]
fn retirement_backpressure_defers_own_adopt_and_late_cancel_prevents_every_effect() {
    let mut p = Productive::new(false, 0.75);
    let pending = p.request("all_stems", 0b1010);
    p.capture_then_wait(&pending);
    let source = p.h.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let playback = voice(&p.h.callback.mixer).source_playback;
    let generation = voice(&p.h.callback.mixer).generation;
    let native = voice(&p.h.callback.mixer).stretch.native_state_address();
    let fifos = voice(&p.h.callback.mixer).stretch.pending_fifo_frames();
    p.h.callback.retirement.slots = 2;
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 0);
    assert_eq!(pending.residency_status(), "pending");
    assert_eq!(pending.effective_mode_status(), "waiting");
    assert!(p.h.consumer.peek().is_ok());
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(
        p.h.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    assert!(
        voice(&p.h.callback.mixer)
            .source_playback
            .matches_exact(&playback)
    );
    assert_eq!(voice(&p.h.callback.mixer).generation, generation);
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.pending_fifo_frames(),
        fifos
    );
    assert!(pending.cancel());
    p.h.callback.retirement.slots = usize::MAX;
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(pending.residency_status(), "rejected");
    assert_eq!(pending.effective_mode_status(), "rejected");
    assert!(pending.error().is_some());
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    let current = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&source.samples, &current.samples));
    assert_eq!(source.resident_binding(), current.resident_binding());
    assert!(
        voice(&p.h.callback.mixer)
            .source_playback
            .matches_exact(&playback)
    );
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.pending_fifo_frames(),
        fifos
    );
    p.render(257, false);
}

#[test]
fn empty_mask_mid_ramp_mask_change_and_full_mix_supersession_preserve_real_rate_smoothing() {
    let mut p = Productive::new(true, 0.75);
    let generation = voice(&p.h.callback.mixer).generation;
    let native = voice(&p.h.callback.mixer).stretch.native_state_address();
    p.h.callback.mixer.set_speed(1.25);
    p.reference.set_speed(1.25);
    p.render(13, false);
    assert_eq!(
        voice(&p.h.callback.mixer).source_playback.rate_target(),
        1.25
    );
    assert!(voice(&p.h.callback.mixer).source_playback.tempo_ratio() < 1.25);
    let empty = p.request("all_stems", 0);
    p.capture_then_wait(&empty);
    p.render(31, false);
    assert_eq!(
        p.h.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(empty.residency_status(), "accepted");
    p.select_reference("all_stems", 0);
    for frames in [1, 31, 257, 127] {
        p.render(frames, false);
    }
    assert!(
        !p.h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert_eq!(p.h.callback.mixer.stem_demand_for_measurement(0).1, 0);
    assert!(
        voice(&p.h.callback.mixer)
            .stretch
            .varispeed_buffers()
            .iter()
            .all(|channel| channel[..127].iter().all(|sample| *sample == 0.0))
    );
    assert_eq!(empty.effective_mode_status(), "transitioning");
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );
    let identity = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .complete_set_identity
        .clone();
    let audible = p.request("all_stems", 0b1010);
    p.capture_then_wait(&audible);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    p.select_reference("all_stems", 0b1010);
    p.render(17, false);
    assert!(
        p.h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    let altered = p.request("all_stems", 0b0101);
    p.capture_then_wait(&altered);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(altered.residency_status(), "accepted");
    assert_eq!(audible.effective_mode_status(), "rejected");
    p.select_reference("all_stems", 0b0101);
    p.render(7, false);
    assert!(
        p.h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    let late_mask = p.request("all_stems", 0b0011);
    p.capture_then_wait(&late_mask);
    // The future FullMix request supersedes the already prepared mask worker;
    // its separate Capture and Adopt still need their own callback work.
    let release = p.request("full_mix", 0b0011);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 2);
    assert_eq!(late_mask.residency_status(), "rejected");
    assert_eq!(late_mask.effective_mode_status(), "rejected");
    assert_eq!(release.effective_mode_status(), "waiting");
    wait_until(|| p.h.consumer.peek().is_ok() || release.error().is_some());
    assert_eq!(
        release.residency_status(),
        "pending",
        "{:?}",
        release.error()
    );
    let current = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&identity, &current.complete_set_identity));
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    p.select_reference("full_mix", 0b0011);
    for frames in [1, 31, 127, 257, 777] {
        p.render(frames, false);
    }
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(voice(&p.h.callback.mixer).generation, generation);
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );
    assert!(voice(&p.h.callback.mixer).source_playback.tempo_ratio() > 0.8);
    assert!(voice(&p.h.callback.mixer).source_playback.tempo_ratio() < 1.25);
    assert_eq!(
        voice(&p.h.callback.mixer).source_playback.rate_target(),
        1.25
    );
}

#[test]
fn accepted_component_residency_native_worker_failure_rejects_only_unproved_effective_receipt() {
    let mut p = Productive::new(true, 0.75);
    let all = p.request("all_stems", 0b1010);
    p.capture_then_wait(&all);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    p.select_reference("all_stems", 0b1010);
    p.render(1, false);
    assert_eq!(all.residency_status(), "accepted");
    assert_eq!(all.effective_mode_status(), "transitioning");
    assert!(
        !voice(&p.h.callback.mixer)
            .stretch
            .wet_current_source_output_ready()
    );
    let sample = p.h.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let components = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .stems
        .clone();
    let identity = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .complete_set_identity
        .clone();
    let generation = voice(&p.h.callback.mixer).generation;
    let playback = voice(&p.h.callback.mixer).source_playback;
    let native = voice(&p.h.callback.mixer).stretch.native_state_address();
    let fifos = voice(&p.h.callback.mixer).stretch.pending_fifo_frames();
    voice(&p.h.callback.mixer).stretch.fail_preparation_worker();
    // Execute the actual bounded receipt-finalization kernel without consuming
    // source frames. Failure revokes no accepted reader or usable Native owner.
    p.h.callback.mixer.finish_committed_stem_mode(0);
    assert_eq!(all.residency_status(), "accepted");
    assert_eq!(all.effective_mode_status(), "rejected");
    assert_eq!(
        all.effective_failure_reason(),
        Some("native-preparation-failed")
    );
    assert!(all.error().is_some());
    let current = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&sample.samples, &current.samples));
    assert_eq!(sample.resident_binding(), current.resident_binding());
    let current = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&identity, &current.complete_set_identity));
    for (before, after) in components.iter().zip(&current.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
    assert_eq!(voice(&p.h.callback.mixer).generation, generation);
    assert!(
        voice(&p.h.callback.mixer)
            .source_playback
            .matches_exact(&playback)
    );
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.pending_fifo_frames(),
        fifos
    );
    p.render(257, false);
    assert_eq!(all.residency_status(), "accepted");
    assert_eq!(all.effective_mode_status(), "rejected");
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );
    p.h.engine.input_runtime_ownership.cancel_launches(0);
    assert!(!all.is_current());
    assert_eq!(
        all.effective_failure_reason(),
        Some("native-preparation-failed")
    );
    assert_eq!(
        all.error().as_deref(),
        Some("Committed stem effective mode Native preparation failed")
    );
}

#[test]
fn stopped_full_mix_ack_releases_component_control_owner_without_claiming_output() {
    let mut h = long_harness();
    let source = h.ticket(0);
    let pair = h.prepare(0, &source, false, None);
    let _saved = h.save(&pair);
    let selected = register(&mut h, &pair);
    let all = h
        .engine
        .request_committed_stem_residency_with_producer(&selected, "all_stems", 0b1010, &h.producer)
        .unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    wait_until(|| h.consumer.peek().is_ok() || all.residency_status() == "rejected");
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    reconcile(&h.engine).unwrap();
    assert_eq!(all.residency_status(), "accepted");
    assert_eq!(all.effective_mode_status(), "waiting");
    h.callback.mixer.finish_committed_stem_mode(0);
    assert_eq!(all.effective_mode_status(), "waiting");
    assert!(h.callback.mixer.stems_for_measurement()[0].is_some());
    assert!(
        h.engine.resident_stem_cache.lock().unwrap()[0]
            .accepted
            .is_some()
    );
    let release = h
        .engine
        .request_committed_stem_residency_with_producer(&selected, "full_mix", 0b1010, &h.producer)
        .unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    wait_until(|| h.consumer.peek().is_ok() || release.residency_status() == "rejected");
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    h.callback.mixer.finish_committed_stem_mode(0);
    reconcile(&h.engine).unwrap();
    assert_eq!(release.residency_status(), "accepted");
    assert!(release.is_current());
    assert_eq!(release.effective_mode_status(), "waiting");
    assert_eq!(release.effective_failure_reason(), None);
    assert_eq!(release.error(), None);
    assert!(h.callback.mixer.stems_for_measurement()[0].is_none());
    assert!(
        h.engine.resident_stem_cache.lock().unwrap()[0]
            .accepted
            .is_none()
    );
    assert!(selected.is_current());
}

#[test]
fn actual_residency_worker_read_registry_and_bounded_peak_failures_preserve_full_mix_owners() {
    for failure in ["component_read", "bounded_peak", "reader_registry"] {
        let mut p = Productive::new(false, 0.75);
        let source = p.h.callback.mixer.bank_for_measurement()[0]
            .clone()
            .unwrap();
        let generation = voice(&p.h.callback.mixer).generation;
        let playback = voice(&p.h.callback.mixer).source_playback;
        let native = voice(&p.h.callback.mixer).stretch.native_state_address();
        let fifos = voice(&p.h.callback.mixer).stretch.pending_fifo_frames();
        match failure {
            "component_read" => p.pair.pair_reader.fail_next_component_read_for_test(),
            "reader_registry" => {
                // Fill the real asset registry with genuine retained complete
                // component readers. Their PCM is already held by the oracle;
                // no forged publication permit or phantom byte count is used.
                let stems = p.reference.stems_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .clone();
                let mut full = false;
                for _ in 0..2048 {
                    match p
                        .h
                        .engine
                        .project_assets
                        .retain_stems(p.h.wav_path.clone(), &stems)
                    {
                        Ok(()) => {}
                        Err(error) => {
                            assert!(
                                error.to_string().contains("reader registry full"),
                                "{error}"
                            );
                            full = true;
                            break;
                        }
                    }
                }
                assert!(full, "the actual fixed reader registry must become full");
            }
            "bounded_peak" => {
                // Snapshot a bounded per-pad test cap into the same actual
                // worker's held+new+scratch calculation. Production retains its
                // fixed 1-GiB cap; this proves that admission/error route only.
                let minimum_peak = source.samples.len() * 20 + 64 * 1024;
                p.h.engine.resident_stem_cache.lock().unwrap()[0]
                    .committed
                    .pcm_limit_for_test = Some(minimum_peak - 1);
            }
            _ => unreachable!(),
        }
        let pending = p.request("all_stems", 0b1010);
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        wait_until(|| pending.error().is_some());
        assert_eq!(
            pending.residency_status(),
            "rejected",
            "{failure}: {:?}",
            pending.error()
        );
        assert_eq!(pending.effective_mode_status(), "rejected");
        let error = pending.error().unwrap();
        assert!(
            match failure {
                "component_read" => error.contains("component") && error.contains("read"),
                "reader_registry" => error.contains("reader registry full"),
                "bounded_peak" => error.contains("transient admission"),
                _ => false,
            },
            "{failure}: {error}"
        );
        assert!(p.h.consumer.peek().is_err());
        assert!(p.selected.is_current());
        assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
        assert_eq!(
            p.h.callback.mixer.stem_demand_for_measurement(0).0,
            StemMixMode::FullMix
        );
        let current = p.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(&source.samples, &current.samples));
        assert_eq!(source.resident_binding(), current.resident_binding());
        assert_eq!(voice(&p.h.callback.mixer).generation, generation);
        assert!(
            voice(&p.h.callback.mixer)
                .source_playback
                .matches_exact(&playback)
        );
        assert_eq!(
            voice(&p.h.callback.mixer).stretch.native_state_address(),
            native
        );
        assert_eq!(
            voice(&p.h.callback.mixer).stretch.pending_fifo_frames(),
            fifos
        );
        p.render(257, false);
    }
}

#[test]
fn active_selected_set_idempotence_does_not_enable_new_generation_publication() {
    let mut p = Productive::new(false, 0.73);
    let source = p.h.ticket(0);
    let same =
        p.h.engine
            .register_committed_stem_pair_with_producer(&p.pair, &source, &p.h.producer)
            .unwrap();
    if same.publication_status() == "pending" {
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    }
    assert_eq!(same.publication_status(), "accepted");
    assert!(same.is_current() && p.selected.is_current());
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    p.render(31, false);

    let reference =
        p.h.wav_reference
            .replace(GENERATION, "fedcba9876543210fedcba9876543210");
    let path = p.h.root.parent().unwrap().join(&reference);
    write_signal_set(&path, p.h.material.sample.frame_count(), &p.h.version, 9);
    let changed_source = p.h.ticket(0);
    let changed =
        p.h.engine
            .prepare_stem_pair_at_root(
                &p.h.root,
                0,
                &p.h.version,
                &reference,
                &changed_source,
                false,
                None,
            )
            .unwrap();
    assert_ne!(selection(&changed), selection(&p.pair));
    let attempt =
        p.h.engine
            .register_committed_stem_pair_with_producer(&changed, &changed_source, &p.h.producer)
            .unwrap();
    assert_eq!(attempt.publication_status(), "pending");
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(attempt.publication_status(), "rejected");
    assert!(attempt.error().is_some());
    assert!(!attempt.is_current());
    assert!(p.selected.is_current());
    p.render(257, false);

    // The old ordinary publication path still rejects the genuine active pad.
    let ordinary_source = p.h.ticket(0);
    let ordinary =
        p.h.prepare(0, &ordinary_source, true, Some(&selection(&p.pair)));
    p.h.engine
        .publish_stem_pair_with_producer(&ordinary, &ordinary_source, &p.h.producer)
        .unwrap();
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(ordinary_source.publication_status(), "rejected");
    assert_eq!(ordinary_source.rejection_reason(), Some("pad-playing"));
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    p.render(257, false);

    // The migration witness can also be an independently accepted ordinary
    // component publication. Its exact bank identity authorizes registration
    // of that same committed set after Play, without replacing any voice.
    let mut h = long_harness();
    let source = h.ticket(0);
    let prepared = h.prepare(0, &source, true, None);
    let saved_selection = selection(&prepared);
    let _saved = h.save(&prepared);
    h.engine
        .publish_stem_pair_with_producer(&prepared, &source, &h.producer)
        .unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(source.publication_status(), "accepted");
    reconcile(&h.engine).unwrap();
    assert!(h.callback.mixer.play_sample_at_output_frame(0, 1.0, 0));
    let old_generation = voice(&h.callback.mixer).generation;
    let old_native = voice(&h.callback.mixer).stretch.native_state_address();
    let source = h.ticket(0);
    let disk_only = h.prepare(0, &source, false, Some(&saved_selection));
    assert!(!disk_only.has_components());
    let registered = h
        .engine
        .register_committed_stem_pair_with_producer(&disk_only, &source, &h.producer)
        .unwrap();
    assert_eq!(registered.publication_status(), "pending");
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(
        registered.publication_status(),
        "accepted",
        "{:?}",
        registered.error()
    );
    assert!(registered.is_current());
    assert_eq!(voice(&h.callback.mixer).generation, old_generation);
    assert_eq!(
        voice(&h.callback.mixer).stretch.native_state_address(),
        old_native
    );
    let other_reference = h
        .wav_reference
        .replace(GENERATION, "89abcdef0123456789abcdef01234567");
    write_signal_set(
        &h.root.parent().unwrap().join(&other_reference),
        h.material.sample.frame_count(),
        &h.version,
        11,
    );
    let fresh_source = h.ticket(0);
    let other = h
        .engine
        .prepare_stem_pair_at_root(
            &h.root,
            0,
            &h.version,
            &other_reference,
            &fresh_source,
            false,
            None,
        )
        .unwrap();
    let rejected = h
        .engine
        .register_committed_stem_pair_with_producer(&other, &fresh_source, &h.producer)
        .unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(rejected.publication_status(), "rejected");
    assert!(registered.is_current());
    assert_eq!(voice(&h.callback.mixer).generation, old_generation);
    assert_eq!(
        voice(&h.callback.mixer).stretch.native_state_address(),
        old_native
    );
}

#[test]
fn distinct_verified_descriptor_registration_has_own_revocation_and_preserves_existing_set_history()
{
    let mut p = Productive::new(true, 0.75);
    let first = p.request("all_stems", 0b1010);
    p.capture_then_wait(&first);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    p.select_reference("all_stems", 0b1010);
    p.render(257, false);
    p.render(13, true);
    p.adopt_native();
    assert_eq!(first.effective_mode_status(), "effective");
    let selected = selection(&p.pair);
    let descriptor = selected_path(&p.h.root, &selected, "descriptor_reference");
    let descriptor_reference = selected["descriptor_reference"].as_str().unwrap();
    let fresh_reference = format!(
        "{}/89abcdef0123456789abcdef01234567.json",
        descriptor_reference.rsplit_once('/').unwrap().0
    );
    let fresh_path = p.h.root.parent().unwrap().join(&fresh_reference);
    assert!(!fresh_path.exists());
    fs::copy(&descriptor, &fresh_path).unwrap();
    let source = p.h.ticket(0);
    let renewal =
        p.h.engine
            .prepare_stem_pair_at_root(
                &p.h.root,
                0,
                &p.h.version,
                &p.h.wav_reference,
                &source,
                false,
                Some(&fresh_reference),
            )
            .unwrap();
    assert_eq!(
        selection(&renewal)["wav_generation"],
        selected["wav_generation"]
    );
    assert_eq!(
        selection(&renewal)["pcm_generation"],
        selected["pcm_generation"]
    );
    assert_ne!(
        selection(&renewal)["descriptor_reference"],
        selected["descriptor_reference"]
    );
    let sample = p.h.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let generation = voice(&p.h.callback.mixer).generation;
    let playback = voice(&p.h.callback.mixer).source_playback;
    let native = voice(&p.h.callback.mixer).stretch.native_state_address();
    let fifos = voice(&p.h.callback.mixer).stretch.pending_fifo_frames();
    let resident = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    let identity = resident.complete_set_identity.clone();
    let components = resident.stems.clone();
    let renewed =
        p.h.engine
            .register_committed_stem_pair_with_producer(&renewal, &source, &p.h.producer)
            .unwrap();
    assert_eq!(renewed.publication_status(), "pending");
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(
        renewed.publication_status(),
        "accepted",
        "{:?}",
        renewed.error()
    );
    assert!(renewed.is_current());
    assert_eq!(voice(&p.h.callback.mixer).generation, generation);
    assert!(
        voice(&p.h.callback.mixer)
            .source_playback
            .matches_exact(&playback)
    );
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.pending_fifo_frames(),
        fifos
    );
    assert!(p.selected.cancel());
    assert!(!p.selected.is_current());
    assert!(renewed.is_current());
    let future =
        p.h.engine
            .request_committed_stem_residency_with_producer(
                &renewed,
                "all_stems",
                0b1010,
                &p.h.producer,
            )
            .unwrap();
    p.capture_then_wait(&future);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(
        future.residency_status(),
        "accepted",
        "{:?}",
        future.error()
    );
    let current = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&sample.samples, &current.samples));
    assert_eq!(sample.resident_binding(), current.resident_binding());
    let resident = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&identity, &resident.complete_set_identity));
    for (before, after) in components.iter().zip(&resident.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
    assert_eq!(voice(&p.h.callback.mixer).generation, generation);
    assert!(
        voice(&p.h.callback.mixer)
            .source_playback
            .matches_exact(&playback)
    );
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.native_state_address(),
        native
    );
    assert_eq!(
        voice(&p.h.callback.mixer).stretch.pending_fifo_frames(),
        fifos
    );
    p.render(31, true);
    assert_eq!(future.effective_mode_status(), "effective");
    assert!(renewed.is_current());
}

#[test]
fn ordered_legacy_full_mix_and_selection_cancel_never_repromote_accepted_typed_components() {
    let mut p = Productive::new(false, 0.75);
    let all = p.request("all_stems", 0b1010);
    p.capture_then_wait(&all);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    p.select_reference("all_stems", 0b1010);
    p.render(257, false);
    assert_eq!(all.residency_status(), "accepted");
    assert_eq!(all.effective_mode_status(), "effective");
    reconcile(&p.h.engine).unwrap();
    assert!(
        p.h.engine.resident_stem_cache.lock().unwrap()[0]
            .accepted
            .is_some()
    );
    let weak = p.h.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .stems
        .each_ref()
        .map(|stem| Arc::downgrade(&stem.samples));
    crate::audio_engine::resident_relocation::set_stem_pair_full_mix_with_producer(
        &p.h.engine,
        0,
        &p.h.producer,
    )
    .unwrap();
    assert!(p.selected.cancel());
    assert!(!p.selected.is_current());
    for _ in 0..32 {
        reconcile(&p.h.engine).unwrap();
        assert!(
            p.h.engine.resident_stem_cache.lock().unwrap()[0]
                .accepted
                .is_none()
        );
    }
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_some());
    assert!(weak.iter().all(|reader| reader.strong_count() > 0));
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    p.select_reference("full_mix", 0b1010);
    p.render(1, false);
    assert!(
        p.h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert!(weak.iter().all(|reader| reader.strong_count() > 0));
    reconcile(&p.h.engine).unwrap();
    assert!(
        p.h.engine.resident_stem_cache.lock().unwrap()[0]
            .accepted
            .is_none()
    );
    p.render(256, false);
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    p.h.callback.retirement.retired.clear();
    reconcile(&p.h.engine).unwrap();
    assert!(
        p.h.engine.resident_stem_cache.lock().unwrap()[0]
            .accepted
            .is_none()
    );
    wait_until(|| weak.iter().all(|reader| reader.strong_count() == 0));
    assert!(
        p.h.engine
            .request_committed_stem_residency_with_producer(
                &p.selected,
                "all_stems",
                0b1010,
                &p.h.producer,
            )
            .is_err()
    );
    p.render(257, false);
}

#[test]
fn full_residency_control_queue_preserves_intent_and_releases_cold_job_reservations() {
    let mut p = Productive::new(false, 0.73);
    let sample = p.h.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let playback = voice(&p.h.callback.mixer).source_playback;
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    producer.push(ControlMessage::Ping()).unwrap();
    let producer = Arc::new(Mutex::new(producer));
    // More than the fixed 32-job capacity: every failed command admission must
    // release its cold-lane reservation before the next attempt.
    for _ in 0..64 {
        let error = p.h.engine.request_committed_stem_residency_with_producer(
            &p.selected,
            "all_stems",
            0b1111,
            &producer,
        );
        assert!(error.is_err());
    }
    assert!(matches!(consumer.pop().unwrap(), ControlMessage::Ping()));
    assert!(consumer.pop().is_err());
    assert!(p.selected.is_current());
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    assert!(playback.matches_exact(&voice(&p.h.callback.mixer).source_playback));
    let current = p.h.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&sample.samples, &current.samples));
    let retry = p.request("all_stems", 0b1111);
    p.capture_then_wait(&retry);
    assert!(retry.cancel());
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(retry.residency_status(), "rejected");
    p.render(257, false);
}

#[test]
fn changed_source_or_timing_rejects_captured_component_return_and_keeps_full_mix() {
    for source_change in [false, true] {
        let mut p = Productive::new(false, 0.73);
        let sample = p.h.callback.mixer.bank_for_measurement()[0]
            .clone()
            .unwrap();
        let pending = p.request("all_stems", 0b1111);
        p.capture_then_wait(&pending);
        if source_change {
            p.h.engine.input_runtime_ownership.revoke_source(0);
        } else {
            let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
            push_preparation_epoch_message(
                &mut producer,
                ControlParameterMessage::SetPadBpm {
                    id: 0,
                    bpm: Some(127.0),
                },
                &p.h.engine.prepared_source_epochs[0],
                "SetPadBpm",
            )
            .unwrap();
            assert!(consumer.pop().is_ok());
        }
        assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
        assert_eq!(pending.residency_status(), "rejected");
        assert_eq!(pending.effective_mode_status(), "rejected");
        assert!(pending.error().is_some());
        assert!(!pending.is_current());
        assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
        assert_eq!(
            p.h.callback.mixer.stem_demand_for_measurement(0).0,
            StemMixMode::FullMix
        );
        let current = p.h.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(&sample.samples, &current.samples));
        p.render(257, false);
    }
}

#[test]
fn actual_new_window_ack_fences_old_capture_but_same_committed_selection_can_retry() {
    let mut p = Productive::new(false, 0.73);
    let initial = p.h.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let pending = p.request("all_stems", 0b1111);
    p.capture_then_wait(&pending);
    let refresh = prepare_window_with_producer(
        &p.h.engine,
        0,
        WindowRequest {
            storage_range: Some((
                (LOOP_START - 1) as f64 / f64::from(RATE),
                (LOOP_END + 1) as f64 / f64::from(RATE),
            )),
            ..WindowRequest::default()
        },
        p.h.producer.clone(),
    )
    .unwrap();
    wait_until(|| refresh.publication_status() == "pending" || refresh.error().unwrap().is_some());
    assert_eq!(
        refresh.publication_status(),
        "pending",
        "{:?}",
        refresh.error()
    );
    assert!(p.h.callback.drain(&mut p.h.consumer) >= 2);
    assert_eq!(refresh.publication_status(), "accepted");
    assert_eq!(pending.residency_status(), "rejected");
    assert_eq!(pending.effective_mode_status(), "rejected");
    reconcile(&p.h.engine).unwrap();
    assert!(p.selected.is_current());
    assert!(p.h.callback.mixer.stems_for_measurement()[0].is_none());
    let current = p.h.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    assert!(!Arc::ptr_eq(&initial.samples, &current.samples));
    assert_ne!(initial.resident_binding(), current.resident_binding());
    p.render(257, false);
    let fresh = p.request("all_stems", 0b1111);
    p.capture_then_wait(&fresh);
    assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
    assert_eq!(fresh.residency_status(), "accepted");
    p.select_reference("all_stems", 0b1111);
    p.render(257, false);
    assert_eq!(fresh.effective_mode_status(), "effective");
    assert!(p.selected.is_current());
}

#[test]
fn first_selection_registration_cannot_borrow_active_or_paused_old_voice_authority() {
    for paused in [false, true] {
        let mut h = long_harness();
        assert!(h.callback.mixer.play_sample_at_output_frame(0, 1.0, 0));
        let old_sample = voice(&h.callback.mixer).sample.clone().unwrap();
        let old_generation = voice(&h.callback.mixer).generation;
        if paused {
            h.callback.mixer.pause_sample(0);
        }
        let preparation = control::prepared_for_test(
            &h.engine,
            PreparedMigrationMaterial::from_current(
                &h.root,
                h.engine.sample_cache.lock().unwrap()[0].clone().unwrap(),
                h.engine.cold_leases.lock().unwrap()[0].clone().unwrap(),
            )
            .unwrap(),
            &h.root,
        )
        .unwrap();
        let source_adoption = control::adopt_for_format(
            &h.engine,
            0,
            &preparation,
            h.producer.clone(),
            (2, RATE, h.root.clone()),
        )
        .unwrap();
        wait_until(|| {
            if h.consumer.peek().is_ok() {
                return true;
            }
            while let Ok(event) = h.engine.loader_rx.lock().unwrap().try_recv() {
                if let LoaderEvent::Error {
                    request_id, error, ..
                } = event
                {
                    assert_ne!(
                        request_id,
                        source_adoption.request_id(),
                        "actual replacement source failed before callback: {error:?}"
                    );
                }
            }
            assert_ne!(
                source_adoption.phase().unwrap(),
                "rejected",
                "actual replacement source was rejected before callback"
            );
            false
        });
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        source_successes(&h.engine, &[source_adoption.request_id()]);
        preparation.release_preparation().unwrap();
        assert_eq!(voice(&h.callback.mixer).generation, old_generation);
        assert_eq!(voice(&h.callback.mixer).paused, paused);
        assert!(Arc::ptr_eq(
            &old_sample.samples,
            &voice(&h.callback.mixer).sample.as_ref().unwrap().samples,
        ));
        let source = h.ticket(0);
        let pair = h.prepare(0, &source, false, None);
        let _saved = h.save(&pair);
        let attempted = h
            .engine
            .register_committed_stem_pair_with_producer(&pair, &source, &h.producer)
            .unwrap();
        assert_eq!(attempted.publication_status(), "pending");
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert_eq!(attempted.publication_status(), "rejected");
        assert!(!attempted.is_current());
        assert!(attempted.error().is_some());
        assert!(h.callback.mixer.stems_for_measurement()[0].is_none());
        assert_eq!(voice(&h.callback.mixer).generation, old_generation);
        assert_eq!(voice(&h.callback.mixer).paused, paused);
        assert!(Arc::ptr_eq(
            &old_sample.samples,
            &voice(&h.callback.mixer).sample.as_ref().unwrap().samples,
        ));
    }
}

const CONTROLLER_CHILD: &str = "FLITZI_COMMITTED_RESIDENCY_CONTROLLER_CHILD";
const CONTROLLER_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::actual_python_controller_active_all_full_mix_all_uses_own_receipts_and_wet_chronology";
const STOPPED_CONTROLLER_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::actual_stopped_all_ordinary_transport_play_withdraws_receipt_then_explicit_all_confirms";

/// This facade forwards the actual control/query implementations into the same
/// producer used by the genuine StemController. It supplies no fabricated ACK.
#[pyclass]
struct ReadyPlaybackAudio {
    engine: Arc<AudioEngine>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
}

#[pymethods]
impl ReadyPlaybackAudio {
    fn output_sample_rate(&self) -> u32 {
        RATE
    }
    fn loaded_sample_shape(&self, id: usize) -> PyResult<(u32, usize, usize)> {
        self.engine.loaded_sample_shape(id)
    }
    fn loaded_residency(&self, py: Python<'_>, id: usize) -> PyResult<Py<PyDict>> {
        self.engine.loaded_residency(py, id)
    }
    fn current_input_runtime_pad_binding(
        &self,
        id: usize,
    ) -> PyResult<Option<crate::audio_engine::input_runtime_binding::InputRuntimePadBinding>> {
        self.engine.current_input_runtime_pad_binding(id)
    }
    fn pad_key_lock_status(&self, py: Python<'_>, id: usize) -> PyResult<Option<Py<PyDict>>> {
        self.engine.pad_key_lock_status(py, id)
    }
    fn current_constant_timing(&self, py: Python<'_>, id: usize) -> PyResult<Option<Py<PyAny>>> {
        self.engine.current_constant_timing(py, id)
    }
    fn pad_timing_intent(&self, id: usize) -> PyResult<&'static str> {
        self.engine.pad_timing_intent(id)
    }
    #[pyo3(signature=(id, start_s=None, end_s=None, position_s=None, key_lock=None))]
    fn prepare_resident_control(
        &self,
        id: usize,
        start_s: Option<f64>,
        end_s: Option<f64>,
        position_s: Option<f64>,
        key_lock: Option<bool>,
    ) -> PyResult<crate::audio_engine::resident_relocation::ResidentWindowTicket> {
        prepare_window_with_producer(
            &self.engine,
            id,
            WindowRequest {
                loop_region: start_s.map(|start| (start, end_s)),
                seek_position_s: position_s,
                key_lock,
                ..WindowRequest::default()
            },
            self.producer.clone(),
        )
    }
    #[pyo3(signature=(ticket, exclusive=false, received_at_ns=None))]
    fn play_resident_control(
        &self,
        ticket: &crate::audio_engine::resident_relocation::ResidentWindowTicket,
        exclusive: bool,
        received_at_ns: Option<u64>,
    ) -> PyResult<bool> {
        let now = self.engine.capture_input_timestamp_ns();
        crate::audio_engine::resident_relocation::launch_with_producer(
            &self.engine,
            ticket,
            exclusive,
            crate::audio_engine::timing::validated_input_timestamp(received_at_ns, now)
                .unwrap_or(now),
            &self.producer,
        )
    }
}

fn controller_python(locals: &Py<PyDict>, code: &str) {
    Python::attach(|py| worker_python(py, locals.bind(py), code));
}

fn controller_ticket_status(locals: &Py<PyDict>, name: &str, method: &str) -> String {
    Python::attach(|py| {
        locals
            .bind(py)
            .get_item(name)
            .unwrap()
            .unwrap()
            .call_method0(method)
            .unwrap()
            .extract()
            .unwrap()
    })
}

struct ControllerPlayback {
    callback: Box<Callback>,
    reference: Box<RtMixer>,
    full_mix: Box<RtMixer>,
    frame: u64,
    source: SampleBuffer,
    components: [SampleBuffer; 4],
    ratio: f64,
    selection: Option<u8>,
    previous: Option<u8>,
    ramp_start: u64,
    raw: Option<ControllerRawNative>,
    candidate: Option<(u64, ControllerRawNative)>,
    filter: PerPadDspChain,
}

struct ControllerRawNative {
    native: RubberBandLiveShifter,
    input: [VecDeque<f32>; 2],
    output: [VecDeque<f32>; 2],
    used: bool,
}

impl ControllerRawNative {
    fn new(ratio: f64, prepared: bool) -> Self {
        let mut native = RubberBandLiveShifter::new(RATE, 2).unwrap();
        if prepared {
            native
                .prepare_exact_pitch(pitch_scale_for_tempo_ratio(ratio))
                .unwrap();
        } else {
            native.prepare_for_reuse().unwrap();
            native
                .set_pitch_scale(pitch_scale_for_tempo_ratio(ratio))
                .unwrap();
        }
        let block = native.block_size();
        Self {
            native,
            input: std::array::from_fn(|_| VecDeque::new()),
            output: std::array::from_fn(|_| VecDeque::from(vec![0.0; block - 1])),
            used: false,
        }
    }

    fn render(&mut self, feed: &[Vec<f32>; 2]) -> Vec<f32> {
        let frames = feed[0].len();
        for (input, feed) in self.input.iter_mut().zip(feed) {
            input.extend(feed.iter().copied());
        }
        let block = self.native.block_size();
        while self.input[0].len() >= block {
            let input = self
                .input
                .iter_mut()
                .map(|input| input.drain(..block).collect())
                .collect::<Vec<Vec<f32>>>();
            let mut output = vec![vec![0.0; block]; 2];
            self.native.shift(&input, &mut output).unwrap();
            self.used = true;
            for (fifo, output) in self.output.iter_mut().zip(output) {
                fifo.extend(output);
            }
        }
        assert!(self.output.iter().all(|fifo| fifo.len() >= frames));
        (0..frames)
            .flat_map(|_| {
                [
                    self.output[0].pop_front().unwrap(),
                    self.output[1].pop_front().unwrap(),
                ]
            })
            .collect()
    }

    fn render_chosen(&mut self, feed: &[Vec<f32>; 2]) -> Vec<f32> {
        // A tracked initial KEY LOCK receipt retains the dry source through
        // the first complete Native block. Determine that boundary from this
        // oracle's own input FIFO, rather than reading the actual mixer gate.
        let mut chosen = Vec::with_capacity(feed[0].len() * 2);
        let mut offset = 0;
        while offset < feed[0].len() {
            let dry = !self.used;
            let frames = if dry {
                (self.native.block_size() - self.input[0].len()).min(feed[0].len() - offset)
            } else {
                feed[0].len() - offset
            };
            let chunk =
                std::array::from_fn(|channel| feed[channel][offset..offset + frames].to_vec());
            let wet = self.render(&chunk);
            if dry {
                chosen.extend((0..frames).flat_map(|frame| [chunk[0][frame], chunk[1][frame]]));
            } else {
                chosen.extend(wet);
            }
            offset += frames;
        }
        chosen
    }
}

fn controller_filter() -> PerPadDspChain {
    let mut filter = PerPadDspChain::new(0, RATE as f32, 2);
    for (slot, db) in [
        (DspParameterSlot::Slot0, -12.0_f32),
        (DspParameterSlot::Slot1, -3.0),
        (DspParameterSlot::Slot2, 4.0),
    ] {
        let normalized = if db <= 0.0 {
            0.5 * 10.0_f32.powf(db / 20.0)
        } else {
            0.5 + 0.5 * (db / 6.0)
        };
        assert!(filter.set_parameter(
            DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap(),
            normalized
        ));
    }
    filter.reset();
    filter
}

impl ControllerPlayback {
    fn select(&mut self, mask: Option<u8>) {
        self.previous = self.selection;
        self.selection = mask;
        self.ramp_start = self.frame;
    }

    fn feed(&self, start: u64, frames: usize) -> [Vec<f32>; 2] {
        // Binary-exact fixture rates permit an algebraic clock independent
        // of SourcePlayback, SourceReadPlan and either mixer. Physical knots
        // come from the independently retained complete original/components.
        std::array::from_fn(|channel| {
            (start..start + frames as u64)
                .map(|output_frame| {
                    let phase = (output_frame as f64 * self.ratio)
                        .rem_euclid((LOOP_END - LOOP_START) as f64);
                    let left = LOOP_START + phase.floor() as usize;
                    let right = if left + 1 == LOOP_END {
                        LOOP_START
                    } else {
                        left + 1
                    };
                    let knot = |frame: usize, mask: Option<u8>| {
                        mask.map_or(self.source.samples[frame * 2 + channel], |mask| {
                            self.components
                                .iter()
                                .enumerate()
                                .filter(|(index, _)| mask & (1 << index) != 0)
                                .fold(0.0, |sum, (_, component)| {
                                    sum + component.samples[frame * 2 + channel]
                                })
                        })
                    };
                    let sample = |frame| {
                        let gain = if self.previous == self.selection {
                            1.0
                        } else {
                            ((output_frame.saturating_sub(self.ramp_start) as f64 * self.ratio)
                                / 128.0)
                                .min(1.0) as f32
                        };
                        if gain == 1.0 {
                            knot(frame, self.selection)
                        } else {
                            knot(frame, self.previous) * (1.0 - gain)
                                + knot(frame, self.selection) * gain
                        }
                    };
                    let left_value = sample(left);
                    left_value + (sample(right) - left_value) * phase.fract() as f32
                })
                .collect()
        })
    }

    fn render(&mut self, frames: usize, timestamp: bool) -> (Vec<f32>, Vec<f32>) {
        if self
            .candidate
            .as_ref()
            .is_some_and(|(target, _)| self.frame >= *target)
        {
            let (target, raw) = self.candidate.take().unwrap();
            assert_eq!(self.frame, target);
            self.raw = Some(raw);
        }
        let feed = self.feed(self.frame, frames);
        let raw = self
            .raw
            .as_mut()
            .expect("KEYLOCK ON raw Native oracle")
            .render_chosen(&feed);
        let mut output = vec![0.0; frames * 2];
        let mut expected = vec![0.0; frames * 2];
        let mut full_mix = vec![0.0; frames * 2];
        reset_tap_observation_for_test();
        if timestamp {
            self.callback.mixer.render_rt_at_output_frame(
                &mut output,
                &mut [0.0; NUM_SAMPLES],
                self.frame,
                &mut self.callback.retirement,
            );
        } else {
            self.callback.mixer.render_rt(
                &mut output,
                &mut [0.0; NUM_SAMPLES],
                &mut self.callback.retirement,
            );
        }
        let taps = tap_observation_for_test();
        assert_eq!(taps.missing_reads, 0);
        assert!(taps.min_frame.is_some_and(|frame| frame >= LOOP_START));
        assert!(taps.max_frame.is_some_and(|frame| frame < LOOP_END));
        for (mixer, target) in [
            (self.reference.as_mut(), &mut expected),
            (self.full_mix.as_mut(), &mut full_mix),
        ] {
            if timestamp {
                mixer.render_at_output_frame(self.frame, target, &mut [0.0; NUM_SAMPLES]);
            } else {
                mixer.render(target, &mut [0.0; NUM_SAMPLES]);
            }
        }
        assert_eq!(
            output, expected,
            "controller wet chronology at {}",
            self.frame
        );
        for (actual, raw) in output.chunks_exact(2).zip(raw.chunks_exact(2)) {
            self.filter.begin_frame();
            for channel in 0..2 {
                assert_eq!(
                    actual[channel],
                    self.filter.process_sample(channel, raw[channel]),
                    "controller raw Native + uninterrupted EQ at {}",
                    self.frame
                );
            }
        }
        assert!(
            voice(&self.callback.mixer)
                .source_playback
                .matches_exact(&voice(&self.reference).source_playback)
        );
        assert!(
            voice(&self.callback.mixer)
                .source_playback
                .matches_exact(&voice(&self.full_mix).source_playback)
        );
        assert_eq!(
            voice(&self.callback.mixer).stretch.pending_fifo_frames(),
            voice(&self.reference).stretch.pending_fifo_frames(),
        );
        self.frame += frames as u64;
        (output, full_mix)
    }

    fn adopt_native(&mut self, initial: bool) -> (Vec<f32>, Vec<f32>) {
        wait_until(|| {
            voice_mut(&mut self.callback.mixer)
                .stretch
                .source_preparation_ready()
                && voice_mut(&mut self.reference)
                    .stretch
                    .source_preparation_ready()
                && (!initial
                    || voice_mut(&mut self.full_mix)
                        .stretch
                        .source_preparation_ready())
        });
        let target = voice_mut(&mut self.callback.mixer)
            .stretch
            .prepared_target_output_frame()
            .unwrap();
        let prepared = voice_mut(&mut self.callback.mixer)
            .stretch
            .prepared_native_address();
        assert_ne!(prepared, 0);
        assert_ne!(
            prepared,
            voice(&self.callback.mixer).stretch.native_state_address()
        );
        assert_eq!(
            voice_mut(&mut self.reference)
                .stretch
                .prepared_target_output_frame(),
            Some(target),
        );
        let prepared_taps = voice_mut(&mut self.callback.mixer)
            .stretch
            .prepared_tap_observation()
            .unwrap();
        assert_eq!(prepared_taps.missing_reads, 0);
        assert!(
            prepared_taps
                .min_frame
                .is_some_and(|frame| frame >= LOOP_START)
        );
        assert!(
            prepared_taps
                .max_frame
                .is_some_and(|frame| frame < LOOP_END)
        );
        let mut raw = ControllerRawNative::new(self.ratio, true);
        let warm_start =
            target - crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES as u64;
        raw.render(&self.feed(warm_start, (target - warm_start) as usize));
        self.candidate = Some((target, raw));
        let mut partitions = [1_usize, 127, 777, 96, 257, 512, 31].into_iter().cycle();
        while self.frame < target {
            let frames = partitions
                .next()
                .unwrap()
                .min((target - self.frame) as usize);
            self.render(frames, true);
        }
        let (mut output, mut full_mix) = self.render(137, true);
        assert_eq!(
            voice(&self.callback.mixer).stretch.native_state_address(),
            prepared
        );
        assert_eq!(
            voice(&self.callback.mixer).stretch.native_tap_observation(),
            Some(prepared_taps)
        );
        let mut remaining = voice(&self.callback.mixer).stretch.rubberband_block_size() * 8;
        for frames in [1_usize, 31, 137, 257, 512].into_iter().cycle() {
            let frames = frames.min(remaining);
            let (actual, counterfactual) = self.render(frames, true);
            output.extend(actual);
            full_mix.extend(counterfactual);
            assert_eq!(
                voice(&self.callback.mixer).stretch.native_state_address(),
                prepared
            );
            remaining -= frames;
            if remaining == 0 {
                break;
            }
        }
        (output, full_mix)
    }

    fn ramp(&mut self, locals: &Py<PyDict>, ticket: &str, ratio: f64) {
        let native = voice(&self.callback.mixer).stretch.native_state_address();
        let mut elapsed = 0;
        for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
            let remaining = (128.0 / ratio).ceil() as usize - elapsed;
            let frames = frames.min(remaining);
            self.render(frames, false);
            elapsed += frames;
            assert_eq!(
                voice(&self.callback.mixer).stretch.native_state_address(),
                native
            );
            if elapsed as f64 * ratio >= 128.0 {
                break;
            }
            assert_eq!(
                controller_ticket_status(locals, ticket, "effective_mode_status"),
                "transitioning"
            );
        }
        // Finishing the input ramp does not prove that the predecessor Native
        // and its queued Wet samples already supply this target's own history.
        assert_eq!(
            controller_ticket_status(locals, ticket, "effective_mode_status"),
            "transitioning"
        );
    }
}

#[test]
fn actual_python_controller_active_all_full_mix_all_uses_own_receipts_and_wet_chronology() {
    if std::env::var_os(CONTROLLER_CHILD).is_some() {
        controller_active_round_trip(false);
        return;
    }
    // Only the single-test child changes cwd to its owned project fixture. The
    // Cargo process and every other test retain their original execution context.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            CONTROLLER_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CONTROLLER_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "productive committed residency controller child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_stopped_all_ordinary_transport_play_withdraws_receipt_then_explicit_all_confirms() {
    if std::env::var_os(CONTROLLER_CHILD).is_some() {
        controller_active_round_trip(true);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            STOPPED_CONTROLLER_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CONTROLLER_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stopped committed residency controller child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

fn controller_stopped_prepare_then_play(
    engine: &AudioEngine,
    locals: &Py<PyDict>,
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
) {
    let source = callback.mixer.bank_for_measurement()[0].clone().unwrap();
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
    controller_python(
        locals,
        r#"
assert not session.active_sample_ids
assert controller.set_stem_mix_mode(0, 'all_stems')
stopped = controller._committed_residency._selections[0].receipt
assert stopped is not None and stopped.effective_mode_status() == 'waiting'
"#,
    );
    assert_eq!(callback.drain(consumer), 1);
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, "stopped", "residency_status") == "rejected"
    });
    assert_eq!(callback.drain(consumer), 1);
    callback.mixer.finish_committed_stem_mode(0);
    reconcile(engine).unwrap();
    let set = callback.mixer.stems_for_measurement()[0].clone().unwrap();
    assert_eq!(
        callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::AllStems
    );
    assert!(Arc::ptr_eq(
        &source.samples,
        &callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    controller_python(
        locals,
        r#"
controller.on_frame_render()
assert stopped.residency_status() == 'accepted' and stopped.is_current()
assert stopped.effective_mode_status() == 'waiting'
assert stopped.effective_failure_reason() is None and stopped.error() is None
assert controller.stem_residency_status(0).effective == 'waiting'
from flitzis_looper.controller.transport import TransportController
class ReadyTransportAudio:
    def __getattr__(self, name):
        return getattr(playback_audio, name)
    def play_sample(self, *args, **kwargs):
        raise AssertionError('cache-backed ordinary Play must use its own resident window ACK')
    play_sample_exclusive = play_sample
project.pad_loop_auto[0] = False
project.pad_loop_start_s[0] = 17 / fixture['rate']
project.pad_loop_end_s[0] = 1517 / fixture['rate']
project.pad_key_lock[0] = True
transport = TransportController(project, session, ReadyTransportAudio())
transport.playback.trigger_pad_keep_others(0)
ready_start = transport.residency._pending[0].ticket
assert ready_start is not None and ready_start.publication_status() == 'pending'
assert not stopped.is_current()
controller.on_frame_render()
selection_state = controller._committed_residency._selections[0]
assert selection_state.ticket is registration and registration.is_current()
assert selection_state.receipt is None and selection_state.desired is None
assert selection_state.receipt_intent is None
assert controller.stem_residency_status(0).effective is None
assert project.pad_stem_mix_mode[0] == 'all_stems'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert!(matches!(
        consumer.peek(),
        Ok(ControlMessage::RelocateResident(_))
    ));
    assert_eq!(callback.drain(consumer), 1);
    assert!(Arc::ptr_eq(
        &source.samples,
        &callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .complete_set_identity
    ));
    Python::attach(|py| {
        let ready = locals.bind(py).get_item("ready_start").unwrap().unwrap();
        let ready = ready
            .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
            .unwrap();
        assert!(
            ready.read_observation_for_test().is_none(),
            "ready Play must reuse the acknowledged window"
        );
    });
    controller_python(
        locals,
        "transport.on_frame_render()\nassert transport.residency.status(0)[1] is None",
    );
    assert!(matches!(
        consumer.peek(),
        Ok(ControlMessage::TriggerInputPad {
            resident_control: Some(_),
            ..
        })
    ));
    assert_eq!(callback.drain(consumer), 1);
    assert!(consumer.is_empty());
    assert!(
        callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    controller_python(
        locals,
        r#"
session.active_sample_ids.add(0)
for _ in range(16):
    controller.on_frame_render()
assert controller._committed_residency._selections[0].receipt is None
assert controller.stem_residency_status(0).effective is None
assert not session.stem_generation_errors, session.stem_generation_errors
assert audio.worker_call_count() == 1
"#,
    );
    assert!(
        consumer.is_empty(),
        "observing a withdrawn receipt must not launch an automatic residency retry"
    );
}

fn controller_stopped_fresh_all(
    locals: &Py<PyDict>,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
) {
    let generation = voice(&playback.callback.mixer).generation;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let set = playback.callback.mixer.stems_for_measurement()[0]
        .clone()
        .unwrap();
    controller_python(
        locals,
        r#"
assert controller._committed_residency._selections[0].receipt is None
assert controller.set_stem_mix_mode(0, 'all_stems')
live_receipt = controller._committed_residency._selections[0].receipt
assert live_receipt is not stopped and live_receipt.is_current()
assert live_receipt.residency_status() in {'captured', 'pending'}
assert live_receipt.effective_mode_status() == 'waiting'
"#,
    );
    assert_eq!(playback.callback.drain(consumer), 1);
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, "live_receipt", "residency_status") == "rejected"
    });
    // A real callback partition advances the voice after capture, without
    // granting cursor equality or replacing its source/Native history owners.
    playback.render(31, false);
    let fifos = voice(&playback.callback.mixer)
        .stretch
        .pending_fifo_frames();
    assert_eq!(playback.callback.drain(consumer), 1);
    assert_eq!(
        controller_ticket_status(locals, "live_receipt", "residency_status"),
        "accepted"
    );
    assert_eq!(
        controller_ticket_status(locals, "live_receipt", "effective_mode_status"),
        "transitioning"
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        native
    );
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .pending_fifo_frames(),
        fifos
    );
    let actual = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &actual.complete_set_identity
    ));
    for (before, after) in set.stems.iter().zip(&actual.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
    let mut output = Vec::new();
    for frames in [1, 31, 137, 257, 777] {
        output.extend(playback.render(frames, true).0);
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
    }
    assert!(output.iter().any(|sample| sample.abs() > 0.001));
    controller_python(
        locals,
        r#"
controller.on_frame_render()
assert live_receipt.is_current() and live_receipt.effective_mode_status() == 'effective'
assert live_receipt.effective_failure_reason() is None and live_receipt.error() is None
assert controller.stem_residency_status(0).effective == 'effective'
assert not stopped.is_current()
assert not session.stem_generation_errors, session.stem_generation_errors
assert audio.worker_call_count() == 1
controller.shut_down()
"#,
    );
}

fn controller_active_round_trip(stopped_start: bool) {
    let mut h = long_harness();
    // This process executes only the exact child above; no runner-wide cwd is
    // changed. Every written fixture remains inside the held temporary project.
    std::env::set_current_dir(h.root.parent().unwrap()).unwrap();
    let source = h.ticket(0);
    let oracle_pair = h.prepare(0, &source, true, None);
    assert_eq!(
        oracle_pair
            .pair_reader
            .descriptor
            .content
            .alignment
            .offset_frames,
        0
    );
    for component in oracle_pair.component_views_for_test().unwrap() {
        assert!(
            component.samples[LOOP_START * 2..LOOP_END * 2]
                .iter()
                .any(|sample| sample.abs() > 0.04),
            "actual controller loop component must contain signal"
        );
    }
    let saved_selection = selection(&oracle_pair);
    let _saved = h.save(&oracle_pair);
    let full = h.material.sample.clone();
    let full_stems = PreparedStemSet {
        complete_set_identity: Arc::new([73; 32]),
        reference_samples: full.samples.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        accepted_timing: None,
        source_version_hash: crate::audio_engine::stem_cache::source_version_hash(&h.version),
        sample_rate_hz: RATE,
        channels: 2,
        frame_count: full.frame_count(),
        available_mask: 0b1111,
        stems: oracle_pair.component_views_for_test().unwrap().clone(),
    };
    drop(oracle_pair);
    let window = prepare_window_with_producer(
        &h.engine,
        0,
        WindowRequest {
            key_lock: Some(true),
            loop_region: Some((
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
            )),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    wait_until(|| h.consumer.peek().is_ok() || window.error().unwrap().is_some());
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(window.publication_status(), "accepted");
    reconcile(&h.engine).unwrap();
    let source = h.callback.mixer.bank_for_measurement()[0].clone().unwrap();
    assert!(h.callback.mixer.stems_for_measurement()[0].is_none());
    let metadata = h.material.metadata();
    let fixture = json!({
        "original_reference":metadata["new_reference"],
        "material_id":metadata["material_id"],
        "source_version":h.version,
        "pair":saved_selection,
        "frames":full.frame_count(),
        "rate":RATE,
    });
    let engine = Arc::new(h.engine);
    let locals = Python::attach(|py| {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        py.import("sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .call_method1(
                "insert",
                (0, repository.join("src").to_string_lossy().as_ref()),
            )
            .unwrap();
        let locals = PyDict::new(py);
        locals
            .set_item("input_json", serde_json::to_string(&fixture).unwrap())
            .unwrap();
        locals
            .set_item(
                "audio",
                Py::new(
                    py,
                    PairWorkerAudio {
                        engine: engine.clone(),
                        root: h.root.clone(),
                        producer: h.producer.clone(),
                        control_thread: std::thread::current().id(),
                        worker_calls: AtomicUsize::new(0),
                        only_pool_threads: AtomicBool::new(true),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        locals
            .set_item(
                "playback_audio",
                Py::new(
                    py,
                    ReadyPlaybackAudio {
                        engine: engine.clone(),
                        producer: h.producer.clone(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        worker_python(
            py,
            &locals,
            r#"
import json
from flitzis_looper.models import ProjectState, SessionState, StemCacheEntry, PadContentIdentity
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.stem_workers import StemWorkerPool
from flitzis_looper.stem_pair_selection import StemPairSelection
fixture = json.loads(input_json)
project = ProjectState()
project.sample_paths[0] = fixture['original_reference']
project.sample_durations[0] = fixture['frames'] / fixture['rate']
project.pad_content[0] = PadContentIdentity(instance_id='7' * 32, material_id=fixture['material_id'])
project.pad_timing_intent[0] = 'legacy'
project.pad_stem_mix_mode[0] = 'full_mix'
disk_selection = StemPairSelection.model_validate(fixture['pair'])
project.stem_cache[0] = StemCacheEntry(source_version=fixture['source_version'],
    cache_dir=disk_selection.wav_generation, stems=expected_stem_files(disk_selection.wav_generation),
    pair=disk_selection, available=True)
session = SessionState()
session.pad_stem_enabled_mask[0] = 0b1010
controller = StemController(project, session, audio)
assert isinstance(controller._stem_worker_pool, StemWorkerPool)
assert controller._stem_worker_pool._executor._max_workers == 2
controller.restore_stem_cache_from_project_state()
assert not controller.stems_available(0)
assert controller.publish_restored_stem_cache_if_available(0)
"#,
        );
        locals.unbind()
    });
    wait_until(|| {
        Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
            );
            let status = locals
                .bind(py)
                .get_item("controller")
                .unwrap()
                .unwrap()
                .call_method1("stem_residency_status", (0,))
                .unwrap();
            !status.is_none()
                && status
                    .getattr("registration")
                    .unwrap()
                    .extract::<String>()
                    .unwrap()
                    == "pending"
        })
    });
    controller_python(
        &locals,
        r#"
registration = controller._committed_residency._selections[0].ticket
assert registration.publication_status() == 'pending'
assert controller.stems_available(0)
assert project.stem_cache[0].pair == disk_selection
assert controller.stem_residency_status(0).residency is None
assert controller.stem_residency_status(0).effective is None
assert project.pad_stem_mix_mode[0] == 'full_mix'
assert audio.worker_call_count() == 1 and audio.prepared_only_on_pool_threads()
"#,
    );
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    controller_python(
        &locals,
        "controller.on_frame_render()\nassert registration.publication_status() == 'accepted'\nassert registration.is_current()\nassert controller.stem_residency_status(0).effective is None",
    );

    let ratio = 0.75;
    let make_reference = |all: bool| {
        let mut mixer = Box::new(RtMixer::new(2, RATE as f32));
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        ownership.publish_source(0, &full, RATE, 1);
        mixer.set_input_runtime_ownership(ownership.clone());
        mixer.load_sample(0, full.clone());
        assert!(mixer.publish_prepared_stems(0, full_stems.clone()));
        mixer.set_loop_region_frames(0, LOOP_START, Some(LOOP_END));
        mixer.set_speed(ratio);
        let key_lock = ownership.next_key_lock_request(0).unwrap();
        mixer.set_pad_key_lock_request(
            0,
            true,
            key_lock,
            InputPadBinding {
                resident: full.resident_binding(),
                source_address: full.source_address(),
                sample_count: full.source_sample_count(),
                channels: full.channels,
                sample_rate_hz: RATE,
                authority_revision: ownership.authority[0].load(Ordering::Acquire),
                runtime_revision: 0,
                accepted: None,
            },
            1,
        );
        mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
        if all {
            let hash = full_stems.source_version_hash;
            assert!(mixer.set_stem_enabled_mask(0, 0b1010, hash));
            assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, hash));
        }
        assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
        mixer
    };
    h.callback.mixer.set_speed(ratio);
    h.callback.mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    if stopped_start {
        controller_stopped_prepare_then_play(&engine, &locals, &mut h.callback, &mut h.consumer);
    } else {
        assert!(h.callback.mixer.play_sample_at_output_frame(0, 1.0, 0));
    }
    let mut playback = ControllerPlayback {
        callback: h.callback,
        reference: make_reference(stopped_start),
        full_mix: make_reference(false),
        frame: 0,
        source: full.clone(),
        components: full_stems.stems.clone(),
        ratio,
        selection: stopped_start.then_some(0b1010),
        previous: stopped_start.then_some(0b1010),
        ramp_start: 0,
        raw: Some(ControllerRawNative::new(ratio, false)),
        candidate: None,
        filter: controller_filter(),
    };
    playback.render(13, true);
    let (initial_output, _) = playback.adopt_native(true);
    if stopped_start {
        assert!(initial_output.iter().any(|sample| sample.abs() > 0.001));
        controller_stopped_fresh_all(&locals, &mut playback, &mut h.consumer);
        return;
    }
    let generation = voice(&playback.callback.mixer).generation;
    controller_python(
        &locals,
        r#"
session.active_sample_ids.add(0)
assert controller.set_stem_mix_mode(0, 'all_stems')
first = controller._committed_residency._selections[0].receipt
assert first is not None and first.effective_mode_status() == 'waiting'
assert project.pad_stem_mix_mode[0] == 'all_stems'
assert controller.stem_residency_status(0).effective == 'waiting'
for _ in range(32):
    controller.on_frame_render()
assert first.effective_mode_status() == 'waiting'
"#,
    );
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(playback.callback.drain(&mut h.consumer), 1);
    wait_until(|| {
        h.consumer.peek().is_ok()
            || controller_ticket_status(&locals, "first", "residency_status") == "rejected"
    });
    assert_eq!(
        controller_ticket_status(&locals, "first", "residency_status"),
        "pending"
    );
    playback.render(31, false);
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    assert_eq!(
        controller_ticket_status(&locals, "first", "effective_mode_status"),
        "waiting"
    );
    assert_eq!(playback.callback.drain(&mut h.consumer), 1);
    assert_eq!(
        controller_ticket_status(&locals, "first", "residency_status"),
        "accepted"
    );
    assert_eq!(
        controller_ticket_status(&locals, "first", "effective_mode_status"),
        "transitioning"
    );
    let current = playback.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&source.samples, &current.samples));
    assert_eq!(source.resident_binding(), current.resident_binding());
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    let hash = full_stems.source_version_hash;
    assert!(playback.reference.set_stem_enabled_mask(0, 0b1010, hash));
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::AllStems, hash)
    );
    playback.select(Some(0b1010));
    playback.ramp(&locals, "first", ratio);
    playback.render(13, true);
    let (actual, full_mix_output) = playback.adopt_native(false);
    assert!(actual.iter().any(|value| value.abs() > 0.001));
    assert!(
        actual
            .iter()
            .zip(full_mix_output)
            .any(|(actual, full)| (actual - full).abs() > 0.005),
        "a ready predecessor FullMix FIFO cannot stand in for actual audible ALL"
    );
    controller_python(
        &locals,
        "controller.on_frame_render()\nassert controller.stem_residency_status(0).effective == 'effective'\nassert not session.stem_generation_errors, session.stem_generation_errors",
    );
    let old = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .stems
        .each_ref()
        .map(|stem| Arc::downgrade(&stem.samples));
    controller_python(
        &locals,
        r#"
assert controller.set_stem_mix_mode(0, 'full_mix')
release = controller._committed_residency._selections[0].receipt
assert release is not first and release.effective_mode_status() == 'waiting'
assert project.pad_stem_mix_mode[0] == 'full_mix'
assert project.stem_cache[0].pair == disk_selection
"#,
    );
    assert_eq!(playback.callback.drain(&mut h.consumer), 1);
    wait_until(|| {
        h.consumer.peek().is_ok()
            || controller_ticket_status(&locals, "release", "residency_status") == "rejected"
    });
    assert_eq!(playback.callback.drain(&mut h.consumer), 1);
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::FullMix, 0)
    );
    playback.select(None);
    playback.ramp(&locals, "release", ratio);
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert!(
        old.iter().any(|reader| reader.strong_count() > 0),
        "the actual adopted ALL Native/history owner must outlive its bank retirement"
    );
    playback.render(13, true);
    playback.adopt_native(false);
    playback.callback.retirement.retired.clear();
    reconcile(&engine).unwrap();
    wait_until(|| old.iter().all(|reader| reader.strong_count() == 0));
    controller_python(
        &locals,
        r#"
controller.on_frame_render()
assert controller.stem_residency_status(0).effective == 'effective'
assert controller.stems_available(0) and project.stem_cache[0].pair == disk_selection
assert registration.is_current()
assert controller.set_stem_mix_mode(0, 'all_stems')
next_receipt = controller._committed_residency._selections[0].receipt
assert next_receipt is not first and next_receipt is not release
assert next_receipt.effective_mode_status() == 'waiting'
assert project.pad_stem_mix_mode[0] == 'all_stems'
assert audio.worker_call_count() == 1 and audio.prepared_only_on_pool_threads()
"#,
    );
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(playback.callback.drain(&mut h.consumer), 1);
    wait_until(|| {
        h.consumer.peek().is_ok()
            || controller_ticket_status(&locals, "next_receipt", "residency_status") == "rejected"
    });
    playback.render(31, false);
    assert_eq!(
        controller_ticket_status(&locals, "next_receipt", "effective_mode_status"),
        "waiting"
    );
    assert_eq!(playback.callback.drain(&mut h.consumer), 1);
    assert_eq!(
        controller_ticket_status(&locals, "next_receipt", "residency_status"),
        "accepted"
    );
    let resident = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    for (previous, current) in old.iter().zip(&resident.stems) {
        assert!(!previous.ptr_eq(&Arc::downgrade(&current.samples)));
    }
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::AllStems, hash)
    );
    playback.select(Some(0b1010));
    playback.ramp(&locals, "next_receipt", ratio);
    playback.render(13, true);
    let (actual, full_mix_output) = playback.adopt_native(false);
    assert!(actual.iter().any(|value| value.abs() > 0.001));
    assert!(
        actual
            .iter()
            .zip(full_mix_output)
            .any(|(actual, full)| (actual - full).abs() > 0.005)
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    let current = playback.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&source.samples, &current.samples));
    assert_eq!(source.resident_binding(), current.resident_binding());
    controller_python(
        &locals,
        r#"
controller.on_frame_render()
assert controller.stem_residency_status(0).registration == 'accepted'
assert controller.stem_residency_status(0).residency == 'accepted'
assert controller.stem_residency_status(0).effective == 'effective'
assert not session.stem_generation_errors, session.stem_generation_errors
assert project.stem_cache[0].pair == disk_selection
controller._committed_residency.cancel(0)
assert not registration.is_current()
assert controller.set_stem_mix_mode(0, 'full_mix')
assert project.pad_stem_mix_mode[0] == 'full_mix'
assert not controller._committed_residency.contains(0)
assert audio.worker_call_count() == 1
"#,
    );
    let old = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .stems
        .each_ref()
        .map(|stem| Arc::downgrade(&stem.samples));
    assert_eq!(playback.callback.drain(&mut h.consumer), 1);
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::FullMix, 0)
    );
    playback.select(None);
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let mut elapsed = 0;
    let total = (128.0 / ratio).ceil() as usize;
    for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
        let frames = frames.min(total - elapsed);
        playback.render(frames, false);
        elapsed += frames;
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        if elapsed == total {
            break;
        }
    }
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    playback.render(13, true);
    playback.adopt_native(false);
    playback.callback.retirement.retired.clear();
    reconcile(&engine).unwrap();
    wait_until(|| old.iter().all(|reader| reader.strong_count() == 0));
    controller_python(
        &locals,
        r#"
controller.on_frame_render()
assert controller.set_stem_mix_mode(0, 'all_stems')
assert project.pad_stem_mix_mode[0] == 'all_stems'
"#,
    );
    wait_until(|| {
        Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
            );
            let status = locals
                .bind(py)
                .get_item("controller")
                .unwrap()
                .unwrap()
                .call_method1("stem_residency_status", (0,))
                .unwrap();
            !status.is_none()
                && status
                    .getattr("registration")
                    .unwrap()
                    .extract::<String>()
                    .unwrap()
                    == "accepted"
        })
    });
    controller_python(
        &locals,
        r#"
renewed_state = controller._committed_residency._live_selections[0]
renewed_registration = renewed_state.disk.selection.ticket
assert renewed_registration is not registration
assert renewed_registration.publication_status() == 'accepted'
assert audio.worker_call_count() == 2 and audio.prepared_only_on_pool_threads()
assert project.stem_cache[0].pair == disk_selection
"#,
    );
    controller_python(
        &locals,
        r#"
controller.on_frame_render()
assert renewed_registration.publication_status() == 'accepted'
assert renewed_registration.is_current() and not registration.is_current()
resealed = renewed_state.attempt.receipt
assert resealed is not first and resealed is not next_receipt
assert resealed.effective_mode_status() == 'waiting'
"#,
    );
    assert_eq!(playback.callback.drain(&mut h.consumer), 1);
    wait_until(|| {
        h.consumer.peek().is_ok()
            || controller_ticket_status(&locals, "resealed", "residency_status") == "rejected"
    });
    playback.render(31, false);
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    assert_eq!(
        controller_ticket_status(&locals, "resealed", "effective_mode_status"),
        "waiting"
    );
    assert_eq!(playback.callback.drain(&mut h.consumer), 1);
    assert_eq!(
        controller_ticket_status(&locals, "resealed", "residency_status"),
        "accepted"
    );
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::AllStems, hash)
    );
    playback.select(Some(0b1010));
    playback.ramp(&locals, "resealed", ratio);
    playback.render(13, true);
    let (actual, full_mix_output) = playback.adopt_native(false);
    assert!(actual.iter().any(|value| value.abs() > 0.001));
    assert!(
        actual
            .iter()
            .zip(full_mix_output)
            .any(|(actual, full)| (actual - full).abs() > 0.005)
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    let current = playback.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&source.samples, &current.samples));
    assert_eq!(source.resident_binding(), current.resident_binding());
    controller_python(
        &locals,
        r#"
controller.on_frame_render()
assert controller.stem_residency_status(0).registration == 'accepted'
assert controller.stem_residency_status(0).residency == 'accepted'
assert controller.stem_residency_status(0).effective == 'effective'
assert project.stem_cache[0].pair == disk_selection
assert not session.stem_generation_errors, session.stem_generation_errors
controller.shut_down()
controller._assets.release_saved_assignments()
"#,
    );
    playback
        .callback
        .mixer
        .stop_sample_rt(0, &mut playback.callback.retirement);
    playback.callback.retirement.retired.clear();
    drop(locals);
    // Leave the temporary cwd before TempDir cleanup in this owned child.
    std::env::set_current_dir(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
}
