//! Productive offline custody/output while the existing audio selection keeps rendering.
//! The declared worker is an owned PCM16 arithmetic separator, not neural inference,
//! a hardware/device measurement, or permission to adopt a new set during playback.

use super::*;
use crate::audio_engine::OfflineStemSourceLease;
use crate::audio_engine::SealedProcessingFiles;
use crate::audio_engine::audio_stream::{drain_control_messages, render_scheduled_audio};
use crate::audio_engine::productive_source_history::ProductiveSourceBinding;
use crate::audio_engine::voice_slot::VoiceSlot;
use std::sync::mpsc::{Receiver, channel};
use std::thread::{self, JoinHandle};

const PARTITIONS: [usize; 7] = [1, 127, 384, 96, 257, 512, 31];
const WORKER_LINEAGE: &str = "owned-rust-pcm16-quarters-v1";
const WORKER_GENERATION: &str = "123456789abcdef0123456789abcdef0";
const MASTER_BPM: f64 = 127.312_345_678_901;
const DOWNBEAT: u64 = 111;

struct LivePad {
    id: usize,
    source: SampleBuffer,
    generation: u64,
    native: usize,
    history: Option<ProductiveSourceBinding>,
    demand: (StemMixMode, u8, u64),
    stems: Option<PreparedStemSet>,
    key_lock: bool,
    ratio: f64,
}

fn pad_voice(mixer: &RtMixer, id: usize) -> &VoiceSlot {
    mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(id))
        .expect("the actual productive pad voice disappeared")
}

impl LivePad {
    fn capture(mixer: &RtMixer, id: usize) -> Self {
        let voice = pad_voice(mixer, id);
        Self {
            id,
            source: mixer.bank_for_measurement()[id].clone().unwrap(),
            generation: voice.generation,
            native: voice.stretch.native_state_address(),
            history: voice
                .stretch
                .productive_history()
                .map(|history| history.binding),
            demand: mixer.stem_demand_for_measurement(id),
            stems: mixer.stems_for_measurement()[id].clone(),
            key_lock: mixer.key_lock_for_measurement(id),
            ratio: voice.source_playback.tempo_ratio(),
        }
    }

    fn assert_current(&self, actual: &RtMixer, reference: &RtMixer) {
        let bank = actual.bank_for_measurement()[self.id].as_ref().unwrap();
        assert!(Arc::ptr_eq(&bank.samples, &self.source.samples));
        assert!(bank.same_window(&self.source));
        let voice = pad_voice(actual, self.id);
        let expected = pad_voice(reference, self.id);
        assert_eq!(voice.generation, self.generation);
        assert_eq!(voice.stretch.native_state_address(), self.native);
        assert!(Arc::ptr_eq(
            &voice.sample.as_ref().unwrap().samples,
            &self.source.samples
        ));
        assert!(voice.sample.as_ref().unwrap().same_window(&self.source));
        assert_eq!(
            voice.source_playback.tempo_ratio().to_bits(),
            self.ratio.to_bits()
        );
        assert!(
            voice
                .source_playback
                .matches_exact(&expected.source_playback)
        );
        assert_eq!(
            voice.stretch.pending_fifo_frames(),
            expected.stretch.pending_fifo_frames()
        );
        assert_eq!(
            voice.stretch.varispeed_buffers(),
            expected.stretch.varispeed_buffers()
        );
        assert_eq!(actual.stem_demand_for_measurement(self.id), self.demand);
        assert_eq!(actual.key_lock_for_measurement(self.id), self.key_lock);
        match (&self.stems, &actual.stems_for_measurement()[self.id]) {
            (None, None) => {}
            (Some(before), Some(current)) => {
                assert!(Arc::ptr_eq(
                    &before.complete_set_identity,
                    &current.complete_set_identity
                ));
                for (before, current) in before.stems.iter().zip(&current.stems) {
                    assert!(Arc::ptr_eq(&before.samples, &current.samples));
                    assert!(before.same_window(current));
                }
            }
            _ => panic!("offline output changed the effective component owners"),
        }
        assert_eq!(
            voice
                .stretch
                .productive_history()
                .map(|history| history.binding),
            self.history
        );
        if let Some(history) = voice.stretch.productive_history() {
            let expected_history = expected.stretch.productive_history().unwrap();
            assert_eq!(history.binding.sample_rate_hz, RATE);
            assert_eq!(history.binding.channels, 2);
            assert_eq!(history.next_position, voice.source_playback.position());
            assert_eq!(history.next_position, expected_history.next_position);
            assert_eq!(
                history.fed_output_frames,
                expected_history.fed_output_frames
            );
            assert_eq!(history.domain, expected_history.domain);
        }
    }
}

struct ProductiveOffline {
    p: Productive,
    reference_transport: TransportTimeline,
    reference_scheduler: FixedCapacityScheduler<8>,
    reference_feedback: Feedback,
    pads: [LivePad; 2],
    previous_selection: Value,
}

impl ProductiveOffline {
    fn new(key_lock: bool, ratio: f64, selected: bool) -> Self {
        let mut p = Productive::new(key_lock, ratio);
        if selected {
            let receipt = p.request("all_stems", 0b1010);
            p.capture_then_wait(&receipt);
            assert_eq!(p.h.callback.drain(&mut p.h.consumer), 1);
            assert_eq!(receipt.residency_status(), "accepted");
            p.select_reference("all_stems", 0b1010);
            let ramp = (128.0 / ratio).ceil() as usize;
            p.render(ramp + 777, false);
            if key_lock {
                p.render(13, true);
                p.adopt_native();
            }
            assert_eq!(receipt.effective_mode_status(), "effective");
        }

        let other = p.h.material.sample.clone();
        p.reference_ownership
            .publish_test_source(215, &other, RATE, 1);
        p.reference.load_sample(215, other);
        for mixer in [&mut p.h.callback.mixer, p.reference.as_mut()] {
            // Pad215 is independently ACKed by the real cold harness. Its dry
            // source/gain/phase are distinct from the generated pad's wet path.
            mixer.set_pad_key_lock(215, false);
            mixer.set_loop_region_frames(215, LOOP_START, Some(LOOP_END));
            mixer.set_pad_eq(215, -2.0, 1.0, -3.0);
            assert!(mixer.play_sample_at_output_frame(215, 0.17, p.frame));
            assert!(mixer.seek_sample_at_output_frame(
                215,
                (LOOP_START + 73) as f64 / f64::from(RATE),
                p.frame,
            ));
        }
        let mut reference_transport = TransportTimeline::new(RATE);
        for transport in [&mut p.h.callback.transport, &mut reference_transport] {
            assert!(transport.set_master_bpm(MASTER_BPM));
            transport.set_downbeat_frame(DOWNBEAT);
            transport.advance_by_rendered_frames(usize::try_from(p.frame).unwrap());
            assert_eq!(transport.output_frame(), p.frame);
        }
        let pads = [0, 215].map(|id| LivePad::capture(&p.h.callback.mixer, id));
        let previous_selection = selection(&p.pair);
        Self {
            p,
            reference_transport,
            reference_scheduler: FixedCapacityScheduler::new(),
            reference_feedback: Feedback {
                slots: usize::MAX,
                events: Vec::new(),
            },
            pads,
            previous_selection,
        }
    }

    fn assert_previous_selection(&self) {
        assert!(self.p.selected.is_current());
        assert_eq!(self.p.selected.publication_status(), "accepted");
        assert_eq!(selection(&self.p.pair), self.previous_selection);
        assert_complete_selection(&self.p.h.root, &self.previous_selection);
        for pad in &self.pads {
            pad.assert_current(&self.p.h.callback.mixer, &self.p.reference);
        }
    }

    fn render(&mut self, frames: usize) -> [f32; 2] {
        let mut output = vec![0.0; frames * 2];
        let mut expected = output.clone();
        let mut peaks = [0.0; NUM_SAMPLES];
        let mut expected_peaks = [0.0; NUM_SAMPLES];
        let callback = self.p.h.callback.as_mut();
        assert_eq!(callback.transport.output_frame(), self.p.frame);
        reset_tap_observation_for_test();
        render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut output,
            &mut peaks,
            self.p.frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        let taps = tap_observation_for_test();
        assert_eq!(taps.missing_reads, 0);
        assert!(taps.min_frame.is_some_and(|frame| frame >= LOOP_START));
        assert!(taps.max_frame.is_some_and(|frame| frame < LOOP_END));
        render_scheduled_audio(
            &mut self.p.reference,
            &mut self.reference_scheduler,
            &mut expected,
            &mut expected_peaks,
            self.p.frame,
            2,
            &mut self.reference_transport,
            &mut self.reference_feedback,
            &mut ImmediateAudioBufferRetirement,
        );
        assert_eq!(
            output, expected,
            "actual offline continuation at {}",
            self.p.frame
        );
        assert_eq!(peaks, expected_peaks);
        assert!(output.iter().all(|sample| sample.is_finite()));
        callback.transport.advance_by_rendered_frames(frames);
        self.reference_transport.advance_by_rendered_frames(frames);
        self.p.frame += frames as u64;
        assert_eq!(callback.transport.output_frame(), self.p.frame);
        assert_eq!(callback.transport.sample_rate_hz(), RATE);
        assert_eq!(callback.transport.downbeat_frame(), DOWNBEAT);
        assert_eq!(
            callback.transport.master_period_seconds(),
            Some(60.0 / MASTER_BPM)
        );
        let beat =
            (self.p.frame as f64 - DOWNBEAT as f64) / (f64::from(RATE) * (60.0 / MASTER_BPM));
        assert_eq!(callback.transport.beat_position(), Some(beat));
        assert_eq!(
            callback.transport.beat_position(),
            self.reference_transport.beat_position()
        );
        self.assert_previous_selection();
        [peaks[0], peaks[215]]
    }

    fn render_many_loops(&mut self, total: usize) {
        let mut elapsed = 0;
        let mut activity = [0.0_f32; 2];
        for requested in PARTITIONS.into_iter().cycle() {
            let frames = requested.min(total - elapsed);
            let peaks = self.render(frames);
            for (activity, peak) in activity.iter_mut().zip(peaks) {
                *activity = (*activity).max(peak);
            }
            elapsed += frames;
            if elapsed == total {
                break;
            }
        }
        assert!(
            activity.iter().all(|peak| *peak > 0.001),
            "both real pads must stay productive: {activity:?}"
        );
    }

    fn drain_current(&mut self) -> usize {
        let callback = self.p.h.callback.as_mut();
        drain_control_messages(
            &mut self.p.h.consumer,
            &mut callback.scheduler,
            self.p.frame,
            &mut callback.quantization,
            &mut callback.transport,
            &mut callback.mixer,
            &mut callback.feedback,
            &mut callback.retirement,
        )
    }
}

struct GeneratedWavs {
    source: OfflineStemSourceLease,
    reference: String,
    fingerprint: String,
    producer: String,
}

fn source_quarters_wavs(original: &[u8], frames: usize) -> [Vec<u8>; 5] {
    // This is the fixture's declared non-neural execution: exact integer
    // decomposition of the actually read managed original, including residual.
    assert_eq!(original.len(), 44 + frames * 4);
    assert_eq!(&original[..4], b"RIFF");
    assert_eq!(&original[8..16], b"WAVEfmt ");
    assert_eq!(u16::from_le_bytes(original[20..22].try_into().unwrap()), 1);
    assert_eq!(u16::from_le_bytes(original[22..24].try_into().unwrap()), 2);
    assert_eq!(
        u32::from_le_bytes(original[24..28].try_into().unwrap()),
        RATE
    );
    assert_eq!(u16::from_le_bytes(original[34..36].try_into().unwrap()), 16);
    assert_eq!(&original[36..40], b"data");
    let mut stems = std::array::from_fn(|_| original[..44].to_vec());
    for encoded in original[44..].chunks_exact(2) {
        let value = i16::from_le_bytes(encoded.try_into().unwrap());
        let quarter = value / 4;
        let residual = value - 3 * quarter;
        let instrumental = value - quarter;
        for (stem, sample) in
            stems
                .iter_mut()
                .zip([quarter, quarter, quarter, residual, instrumental])
        {
            stem.extend_from_slice(&sample.to_le_bytes());
        }
    }
    stems
}

fn blocked_source_worker(
    source: OfflineStemSourceLease,
    path: PathBuf,
    reference: String,
    config: PathBuf,
) -> (
    Receiver<()>,
    std::sync::mpsc::Sender<()>,
    JoinHandle<GeneratedWavs>,
) {
    let (started_tx, started_rx) = channel();
    let (release_tx, release_rx) = channel();
    let worker = thread::spawn(move || {
        let witnesses: Py<SealedProcessingFiles> = Python::attach(|py| {
            py.get_type::<SealedProcessingFiles>()
                .call1((vec![config.to_string_lossy().into_owned()],))
                .unwrap()
                .extract()
                .unwrap()
        });
        let producer = Python::attach(|py| witnesses.borrow(py).witnesses()[0].1.clone());
        Python::attach(|py| source.verify(py).unwrap());
        started_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        // The blocked worker still has an actual original read to perform.
        let original = fs::read(source.source_path()).unwrap();
        assert_eq!(original.len() as u64, source.original_bytes());
        let source_digest = format!("{:x}", Sha256::digest(&original));
        assert!(
            source
                .source_version()
                .ends_with(&format!("|sha256-v1:{source_digest}"))
        );
        let shape = source.target_shape();
        assert_eq!((shape.0, shape.1), (RATE, 2));
        let stems = source_quarters_wavs(&original, shape.2);
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&json!({
                    "schema":"effective-stem-v3", "execution":WORKER_LINEAGE,
                    "producer_identity":producer, "original_sha256":source_digest,
                    "target_shape":[shape.0,shape.1,shape.2],
                }))
                .unwrap()
            )
        );
        fs::create_dir(&path).unwrap();
        let mut hashes = serde_json::Map::new();
        for (name, encoded) in STEM_FILE_NAMES.iter().zip(stems) {
            fs::write(path.join(format!("{name}.wav")), &encoded).unwrap();
            hashes.insert(
                (*name).to_owned(),
                json!(format!("{:x}", Sha256::digest(encoded))),
            );
        }
        fs::write(
            path.join(".complete.json"),
            serde_json::to_vec(&json!({
                "schema":"stem-set-sha256-v1", "source_version":source.source_version(),
                "stems":hashes, "fingerprint_schema":"effective-stem-v3",
                "generation_fingerprint":fingerprint, "producer_identity":producer,
            }))
            .unwrap(),
        )
        .unwrap();
        Python::attach(|py| {
            source.verify(py).unwrap();
            witnesses.borrow_mut(py).verify(py).unwrap();
        });
        drop(witnesses);
        GeneratedWavs {
            source,
            reference,
            fingerprint,
            producer,
        }
    });
    (started_rx, release_tx, worker)
}

fn prepare_new_disk_output(
    rig: &mut ProductiveOffline,
    generated: &GeneratedWavs,
) -> PreparedStemPair {
    let source = rig
        .p
        .h
        .engine
        .capture_prepared_stem_source(0, &generated.source)
        .unwrap();
    let work = rig
        .p
        .h
        .engine
        .capture_stem_pair_work(&rig.p.h.root, 0, &rig.p.h.version, &source)
        .unwrap();
    let reference = generated.reference.clone();
    let (started_tx, started_rx) = channel();
    let (release_tx, release_rx) = channel();
    let worker = thread::spawn(move || {
        started_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        work.prepare(&reference, false, None, None).unwrap()
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    rig.render_many_loops(8193);
    release_tx.send(()).unwrap();
    let pair = worker.join().unwrap();
    assert!(!pair.has_components());
    assert_eq!(source.publication_status(), "captured");
    assert!(rig.p.h.consumer.peek().is_err());
    let selected = selection(&pair);
    assert_ne!(
        selected["stem_set_identity"],
        rig.previous_selection["stem_set_identity"]
    );
    assert_eq!(
        selected["provenance"],
        json!({
            "schema":"effective-stem-v3", "generation_fingerprint":generated.fingerprint,
            "producer_identity":generated.producer,
        })
    );
    assert_complete_selection(&rig.p.h.root, &selected);
    assert_eq!(
        pair.pair_reader.descriptor.content.alignment.offset_frames,
        0
    );
    assert_eq!(pair.pair_reader.descriptor.content.source.frame_count, 4096);
    for artifact in &pair.pair_reader.descriptor.content.artifacts {
        assert_eq!(artifact.wav_bytes, 44 + 4096 * 4);
        assert_eq!(artifact.pcm_bytes, 4096 * 2 * 4);
    }
    rig.assert_previous_selection();
    pair
}

fn reject_active_new_output(
    rig: &mut ProductiveOffline,
    generated: &GeneratedWavs,
    pair: &PreparedStemPair,
) {
    let source = rig
        .p
        .h
        .engine
        .capture_prepared_stem_source(0, &generated.source)
        .unwrap();
    let work = rig
        .p
        .h
        .engine
        .capture_stem_pair_work(&rig.p.h.root, 0, &rig.p.h.version, &source)
        .unwrap();
    let reference = generated.reference.clone();
    let descriptor = pair.descriptor_reference.clone();
    let components = thread::spawn(move || {
        work.prepare(&reference, true, Some(&descriptor), None)
            .unwrap()
    })
    .join()
    .unwrap();
    assert!(components.has_components());
    assert_eq!(selection(&components), selection(pair));
    rig.p
        .h
        .engine
        .publish_stem_pair_with_producer(&components, &source, &rig.p.h.producer)
        .unwrap();
    assert_eq!(source.publication_status(), "pending");
    assert_eq!(rig.drain_current(), 1);
    assert_eq!(source.publication_status(), "rejected");
    assert_eq!(source.rejection_reason(), Some("pad-playing"));
    reconcile(&rig.p.h.engine).unwrap();
    rig.assert_previous_selection();
    rig.render_many_loops(12289);
}

#[test]
fn productive_offline_generation_keeps_real_audio_transport_and_rejects_active_new_set() {
    for selected in [false, true] {
        for key_lock in [false, true] {
            for ratio in [0.73, 1.371_234_567_890_123] {
                let mut rig = ProductiveOffline::new(key_lock, ratio, selected);
                rig.render_many_loops(4097);
                let source = rig
                    .p
                    .h
                    .engine
                    .capture_offline_stem_source(0, rig.p.h.version.clone())
                    .unwrap();
                assert_eq!(source.target_shape(), (RATE, 2, 4096));
                let original = PathBuf::from(source.source_path());
                let reference = rig.p.h.wav_reference.replace(GENERATION, WORKER_GENERATION);
                let path = rig.p.h.root.parent().unwrap().join(&reference);
                let config = rig
                    .p
                    .h
                    .root
                    .parent()
                    .unwrap()
                    .join("owned-non-neural-separator.json");
                fs::write(
                    &config,
                    serde_json::to_vec(&json!({
                        "schema":"owned-non-neural-fixture-v1", "execution":WORKER_LINEAGE,
                        "division":4, "residual":"drums", "instrumental":"original-minus-vocals",
                    }))
                    .unwrap(),
                )
                .unwrap();
                let (started, release, worker) =
                    blocked_source_worker(source, path.clone(), reference, config.clone());
                started.recv_timeout(Duration::from_secs(5)).unwrap();
                assert!(!path.exists());
                assert!(fs::OpenOptions::new().write(true).open(&original).is_err());
                assert!(fs::remove_file(&original).is_err());
                assert!(fs::OpenOptions::new().write(true).open(&config).is_err());
                rig.render_many_loops(25017);
                assert!(!path.exists());
                assert!(rig.p.h.consumer.peek().is_err());
                release.send(()).unwrap();
                let generated = worker.join().unwrap();
                assert!(
                    rig.p
                        .h
                        .engine
                        .offline_stem_source_is_current(0, &generated.source)
                        .unwrap()
                );
                let pair = prepare_new_disk_output(&mut rig, &generated);
                rig.render_many_loops(4097);
                reject_active_new_output(&mut rig, &generated, &pair);
                assert_complete_selection(&rig.p.h.root, &rig.previous_selection);
            }
        }
    }
}
