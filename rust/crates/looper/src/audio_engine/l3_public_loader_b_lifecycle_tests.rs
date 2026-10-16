//! Publicly born A and ordinary Loader B have distinct voice and asset lifecycles.
//! Four isolated histories cross A FullMix/ALL with B OFF/own addressed ON.
//! B's finite source and pad215's complete source retain independent clocks.

use super::super::committed_stem_public_complete_tests as complete;
use super::*;
use crate::audio_engine::content_placement::ContentKey;
use crate::audio_engine::productive_source_history::ProductiveSourceBinding;

const CHILD: &str = "FLITZI_PUBLIC_LOADER_B_LIFECYCLE_CHILD";
const PREFIX: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::l3_public_loader_b_lifecycle_tests::";

#[test]
fn public_a_full_mix_loader_b_off_stems_replacement_and_unload() {
    isolated(
        false,
        false,
        "public_a_full_mix_loader_b_off_stems_replacement_and_unload",
    );
}

#[test]
fn public_a_all_loader_b_off_stems_replacement_and_unload() {
    isolated(
        true,
        false,
        "public_a_all_loader_b_off_stems_replacement_and_unload",
    );
}

#[test]
fn public_a_full_mix_loader_b_own_on_stems_replacement_and_unload() {
    isolated(
        false,
        true,
        "public_a_full_mix_loader_b_own_on_stems_replacement_and_unload",
    );
}

#[test]
fn public_a_all_loader_b_own_on_stems_replacement_and_unload() {
    isolated(
        true,
        true,
        "public_a_all_loader_b_own_on_stems_replacement_and_unload",
    );
}

fn isolated(all_a: bool, on_b: bool, test: &str) {
    if std::env::var_os(CHILD).is_some() {
        let run: complete::PublicCompleteFollowup = if on_b { wet_history } else { dry_history };
        complete::history_with_followup(true, Some((all_a, run)));
        eprintln!("public Loader B lifecycle child PID={}", std::process::id());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("{PREFIX}{test}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "public Loader B lifecycle child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("public Loader B lifecycle child PID=")
    );
}

fn wet_history(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    _: &[Arc<InputRuntimeOwnership>; 2],
) {
    history(playback, consumer, locals, engine, true);
}

fn dry_history(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    _: &[Arc<InputRuntimeOwnership>; 2],
) {
    history(playback, consumer, locals, engine, false);
}

fn python_bool(locals: &Py<PyDict>, expression: &str) -> bool {
    Python::attach(|py| {
        py.eval(
            &std::ffi::CString::new(expression).unwrap(),
            Some(locals.bind(py)),
            None,
        )
        .unwrap()
        .extract()
        .unwrap()
    })
}

fn python_string(locals: &Py<PyDict>, name: &str) -> String {
    Python::attach(|py| {
        locals
            .bind(py)
            .get_item(name)
            .unwrap()
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn disk_json(locals: &Py<PyDict>, name: &str) -> Value {
    Python::attach(|py| {
        serde_json::from_str(
            &locals
                .bind(py)
                .get_item(name)
                .unwrap()
                .unwrap()
                .call_method0("model_dump_json")
                .unwrap()
                .extract::<String>()
                .unwrap(),
        )
        .unwrap()
    })
}

struct IndependentPeer {
    at: u64,
    consumed: u64,
    sample: SampleBuffer,
    generation: u64,
    native: usize,
    fifo: (usize, usize),
    demand: (StemMixMode, u8, u64),
}

impl IndependentPeer {
    fn new(playback: &ControllerPlayback, initial_prepare_frames: u64) -> Self {
        let peer = complete::other(&playback.callback.mixer);
        Self {
            at: playback.frame,
            // The peer's public T=E=0 precedes the main birth. Its consumption
            // is the declared initial preparation plus independently counted
            // main-source output; their shared paused frames consume neither.
            consumed: initial_prepare_frames + playback.source_frame,
            sample: peer.sample.clone().unwrap(),
            generation: peer.generation,
            native: peer.stretch.native_state_address(),
            fifo: peer.stretch.pending_fifo_frames(),
            demand: playback.callback.mixer.stem_demand_for_measurement(215),
        }
    }

    fn assert_current(&self, playback: &ControllerPlayback) {
        let peer = complete::other(&playback.callback.mixer);
        let consumed = self.consumed + playback.frame - self.at;
        let phase = (consumed as f64 * 0.75).rem_euclid(4096.0);
        assert_eq!(
            peer.source_playback.position().frame,
            phase.floor() as usize
        );
        assert_eq!(
            peer.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(
            peer.source_playback.tempo_ratio().to_bits(),
            0.75_f64.to_bits()
        );
        assert_eq!(peer.generation, self.generation);
        assert_eq!(peer.stretch.native_state_address(), self.native);
        assert_eq!(peer.stretch.pending_fifo_frames(), self.fifo);
        assert!(peer.sample.as_ref().unwrap().same_window(&self.sample));
        assert!(Arc::ptr_eq(
            &peer.sample.as_ref().unwrap().samples,
            &self.sample.samples
        ));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            self.demand
        );
    }
}

struct BOracle {
    peer: IndependentPeer,
    birth: u64,
    generation: u64,
    content_key: ContentKey,
    historical_adoption: Option<u64>,
}

impl BOracle {
    fn assert_owned_history(
        &self,
        playback: &ControllerPlayback,
        binding: ProductiveSourceBinding,
    ) {
        let ownership = playback.callback.mixer.input_ownership_for_effect();
        assert_eq!(ownership.callback_feedback_key(0), Some(self.content_key));
        let current = voice(&playback.callback.mixer);
        assert_eq!(current.generation, self.generation);
        assert_eq!(
            ProductiveSourceBinding::new(current.sample.as_ref().unwrap(), RATE, None),
            binding,
        );
        let history = current.stretch.productive_history().unwrap();
        assert_eq!(history.binding, binding);
        let phase = (playback.source_frame as f64 * 0.75).rem_euclid(1500.0);
        assert_eq!(
            history.next_position.frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            history.next_position.fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(
            history.next_position.seek_mode,
            crate::audio_engine::source_reader::ExplicitSeekMode::Normal
        );
        assert_eq!(
            history.domain,
            Some(
                crate::audio_engine::source_reader::SourceLoopDomain::physical(
                    4096,
                    crate::audio_engine::source_reader::FrameRange {
                        start: LOOP_START,
                        end: LOOP_END
                    },
                )
            )
        );
        assert_eq!(
            current.stretch.native_primed_output_frames(),
            crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES
        );
    }

    fn feed(&self, playback: &ControllerPlayback, start: u64, frames: usize) -> [Vec<f32>; 2] {
        playback.feed(start, frames)
    }

    fn render(&self, playback: &mut ControllerPlayback, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let capacity = playback.callback.mixer.max_realtime_render_frames();
        let mut remaining = frames;
        let mut actual_all = Vec::new();
        let mut full_all = Vec::new();
        while remaining > 0 {
            if playback
                .candidate
                .as_ref()
                .is_some_and(|(target, _)| playback.frame >= *target)
            {
                let (target, raw) = playback.candidate.take().unwrap();
                assert_eq!(playback.frame, target);
                playback.raw = Some(raw);
            }
            let count = remaining.min(capacity).min(
                playback
                    .candidate
                    .as_ref()
                    .map_or(usize::MAX, |(target, _)| {
                        (*target - playback.frame) as usize
                    }),
            );
            let feed = self.feed(playback, playback.source_frame, count);
            let wet = playback.raw.is_some();
            assert_eq!(playback.callback.mixer.key_lock_for_measurement(0), wet);
            // The caller keeps its full output partition. Pending ON clips the
            // first inner feed at this oracle's independent first Native block;
            // scratch then contains only the last inner chunk, with a stale tail.
            // Prepared adoption boundaries are already split above, and .75 has
            // no rate glide. No DUT cursor/read-plan/chunk supplies this prefix.
            let scratch_start = playback.raw.as_ref().map_or(0, |raw| {
                let first_block = raw.native.block_size() - raw.input[0].len();
                if !raw.used && first_block < count {
                    first_block
                } else {
                    0
                }
            });
            let raw = playback.raw.as_mut().map(|raw| raw.render_chosen(&feed));
            let before = voice(&playback.callback.mixer);
            let native = before.stretch.native_state_address();
            let fifo = before.stretch.pending_fifo_frames();
            let mut actual = vec![0.0; count * 2];
            let mut expected = vec![0.0; count * 2];
            let mut full = vec![0.0; count * 2];
            reset_tap_observation_for_test();
            playback.callback.mixer.render_rt_at_output_frame(
                &mut actual,
                &mut [0.0; NUM_SAMPLES],
                playback.frame,
                &mut playback.callback.retirement,
            );
            let taps = tap_observation_for_test();
            assert_eq!(taps.missing_reads, 0);
            // The global tap observation is the union of B17..1517 and 2150..4096.
            // The B scratch, physical residents and prepared worker observation below
            // identify B independently; peer taps are never labelled finite B reads.
            assert!(taps.min_frame.is_some());
            assert!(taps.max_frame.is_some_and(|frame| frame < 4096));
            for (mixer, output) in [
                (playback.reference.as_mut(), &mut expected),
                (playback.full_mix.as_mut(), &mut full),
            ] {
                mixer.render_at_output_frame(playback.frame, output, &mut [0.0; NUM_SAMPLES]);
            }
            assert_eq!(
                actual, expected,
                "B complete-PCM reference chronology at {}",
                playback.frame
            );
            let current = voice(&playback.callback.mixer);
            assert_eq!(current.generation, self.generation);
            assert_eq!(
                (
                    current.sample.as_ref().unwrap().resident_start(),
                    current.sample.as_ref().unwrap().resident_end()
                ),
                (LOOP_START, LOOP_END)
            );
            for (channel, samples) in feed.iter().enumerate() {
                assert_eq!(
                    &current.stretch.varispeed_buffers()[channel][..count - scratch_start],
                    &samples[scratch_start..],
                    "B exact final inner source-feed scratch"
                );
                if !wet {
                    assert_eq!(&current.stretch.output_buffers()[channel][..count], samples);
                }
            }
            if let Some(raw) = &playback.raw {
                assert_eq!(
                    current.stretch.pending_fifo_frames(),
                    (raw.input[0].len(), raw.output[0].len()),
                    "B per-render independent raw Native FIFO"
                );
            }
            assert!(
                current
                    .source_playback
                    .matches_exact(&voice(&playback.reference).source_playback)
            );
            assert!(
                current
                    .source_playback
                    .matches_exact(&voice(&playback.full_mix).source_playback)
            );
            assert_eq!(
                current.stretch.pending_fifo_frames(),
                voice(&playback.reference).stretch.pending_fifo_frames()
            );
            let chosen = if let Some(raw) = raw {
                raw
            } else {
                assert_eq!(current.stretch.native_state_address(), native);
                assert_eq!(current.stretch.pending_fifo_frames(), fifo);
                assert!(current.stretch.productive_history().is_none());
                assert!(!current.stretch.wet_render_ready());
                assert!(!current.stretch.wet_output_ready());
                assert!(!current.stretch.wet_current_source_output_ready());
                assert_eq!(
                    current.stretch.adopted_request_id(),
                    self.historical_adoption
                );
                (0..count)
                    .flat_map(|frame| [feed[0][frame], feed[1][frame]])
                    .collect()
            };
            for (output, raw) in actual.chunks_exact(2).zip(chosen.chunks_exact(2)) {
                playback.filter.begin_frame();
                for channel in 0..2 {
                    assert_eq!(
                        output[channel],
                        playback.filter.process_sample(channel, raw[channel]),
                        "B independent feed/Native/uninterrupted EQ"
                    );
                }
            }
            if !wet {
                for mixer in [
                    &mut playback.callback.mixer,
                    &mut playback.reference,
                    &mut playback.full_mix,
                ] {
                    assert!(!voice_mut(mixer).stretch.source_preparation_ready());
                }
            }
            playback.frame += count as u64;
            playback.source_frame += count as u64;
            let phase = (playback.source_frame as f64 * 0.75).rem_euclid(1500.0);
            let current = voice(&playback.callback.mixer);
            assert_eq!(
                current.source_playback.position().frame,
                LOOP_START + phase.floor() as usize
            );
            assert_eq!(
                current.source_playback.position().fraction.to_bits(),
                phase.fract().to_bits()
            );
            assert_eq!(
                current.source_playback.tempo_ratio().to_bits(),
                0.75_f64.to_bits()
            );
            assert_eq!(playback.frame - playback.source_frame, self.birth);
            self.peer.assert_current(playback);
            actual_all.extend(actual);
            full_all.extend(full);
            remaining -= count;
        }
        (actual_all, full_all)
    }

    fn native(
        &self,
        playback: &mut ControllerPlayback,
        minimum: u64,
        initial: bool,
    ) -> (Vec<f32>, Vec<f32>) {
        wait_until(|| {
            voice_mut(&mut playback.callback.mixer)
                .stretch
                .source_preparation_ready()
                && voice_mut(&mut playback.reference)
                    .stretch
                    .source_preparation_ready()
                && (!initial
                    || voice_mut(&mut playback.full_mix)
                        .stretch
                        .source_preparation_ready())
        });
        let target = voice_mut(&mut playback.callback.mixer)
            .stretch
            .prepared_target_output_frame()
            .unwrap();
        let prepared = voice_mut(&mut playback.callback.mixer)
            .stretch
            .prepared_native_address();
        // Copy the exact own-B source binding before this candidate's effect;
        // no additional source/PCM pin survives into final reader cleanup.
        let binding = ProductiveSourceBinding::new(
            voice(&playback.callback.mixer).sample.as_ref().unwrap(),
            RATE,
            None,
        );
        assert_ne!(prepared, 0);
        assert_ne!(
            prepared,
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address()
        );
        assert_eq!(
            voice_mut(&mut playback.reference)
                .stretch
                .prepared_target_output_frame(),
            Some(target)
        );
        let taps = voice_mut(&mut playback.callback.mixer)
            .stretch
            .prepared_tap_observation()
            .expect("own finite B Native worker observation");
        assert_eq!(taps.missing_reads, 0);
        assert!(taps.min_frame.is_some_and(|frame| frame >= LOOP_START));
        assert!(taps.max_frame.is_some_and(|frame| frame < LOOP_END));
        let warm_start =
            target - crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES as u64;
        let stage = if initial {
            "B addressed ON FullMix"
        } else if playback.previous.is_some() {
            "B replacement ALL"
        } else {
            "B first ALL"
        };
        assert!(
            warm_start >= minimum.max(self.birth),
            "{stage}: own B4096 Native history must follow its request/ramp; target={target}, warm_start={warm_start}, minimum={minimum}, birth={}, current_frame={}, source_frame={}, initial={initial}, selection={:?}, previous={:?}, ramp_start={}",
            self.birth,
            playback.frame,
            playback.source_frame,
            playback.selection,
            playback.previous,
            playback.ramp_start,
        );
        let mut raw = ControllerRawNative::new(0.75, true);
        raw.render(&self.feed(playback, warm_start - self.birth, 4096));
        playback.candidate = Some((target, raw));
        let mut partitions = [1_usize, 127, 777, 96, 257, 512, 31].into_iter().cycle();
        while playback.frame < target {
            let count = partitions
                .next()
                .unwrap()
                .min((target - playback.frame) as usize);
            self.render(playback, count);
        }
        let (mut actual, mut full) = self.render(playback, 137);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            prepared
        );
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_tap_observation(),
            Some(taps)
        );
        let mut remaining = voice(&playback.callback.mixer)
            .stretch
            .rubberband_block_size()
            * 8;
        for count in [1_usize, 31, 137, 257, 512].into_iter().cycle() {
            let count = count.min(remaining);
            let (output, counterfactual) = self.render(playback, count);
            actual.extend(output);
            full.extend(counterfactual);
            assert_eq!(
                voice(&playback.callback.mixer)
                    .stretch
                    .native_state_address(),
                prepared
            );
            remaining -= count;
            if remaining == 0 {
                break;
            }
        }
        let raw = playback.raw.as_ref().unwrap();
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .pending_fifo_frames(),
            (raw.input[0].len(), raw.output[0].len()),
            "own later B Native FIFO"
        );
        self.assert_owned_history(playback, binding);
        (actual, full)
    }

    fn ramp(&self, playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str) {
        let native = voice(&playback.callback.mixer)
            .stretch
            .native_state_address();
        let mut remaining = (128.0_f64 / 0.75).ceil() as usize
            - (playback.source_frame - playback.ramp_start) as usize;
        for count in [1_usize, 31, 17, 7].into_iter().cycle() {
            let count = count.min(remaining);
            self.render(playback, count);
            assert_eq!(
                voice(&playback.callback.mixer)
                    .stretch
                    .native_state_address(),
                native
            );
            remaining -= count;
            if remaining == 0 {
                break;
            }
            assert_eq!(
                controller_ticket_status(locals, name, "effective_mode_status"),
                "transitioning"
            );
            assert!(
                playback
                    .callback
                    .mixer
                    .stem_transition_for_measurement(0)
                    .is_active()
            );
        }
        assert!(
            !playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
    }

    fn settle(&self, playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str, ack: u64) {
        self.ramp(playback, locals, name);
        let (actual, full) = if playback.raw.is_some() {
            assert_eq!(
                controller_ticket_status(locals, name, "effective_mode_status"),
                "transitioning"
            );
            let after_ramp = playback.frame;
            self.render(playback, 13);
            self.native(playback, ack.max(after_ramp), false)
        } else {
            self.render(playback, 1064)
        };
        assert!(actual.iter().any(|sample| sample.abs() > 0.001));
        assert!(
            actual
                .iter()
                .zip(full)
                .any(|(actual, full)| (actual - full).abs() > 0.005)
        );
        assert_eq!(
            controller_ticket_status(locals, name, "effective_mode_status"),
            "effective"
        );
        complete::feedback(locals, &mut playback.callback);
        controller_python(
            locals,
            &format!(
                "assert {name}.is_current() and {name}.claimed()\nassert {name}.error() is None and {name}.effective_failure_reason() is None\nassert not session.stem_generation_errors"
            ),
        );
    }
}

fn inactive(playback: &mut ControllerPlayback, peer: &IndependentPeer, frames: usize) {
    let mut output = vec![0.0; frames * 2];
    playback.callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        playback.frame,
        &mut playback.callback.retirement,
    );
    assert!(output.iter().all(|sample| *sample == 0.0));
    assert!(
        !playback
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    playback.frame += frames as u64;
    peer.assert_current(playback);
}

fn fifo_completion(playback: &mut ControllerPlayback, probe: &FinalReaderProbe, stage: &str) {
    let marker = Arc::new(AtomicU8::new(0));
    let completion = Arc::downgrade(&marker);
    playback.callback.retirement.retire_cold_adoption(marker);
    probe.wait_for(stage, || completion.strong_count() == 0);
    assert!(playback.callback.retirement.retired.is_empty());
}

fn wait_cleanup(engine: &AudioEngine, locals: &Py<PyDict>, probe: &FinalReaderProbe) {
    probe.wait_for("B actual final Native/history readers", || {
        Python::attach(|py| {
            engine.poll_loader_events(py).unwrap();
        });
        controller_python(locals, "controller.on_frame_render()");
        probe.identity.strong_count() == 0 && probe.pcm.iter().all(|pcm| pcm.strong_count() == 0)
    });
    probe.wait_for("B WAV/PCM/descriptor asset worker cleanup", || {
        Python::attach(|py| {
            engine.poll_loader_events(py).unwrap();
        });
        probe.areas.iter().all(|path| !path.exists())
    });
}

fn b_files(
    root: &Path,
    original: &str,
    version: &str,
    replacement: bool,
) -> (String, [SampleBuffer; 4]) {
    let material =
        crate::audio_engine::material_paths::material_root_for_original(root, Path::new(original))
            .unwrap();
    let path = material.join("stems").join(format!(
        ".ready-{:032x}",
        if replacement { 0xb502_u32 } else { 0xb501_u32 }
    ));
    write_signal_set(&path, 4096, version, 4);
    if replacement {
        // Melody/bass permutation changes selected1010 but preserves the actual
        // accompaniment sum and complete B sum; no A0..3 file is used.
        fs::write(path.join("melody.wav"), signal_wav(4096, 6)).unwrap();
        fs::write(path.join("bass.wav"), signal_wav(4096, 5)).unwrap();
        let hashes: serde_json::Map<_, _> = STEM_FILE_NAMES
            .iter()
            .map(|name| {
                let bytes = fs::read(path.join(format!("{name}.wav"))).unwrap();
                (
                    (*name).to_owned(),
                    json!(format!("{:x}", Sha256::digest(bytes))),
                )
            })
            .collect();
        fs::write(
            path.join(".complete.json"),
            serde_json::to_vec(&json!({
                "schema": "stem-set-sha256-v1", "source_version": version, "stems": hashes,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let components = std::array::from_fn(|index| {
        replacement_component_pcm(
            &fs::read(path.join(format!("{}.wav", STEM_FILE_NAMES[index]))).unwrap(),
        )
    });
    let project = fs::canonicalize(root.parent().unwrap()).unwrap();
    (
        path.strip_prefix(project)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/"),
        components,
    )
}

/// The public PadController invokes the real parameter queue kernel; only the
/// device stream's producer lookup is supplied by this isolated fixture.
#[pyclass]
struct BMixAudio {
    parameters: Arc<Mutex<rtrb::Producer<ControlParameterMessage>>>,
}

#[pymethods]
impl BMixAudio {
    fn set_pad_eq(&self, id: usize, low_db: f32, mid_db: f32, high_db: f32) -> PyResult<()> {
        assert_eq!(id, 0);
        assert_eq!([low_db, mid_db, high_db], [-12.0, -3.0, 4.0]);
        crate::audio_engine::push_parameter_message(
            &mut self.parameters.lock().unwrap(),
            ControlParameterMessage::SetPadEq {
                id,
                low_db,
                mid_db,
                high_db,
            },
            "SetPadEq",
        )
    }
}

fn history(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    engine: &Arc<AudioEngine>,
    on_b: bool,
) {
    assert_eq!(playback.ratio, 0.75);
    assert!(playback.raw.is_some() && consumer.is_empty());
    assert!(playback.callback.mixer.key_lock_for_measurement(0));
    assert!(
        voice(&playback.callback.mixer)
            .stretch
            .wet_current_source_output_ready()
    );
    let initial_prepare_frames = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("initial_prepare_frames")
            .unwrap()
            .unwrap()
            .extract::<u64>()
            .unwrap()
    });
    let peer = IndependentPeer::new(playback, initial_prepare_frames);
    peer.assert_current(playback);
    let root = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("audio")
            .unwrap()
            .unwrap()
            .extract::<PyRef<PairWorkerAudio>>()
            .unwrap()
            .root
            .clone()
    });
    let producer = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("audio")
            .unwrap()
            .unwrap()
            .extract::<PyRef<PairWorkerAudio>>()
            .unwrap()
            .producer
            .clone()
    });
    let (parameter_producer, mut parameters) = rtrb::RingBuffer::new(16);
    let parameter_producer = Arc::new(Mutex::new(parameter_producer));
    let (actual, worker) = crate::audio_engine::buffer_retirement::create_audio_buffer_retirement();
    playback.callback.retirement.retired.clear();
    playback.callback.retirement.actual = Some(actual);
    let input = root.parent().unwrap().join("public-loader-b.wav");
    let bytes = signal_sum_wav(4096, &[4, 5, 6, 7]);
    let pcm: Vec<f32> = bytes[44..]
        .chunks_exact(2)
        .map(|bytes| f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32768.0)
        .collect();
    fs::write(&input, &bytes).unwrap();
    Python::attach(|py| {
        let locals = locals.bind(py);
        locals
            .set_item("b_input", input.to_string_lossy().as_ref())
            .unwrap();
        locals.set_item("b_on", on_b).unwrap();
        locals
            .set_item(
                "captured_audio",
                Py::new(
                    py,
                    PublicCapturedStartAudio {
                        engine: engine.clone(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        locals
            .set_item(
                "load_audio",
                Py::new(
                    py,
                    PublicLoadAudio {
                        engine: engine.clone(),
                        producer,
                        parameters: parameter_producer.clone(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        locals
            .set_item(
                "b_mix_audio",
                Py::new(
                    py,
                    BMixAudio {
                        parameters: parameter_producer,
                    },
                )
                .unwrap(),
            )
            .unwrap();
    });
    controller_python(
        locals,
        r#"
from pathlib import Path
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.persistence import ProjectPersistence
persistence = ProjectPersistence(project)
persistence.mark_dirty()
persistence.flush()
controller.bind_disk_selection_commit(persistence.commit_stem_selection, persistence.flush)
class BLoadAudio:
    def __init__(self):
        self.success = []
    def __getattr__(self, name):
        for delegate in (load_audio, playback_audio, audio):
            if hasattr(delegate, name):
                return getattr(delegate, name)
        raise AttributeError(name)
    def poll_loader_events(self):
        event = load_audio.poll_loader_events()
        if event is not None and event.get('type') == 'success':
            assert event['analysis']['bpm'] == 0 and event['analysis']['beat_grid']['beats'] == []
            self.success.append((event['request_id'], dict(event['incoming_defaults'])))
        return event
b_load_audio = BLoadAudio()
b_notifications = []
loader = LoaderController(project, session, b_load_audio, b_notifications.append, persistence.mark_dirty)
loader.bind_asset_lifecycle(controller._assets)
loader.set_stems_invalidated_callback(controller.invalidate_stem_cache)
a_path, a_content = project.sample_paths[0], project.pad_content[0]
a_mode = project.pad_stem_mix_mode[0]
a_source = audio.capture_prepared_source(0, fixture['source_version'])
a_mode_feedback = transport.residency._mode_feedback(0)
assert a_mode_feedback['effective'] is True and a_mode_feedback['ready'] is True
a_workers = audio.worker_call_count()
assert session.active_sample_ids == {0, 215} and not session.paused_sample_ids
loader.load_sample_async(0, b_input)
b_request = loader._load_request_ids[0]
b_defaults = {'revision': 0, 'key_lock': False, 'start_s': 0.0, 'end_s': None,
    'period_seconds': None, 'grid_origin_s': 0.0, 'loop_auto': True, 'loop_bars': 8.0}
assert not project.key_lock and project.pad_key_lock[0] is True
assert project.sample_paths[0] == a_path and project.pad_content[0] == a_content
assert project.pad_stem_mix_mode[0] == a_mode and not b_load_audio.success
assert 0 in session.loading_sample_ids and session.pending_sample_paths[0] == b_input
"#,
    );
    let a_generation = voice(&playback.callback.mixer).generation;
    let a_native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let a_adoption = voice(&playback.callback.mixer).stretch.adopted_request_id();
    let a_sample = Arc::downgrade(
        &voice(&playback.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .samples,
    );
    playback.callback.retirement.slots = 0;
    wait_until(|| consumer.peek().is_ok());
    let adoption = match consumer.peek().unwrap() {
        ControlMessage::LoadColdSample {
            id,
            sample,
            adoption,
            incoming_defaults,
            replace_assignment,
            ..
        } => {
            assert_eq!(*id, 0);
            assert!(*replace_assignment);
            assert_eq!(sample.samples.as_ref(), pcm.as_slice());
            assert_eq!(
                (
                    sample.resident_start(),
                    sample.resident_end(),
                    sample.frame_count()
                ),
                (0, 4096, 4096)
            );
            let defaults = incoming_defaults.expect("B owns its Native default receipt");
            assert_eq!(defaults.sample_rate_hz, RATE);
            assert_eq!(defaults.revision, 0);
            assert!(!defaults.key_lock && defaults.loop_auto);
            assert_eq!(defaults.start_frame, 0);
            assert_eq!(defaults.end_frame, None);
            assert_eq!(defaults.period_seconds, None);
            assert_eq!(defaults.origin_frame, 0.0);
            assert_eq!(defaults.loop_bars, 8.0);
            adoption.clone()
        }
        _ => panic!("B's own ordinary public LoadColdSample"),
    };
    for frames in [1, 31, 137, 257] {
        assert_eq!(playback.drain(consumer), 0);
        playback.render(frames, false);
        peer.assert_current(playback);
        let a = voice(&playback.callback.mixer);
        assert_eq!(a.generation, a_generation);
        assert_eq!(a.stretch.native_state_address(), a_native);
        assert_eq!(a.stretch.adopted_request_id(), a_adoption);
        assert!(a_sample.ptr_eq(&Arc::downgrade(&a.sample.as_ref().unwrap().samples)));
        assert_eq!(adoption.load(Ordering::Acquire), 0);
        controller_python(
            locals,
            "loader.poll_loader_events()\nassert project.sample_paths[0] == a_path and project.pad_content[0] == a_content\nassert not b_load_audio.success\nassert audio.worker_call_count() == a_workers",
        );
    }
    let a_clock = voice(&playback.callback.mixer).source_playback;
    let before = playback.callback.feedback.events.len();
    let stopped_feedback =
        ExpectedStoppedFeedback::source_command(&playback.callback.mixer, consumer, 0);
    playback.callback.retirement.slots = usize::MAX;
    assert_eq!(
        playback.drain(consumer),
        1,
        "B own Back ACK stops A without auto-starting B"
    );
    assert_eq!(adoption.load(Ordering::Acquire), 2);
    assert_eq!(playback.callback.feedback.events.len(), before + 1);
    stopped_feedback.assert_message(&playback.callback.feedback.events[before]);
    assert!(a_clock.position().frame < 4096);
    assert_eq!(playback.callback.mixer.loop_region_frames(0), (0, None));
    assert!(!playback.callback.mixer.key_lock_for_measurement(0));
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    complete::feedback(locals, &mut playback.callback);
    wait_until(|| {
        controller_python(
            locals,
            "loader.poll_loader_events()\nassert not session.sample_load_errors, session.sample_load_errors",
        );
        python_bool(locals, "0 not in session.loading_sample_ids")
    });
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    controller_python(
        locals,
        r#"
assert b_load_audio.success == [(b_request, b_defaults)] and b_notifications == []
assert project.sample_paths[0] != a_path and project.pad_content[0] != a_content
assert project.pad_content[0].material_id != a_content.material_id
assert session.active_sample_ids == {215} and not session.paused_sample_ids
assert project.stem_cache[0] is None and project.pad_stem_mix_mode[0] == 'full_mix'
assert not controller._committed_residency.contains(0)
assert project.pad_key_lock[0] is None and not project.resolve_pad_key_lock(0)
assert project.pad_loop_start_s[0] == 0.0 and project.pad_loop_end_s[0] is None
assert project.pad_loop_auto[0] and project.pad_loop_bars[0] == 8.0
assert project.manual_bpm[0] is None and project.pad_timing_intent[0] == 'legacy'
assert project.pad_gain_db[0] == 0
assert (project.pad_eq_low_db[0], project.pad_eq_mid_db[0], project.pad_eq_high_db[0]) == (0, 0, 0)
b_path, b_content = project.sample_paths[0], project.pad_content[0]
b_version = controller.source_version_for_pad(0)
b_origin = audio.capture_prepared_source(0, b_version)
assert b_version != fixture['source_version'] and not b_origin.same_source_request(a_source)
b_native_identity = b_load_audio.waveform_source_identity(0)
assert b_native_identity[0] == b_request and b_native_identity[2:] == (4096, fixture['rate'])
assert audio.worker_call_count() == a_workers
persistence.mark_dirty()
persistence.flush()
"#,
    );
    assert!(consumer.is_empty() && parameters.is_empty());
    for frames in [1, 31, 137] {
        inactive(playback, &peer, frames);
    }
    synchronize_callback_frame(&mut playback.callback, playback.frame);
    controller_python(
        locals,
        r#"
class BTransportAudio:
    def __getattr__(self, name):
        for delegate in (b_mix_audio, playback_audio):
            if hasattr(delegate, name):
                return getattr(delegate, name)
        raise AttributeError(name)
    def capture_pad_start(self, *args, **kwargs):
        original = playback_audio.capture_pad_start(*args, **kwargs)
        captured_start_calls.append((original, kwargs.get('received_at_ns')))
        return original
    def prepare_captured_pad_start(self, original, window=None):
        captured_dependency_calls.append((original, window))
        return captured_audio.prepare_captured_pad_start(original, window)
    def play_sample(self, *args, **kwargs):
        raise AssertionError('B Play awaits its own public resident window ACK')
    play_sample_exclusive = play_sample
project.pad_loop_auto[0] = False
project.pad_loop_start_s[0] = 17 / fixture['rate']
project.pad_loop_end_s[0] = 1517 / fixture['rate']
transport = TransportController(project, session, BTransportAudio(), persistence.mark_dirty)
captured_start_calls, captured_dependency_calls = [], []
ordinary_timestamp = captured_audio.capture_input_timestamp_ns()
transport.playback.trigger_pad_keep_others(0, received_at_ns=ordinary_timestamp)
ordinary_launch = next(iter(transport.residency._captured_starts.values()))
ordinary_start = ordinary_launch.start
b_window = ordinary_launch.ticket
assert b_window is not None and b_window.error() is None
assert ordinary_launch.dependency is None
assert ordinary_start.original_frame() is None and ordinary_start.effective_frame() is None
assert 0 not in session.active_sample_ids
"#,
    );
    wait_until(|| {
        Python::attach(|py| {
            locals
                .bind(py)
                .get_item("b_window")
                .unwrap()
                .unwrap()
                .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
                .unwrap()
                .control_published_for_test()
        })
    });
    assert_original_target_at_front(consumer, locals);
    playback.callback.retirement.slots = 2;
    let original_target = playback.frame;
    assert_eq!(
        playback.drain(consumer),
        1,
        "original B target before held own window"
    );
    assert_eq!(
        ordinary_start_from_python(locals).original_frame(),
        Some(original_target)
    );
    assert!(matches!(
        consumer.peek(),
        Ok(ControlMessage::RelocateResident(_))
    ));
    for frames in [1, 31, 137] {
        inactive(playback, &peer, frames);
        controller_python(
            locals,
            "transport.on_frame_render()\nassert b_window.publication_status() == 'pending'\nassert 0 not in session.active_sample_ids",
        );
        assert_eq!(playback.drain(consumer), 0);
    }
    playback.callback.retirement.slots = usize::MAX;
    assert_eq!(playback.drain(consumer), 1, "own B finite resident ACK");
    controller_python(
        locals,
        "assert b_window.publication_status() == 'accepted' and b_window.is_current()",
    );
    let resident = playback.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            resident.resident_start(),
            resident.resident_end(),
            resident.frame_count()
        ),
        (LOOP_START, LOOP_END, 4096)
    );
    assert_eq!(
        resident.samples.as_ref(),
        &pcm[LOOP_START * 2..LOOP_END * 2]
    );
    Python::attach(|py| {
        let window = locals.bind(py).get_item("b_window").unwrap().unwrap();
        let window = window
            .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
            .unwrap();
        let observed = window.read_observation_for_test().unwrap();
        assert_eq!(
            (
                observed.source.start_frame,
                observed.source.end_frame,
                observed.source.read_bytes
            ),
            (LOOP_START, LOOP_END, 1500 * 2 * 4)
        );
        assert!(observed.stems.read_bytes.iter().all(|bytes| *bytes == 0));
    });
    inactive(playback, &peer, 31);
    assert!(consumer.is_empty());
    synchronize_callback_frame(&mut playback.callback, playback.frame);
    let start = admit_original_public_start(&mut playback.callback, consumer, locals);
    assert_eq!(start.original_frame(), Some(original_target));
    let birth = start.execution_frame().unwrap();
    while playback.frame < birth {
        let frames = (birth - playback.frame).min(257) as usize;
        let mut output = vec![0.0; frames * 2];
        let callback = playback.callback.as_mut();
        crate::audio_engine::audio_stream::render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        callback.transport.advance_by_rendered_frames(frames);
        playback.frame += frames as u64;
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert!(
            !playback
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0))
        );
        assert!(start.effective_frame().is_none());
        peer.assert_current(playback);
    }
    execute_public_start_at_frame(&mut playback.callback, &start);
    assert_eq!(playback.frame, birth);
    peer.assert_current(playback);
    let generation = voice(&playback.callback.mixer).generation;
    assert_eq!(generation, a_generation + 2);
    assert_eq!(
        voice(&playback.callback.mixer)
            .source_playback
            .position()
            .frame,
        LOOP_START
    );
    assert_eq!(
        voice(&playback.callback.mixer).stretch.adopted_request_id(),
        a_adoption
    );
    complete::feedback(locals, &mut playback.callback);
    controller_python(
        locals,
        "assert session.active_sample_ids == {0, 215}\nassert transport_events.count(('started', 0)) == 2 and transport_events.count(('stopped', 0)) == 1",
    );
    let source = SampleBuffer {
        channels: 2,
        samples: Arc::from(pcm),
        residency: None,
    };
    let version = python_string(locals, "b_version");
    let components =
        std::array::from_fn(|index| replacement_component_pcm(&signal_wav(4096, index + 4)));
    // Independent immutable B renderers have unrestricted reference component permits.
    let owners: [Arc<InputRuntimeOwnership>; 2] =
        std::array::from_fn(|_| Arc::new(InputRuntimeOwnership::default()));
    let hash = crate::audio_engine::stem_cache::source_version_hash(&version);
    let stems = PreparedStemSet {
        complete_set_identity: Arc::new([0xb5; 32]),
        reference_samples: source.samples.clone(),
        reference_residency: source.residency.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        accepted_timing: None,
        source_version_hash: hash,
        sample_rate_hz: RATE,
        channels: 2,
        frame_count: 4096,
        available_mask: 0b1111,
        stems: components.clone(),
    };
    let reference = |index: usize| {
        let mut mixer = Box::new(RtMixer::new(2, RATE as f32));
        owners[index].publish_test_source(0, &source, RATE, 1);
        mixer.set_input_runtime_ownership(owners[index].clone());
        mixer.load_sample(0, source.clone());
        mixer.set_loop_region_frames(0, LOOP_START, Some(LOOP_END));
        mixer.set_speed(0.75);
        assert!(mixer.publish_prepared_stems(0, stems.clone()));
        assert!(mixer.play_sample_at_output_frame(0, 1.0, birth));
        mixer
    };
    playback.reference = reference(0);
    playback.full_mix = reference(1);
    playback.source = source;
    playback.components = components;
    playback.source_frame = 0;
    playback.resumed_at = Some(birth);
    playback.seek_epoch = None;
    playback.complete_resident = false;
    playback.loop_start = LOOP_START;
    playback.loop_end = LOOP_END;
    playback.loop_period = 1500.0;
    playback.selection = None;
    playback.previous = None;
    playback.ramp_start = 0;
    playback.raw = None;
    playback.candidate = None;
    playback.filter = PerPadDspChain::new(0, RATE as f32, 2);
    playback.filter.reset();
    let oracle = BOracle {
        peer,
        birth,
        generation,
        content_key: start.content_key(),
        historical_adoption: a_adoption,
    };
    for frames in [1, 31, 137, 257, 511, 511, 257, 511] {
        let (actual, full) = oracle.render(playback, frames);
        assert_eq!(actual, full, "B own OFF FullMix after A ON");
    }
    assert!(playback.source_frame as f64 * 0.75 > 1500.0);
    controller_python(locals, "transport.pad.set_pad_eq(0, -12.0, -3.0, 4.0)");
    crate::audio_engine::audio_stream::drain_parameter_messages(
        &mut parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    assert!(parameters.is_empty());
    for mixer in [&mut playback.reference, &mut playback.full_mix] {
        mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    }
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
        assert!(playback.filter.set_parameter(
            DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap(),
            normalized
        ));
    }
    for frames in [1, 31, 137] {
        let (actual, full) = oracle.render(playback, frames);
        assert_eq!(actual, full, "own public B EQ retains continuous FullMix");
    }
    if on_b {
        public_b_on(playback, consumer, locals, &owners, &oracle);
    }
    let b_full_mix_native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let b_full_mix_adoption = on_b.then(|| {
        voice(&playback.callback.mixer)
            .stretch
            .adopted_request_id()
            .expect("own B FullMix source-history worker already adopted")
    });
    let original = python_string(locals, "b_path");
    let (reference1, components1) = b_files(&root, &original, &version, false);
    assert!(
        components1
            .iter()
            .zip(&playback.components)
            .all(|(file, oracle)| file.samples.as_ref() == oracle.samples.as_ref())
    );
    queue_pair(playback, consumer, locals, &oracle, &reference1, false);
    controller_python(
        locals,
        r#"
b1_disk = project.stem_cache[0].pair
b1_live = controller._committed_residency._live_selections[0]
assert b1_live.control is None and b1_live.attempt is None and b1_live.effective_identity is None
assert not b1_live.disk.prepared.has_components()
assert controller.set_stem_enabled_mask(0, 0b1010)
assert controller.set_stem_mix_mode(0, 'all_stems')
b1 = b1_live.attempt.receipt
assert b1 is not None and b1.effective_mode_status() == 'waiting' and not b1.claimed()
"#,
    );
    let ack1 = adopt(playback, consumer, locals, &oracle, "b1");
    assert!(playback.reference.set_stem_enabled_mask(0, 0b1010, hash));
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::AllStems, hash)
    );
    playback.select(Some(0b1010));
    let old = probe(&root, playback, locals, "b1_disk", &components1, hash);
    oracle.settle(playback, locals, "b1", ack1);
    old.assert_retained();
    let native1 = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let adopted1 = voice(&playback.callback.mixer).stretch.adopted_request_id();
    if let Some(previous) = b_full_mix_adoption {
        assert_ne!(native1, b_full_mix_native);
        // Both values now belong to B's same live source-history worker lane.
        // A's ordinary input sequence is an unrelated numeric identity domain.
        assert!(adopted1.expect("own B1 source history adopted") > previous);
    }
    let (reference2, components2) = b_files(&root, &original, &version, true);
    assert_eq!(
        components2[1].samples.as_ref(),
        components1[2].samples.as_ref()
    );
    assert_eq!(
        components2[2].samples.as_ref(),
        components1[1].samples.as_ref()
    );
    queue_pair(playback, consumer, locals, &oracle, &reference2, true);
    controller_python(
        locals,
        r#"
b2_disk = project.stem_cache[0].pair
assert b2_disk != b1_disk and b2_disk.stem_set_identity != b1_disk.stem_set_identity
b2_live = controller._committed_residency._live_selections[0]
b2 = b2_live.attempt.receipt
assert b2 is not b1 and b2.is_current() and not b2.claimed()
assert b2.effective_mode_status() == 'waiting' and b1.effective_mode_status() == 'effective'
"#,
    );
    let ack2 = adopt(playback, consumer, locals, &oracle, "b2");
    old.assert_retained();
    // B2 swaps melody/bass. Actual1010 therefore selects original B1's1100.
    // This independent complete-PCM reference changes only scalar selection;
    // its live voice/Native/EQ history continues through the same input ramp.
    assert!(playback.reference.set_stem_enabled_mask(0, 0b1100, hash));
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::AllStems, hash)
    );
    playback.select(Some(0b1100));
    let current = probe(&root, playback, locals, "b2_disk", &components2, hash);
    controller_python(
        locals,
        "del b1, b1_live\nloader.poll_loader_events()\ncontroller.on_frame_render()",
    );
    playback.callback.retirement.slots = 0;
    oracle.ramp(playback, locals, "b2");
    // The next timestamped frame transfers outgoing owners and may already
    // request B2's settled4096 history. Its minimum is the actual ramp end,
    // after own Adopt and all128 consumed source frames, before that first feed.
    let ramp_end_frame = playback.frame;
    let outgoing = playback
        .callback
        .mixer
        .outgoing_stems_for_measurement(0)
        .unwrap();
    assert!(
        old.identity
            .ptr_eq(&Arc::downgrade(&outgoing.complete_set_identity))
    );
    old.assert_retained();
    current.assert_retained();
    playback.callback.retirement.slots = usize::MAX;
    oracle.render(playback, 1);
    assert!(
        playback
            .callback
            .mixer
            .outgoing_stems_for_measurement(0)
            .is_none()
    );
    fifo_completion(playback, &old, "B1 outgoing production FIFO completion");
    if on_b {
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native1
        );
        assert_eq!(
            voice(&playback.callback.mixer).stretch.adopted_request_id(),
            adopted1
        );
        assert_eq!(
            controller_ticket_status(locals, "b2", "effective_mode_status"),
            "transitioning"
        );
        old.assert_retained();
        oracle.render(playback, 13);
        let (actual, full) = oracle.native(playback, ack2.max(ramp_end_frame), false);
        assert!(
            actual
                .iter()
                .zip(full)
                .any(|(actual, full)| (actual - full).abs() > 0.005)
        );
        assert_ne!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native1
        );
        assert_ne!(
            voice(&playback.callback.mixer).stretch.adopted_request_id(),
            adopted1
        );
    } else {
        oracle.render(playback, 1064);
    }
    assert_eq!(
        controller_ticket_status(locals, "b2", "effective_mode_status"),
        "effective"
    );
    complete::feedback(locals, &mut playback.callback);
    wait_cleanup(engine, locals, &old);
    current.assert_retained();
    for frames in [1, 31, 137, 511] {
        oracle.render(playback, frames);
        current.assert_retained();
    }
    controller_python(
        locals,
        r#"
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == b2_disk.stem_set_identity
assert status.control_stem_set_identity == b2_disk.stem_set_identity
assert status.effective_stem_set_identity == b2_disk.stem_set_identity and status.effective == 'effective'
assert b2.claimed() and b2.is_current() and b2.error() is None and b2.effective_failure_reason() is None
assert project.sample_paths[0] == b_path and project.pad_content[0] == b_content
assert audio.worker_call_count() == a_workers + 2 and audio.prepared_only_on_pool_threads()
persistence.mark_dirty()
persistence.flush()
b_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert b_reopened.load_error is None and b_reopened.project.stem_cache[0].pair == b2_disk
assert b_reopened.project.sample_paths[0] == b_path and b_reopened.project.pad_content[0] == b_content
assert b_reopened.project.pad_stem_mix_mode[0] == 'all_stems'
assert b_reopened.project.pad_key_lock[0] is (True if b_on else None)
assert (b_reopened.project.pad_eq_low_db[0], b_reopened.project.pad_eq_mid_db[0], b_reopened.project.pad_eq_high_db[0]) == (-12, -3, 4)
assert not b_reopened.project.key_lock and not b_reopened.project.resolve_pad_key_lock(215)
"#,
    );
    public_b_unload(
        playback,
        consumer,
        &mut parameters,
        locals,
        engine,
        &oracle,
        &current,
    );
    assert!(old.areas.iter().all(|path| !path.exists()));
    // This child proves B cleanup while215 remains independently running.
    // The common complete-history finish subsequently stops215; its final cleanup
    // is outside this B scope. Remove this drained sink before its worker ends.
    playback.callback.retirement.actual.take();
    drop(worker);
}

fn public_b_on(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    owners: &[Arc<InputRuntimeOwnership>; 2],
    oracle: &BOracle,
) {
    controller_python(
        locals,
        r#"
b_off = audio.capture_prepared_source(0, b_version)
assert b_off.same_source_request(b_origin) and not b_off.same_source_request(a_source)
transport.pad.set_pad_key_lock(0, enabled=True)
b_on_window = transport.residency._pending[0].ticket
assert b_on_window is not None and b_on_window.key_lock_request_id is not None
assert project.pad_key_lock[0] is True and not project.key_lock and not project.resolve_pad_key_lock(215)
assert transport.residency.key_lock_status(0).pending
"#,
    );
    wait_until(|| consumer.peek().is_ok());
    playback.callback.retirement.slots = 2;
    assert_eq!(playback.drain(consumer), 0);
    for frames in [1, 31, 137] {
        let (actual, full) = oracle.render(playback, frames);
        assert_eq!(actual, full);
        controller_python(
            locals,
            "transport.on_frame_render()\nassert b_on_window.publication_status() == 'pending'\nassert transport.residency.key_lock_status(0).pending",
        );
        assert_eq!(playback.drain(consumer), 0);
    }
    let clock = voice(&playback.callback.mixer).source_playback;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let old_window = voice(&playback.callback.mixer).sample.clone().unwrap();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let historical_adoption = voice(&playback.callback.mixer).stretch.adopted_request_id();
    assert!(
        voice(&playback.callback.mixer)
            .stretch
            .productive_history()
            .is_none()
    );
    let feedback_count = playback.callback.feedback.events.len();
    let ack = playback.frame;
    playback.callback.retirement.slots = usize::MAX;
    assert_eq!(playback.drain(consumer), 1, "B own addressed ON window ACK");
    assert!(
        voice(&playback.callback.mixer)
            .source_playback
            .matches_exact(&clock)
    );
    // A's used handle remained dirty through B's dry playback. The real ON
    // admission exchanges it with the lane's worker-warmed reserve; this is
    // resource admission, before B has supplied its own productive wet history.
    let on_native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    assert_ne!(on_native, 0);
    assert_ne!(on_native, native);
    let after = voice(&playback.callback.mixer);
    assert_eq!(after.generation, oracle.generation);
    let on_window = after.sample.as_ref().unwrap();
    assert!(on_window.same_source(&old_window));
    assert_ne!(on_window.window_revision(), old_window.window_revision());
    assert_eq!(
        (on_window.resident_start(), on_window.resident_end()),
        (LOOP_START, LOOP_END)
    );
    assert_eq!(on_window.samples.as_ref(), old_window.samples.as_ref());
    assert_eq!(
        on_window.residency.as_ref().unwrap().context,
        ResidentContext::KeyLockFiniteLoop
    );
    assert_eq!(after.stretch.pending_fifo_frames(), (0, 0));
    assert_eq!(after.stretch.adopted_request_id(), historical_adoption);
    assert!(after.stretch.productive_history().is_none());
    assert!(!after.stretch.wet_render_ready());
    assert!(!after.stretch.wet_output_ready());
    assert!(!after.stretch.wet_current_source_output_ready());
    let after_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    oracle.peer.assert_current(playback);
    assert_eq!(
        voice(&playback.callback.mixer).stretch.adopted_request_id(),
        oracle.historical_adoption
    );
    assert_eq!(playback.callback.feedback.events.len(), feedback_count);
    controller_python(
        locals,
        r#"
transport.on_frame_render()
assert b_on_window.publication_status() == 'accepted' and b_on_window.is_current()
feedback = transport.residency._mode_feedback(0)
assert feedback['request_id'] == b_on_window.key_lock_request_id
assert feedback['window_revision'] == b_on_window.window_revision
assert feedback['effective'] is False and feedback['ready'] is False
assert transport.residency.key_lock_status(0).pending
"#,
    );
    for (mixer, owner) in [playback.reference.as_mut(), playback.full_mix.as_mut()]
        .into_iter()
        .zip(owners)
    {
        let request = owner.next_key_lock_request(0).unwrap();
        mixer.set_pad_key_lock_request(
            0,
            true,
            request,
            InputPadBinding {
                placement: owner.binding_placement(0),
                resident: playback.source.resident_binding(),
                source_address: playback.source.source_address(),
                sample_count: playback.source.source_sample_count(),
                channels: 2,
                sample_rate_hz: RATE,
                authority_revision: owner.authority_revision(0),
                runtime_revision: 0,
                accepted: None,
            },
            1,
        );
        assert!(voice(mixer).source_playback.matches_exact(&clock));
    }
    playback.raw = Some(ControllerRawNative::new(0.75, false));
    oracle.render(playback, 13);
    let (actual, full) = oracle.native(playback, ack, true);
    assert_eq!(actual, full, "B own FullMix wet output");
    assert!(actual.iter().any(|sample| sample.abs() > 0.001));
    assert!(
        voice(&playback.callback.mixer)
            .stretch
            .wet_current_source_output_ready()
    );
    assert_ne!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        on_native
    );
    // The exact prepared handle, B ContentKey/generation, source-history binding
    // and PCM/FIFO proof above establish ownership. A's retained diagnostic
    // adoption number may equal this independent B worker's local request ID.
    assert!(
        voice(&playback.callback.mixer)
            .stretch
            .adopted_request_id()
            .is_some()
    );
    complete::feedback(locals, &mut playback.callback);
    controller_python(
        locals,
        r#"
status = transport.residency.key_lock_status(0)
assert status.requested and status.effective is True and not status.pending and status.error is None
feedback = transport.residency._mode_feedback(0)
assert feedback['request_id'] == b_on_window.key_lock_request_id
assert feedback['window_revision'] == b_on_window.window_revision
assert feedback['effective'] is True and feedback['ready'] is True
b_wet = audio.capture_prepared_source(0, b_version)
assert b_wet.same_source_request(b_off) and not b_wet.same_preparation_context(b_off)
assert not b_wet.same_source_request(a_source)
assert project.stem_cache[0] is None and controller.stem_residency_status(0) is None
assert audio.worker_call_count() == a_workers
"#,
    );
}

fn queue_pair(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    oracle: &BOracle,
    reference: &str,
    replacement: bool,
) {
    Python::attach(|py| {
        locals.bind(py).set_item("b_cache", reference).unwrap();
        locals
            .bind(py)
            .set_item("b_replacement", replacement)
            .unwrap();
    });
    controller_python(
        locals,
        r#"
b_pair_source = audio.capture_prepared_source(0, b_version)
assert b_pair_source.same_source_request(b_origin) and not b_pair_source.same_source_request(a_source)
b_pair_before = project.stem_cache[0].pair if project.stem_cache[0] is not None else None
b_entry = StemCacheEntry(source_version=b_version, cache_dir=b_cache,
    stems=expected_stem_files(b_cache), available=False)
assert 0 not in controller._fenced_pair_ids
assert controller._prepare_pair(0, b_entry, b_pair_source, components=False)
b_preparation = controller._pair_preparations.pending[0]
assert b_preparation.entry is b_entry and b_preparation.ticket is b_pair_source
assert b_preparation.content_id == b_content.instance_id and b_preparation.source_path == b_path
assert not b_preparation.residency_only
assert b_preparation.generation_intent is None and not b_preparation.cancelled.is_set()
assert b_preparation.own_source_version == b_version and b_preparation.artifact_source_version == b_version
"#,
    );
    for frames in [1, 31, 137] {
        oracle.render(playback, frames);
        assert!(consumer.is_empty());
    }
    wait_until(|| {
        controller_python(
            locals,
            "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
        );
        python_bool(
            locals,
            "0 not in controller._pair_preparations.pending and 0 not in controller._disk_selection_settlements and project.stem_cache[0] is not None and project.stem_cache[0].pair is not None and project.stem_cache[0].pair != b_pair_before",
        )
    });
    controller_python(
        locals,
        r#"
assert 0 not in controller._disk_selection_settlements
assert b_preparation.prepared is not None and not b_preparation.prepared.has_components()
pair_selection = controller._committed_residency._live_selections[0].disk.selection
assert pair_selection is not None and pair_selection.candidate
b_fresh_for_diagnostic = audio.capture_prepared_source(0, b_version)
b_registration_diagnostic = (pair_selection.ticket.publication_status(), pair_selection.ticket.is_current(),
    pair_selection.ticket.error(), b_pair_source.same_source_request(b_fresh_for_diagnostic),
    b_pair_source.same_preparation_context(b_fresh_for_diagnostic))
assert pair_selection.ticket.publication_status() == 'accepted', b_registration_diagnostic
assert pair_selection.ticket.is_current(), b_registration_diagnostic
assert pair_selection.ticket.error() is None, b_registration_diagnostic
assert b_registration_diagnostic[3] and b_registration_diagnostic[4], b_registration_diagnostic
del b_fresh_for_diagnostic
assert project.sample_paths[0] == b_path and project.pad_content[0] == b_content
assert project.stem_cache[0].source_version == b_version
assert audio.worker_call_count() == a_workers + (2 if b_replacement else 1)
assert audio.prepared_only_on_pool_threads()
assert project.pad_stem_mix_mode[0] == ('all_stems' if b_replacement else 'full_mix')
b_pair_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert b_pair_reopened.load_error is None and b_pair_reopened.project.stem_cache[0].pair == project.stem_cache[0].pair
assert b_pair_reopened.project.sample_paths[0] == b_path and b_pair_reopened.project.pad_content[0] == b_content
"#,
    );
    if !replacement {
        assert!(consumer.is_empty(), "disk-only B1 creates no Capture");
    }
}

fn adopt(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    oracle: &BOracle,
    name: &str,
) -> u64 {
    assert_eq!(playback.drain(consumer), 1, "own B component Capture");
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, name, "residency_status") == "rejected"
    });
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "pending"
    );
    oracle.render(playback, 31);
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let ack = playback.frame;
    assert_eq!(playback.drain(consumer), 1, "own B component Adopt");
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "accepted"
    );
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "transitioning"
    );
    let after = voice(&playback.callback.mixer);
    assert!(after.source_playback.matches_exact(&clock));
    assert_eq!(after.generation, oracle.generation);
    assert_eq!(after.stretch.native_state_address(), native);
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let after = after.stretch.productive_history().unwrap();
        assert_eq!(after.binding, history.binding);
        assert_eq!(after.next_position, history.next_position);
        assert_eq!(after.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(after.stretch.productive_history().is_none());
    }
    let after = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after.binding, dsp.binding);
    assert_eq!(after.next_position, dsp.next_position);
    assert_eq!(after.fed_output_frames, dsp.fed_output_frames);
    oracle.peer.assert_current(playback);
    complete::feedback(locals, &mut playback.callback);
    ack
}

fn probe(
    root: &Path,
    playback: &ControllerPlayback,
    locals: &Py<PyDict>,
    name: &str,
    components: &[SampleBuffer; 4],
    hash: u64,
) -> FinalReaderProbe {
    let actual = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert_eq!(actual.source_version_hash, hash);
    for (resident, expected) in actual.stems.iter().zip(components) {
        assert_eq!(
            (
                resident.resident_start(),
                resident.resident_end(),
                resident.frame_count()
            ),
            (LOOP_START, LOOP_END, 4096)
        );
        assert_eq!(
            resident.samples.as_ref(),
            &expected.samples[LOOP_START * 2..LOOP_END * 2]
        );
        assert!(!Arc::ptr_eq(&resident.samples, &expected.samples));
    }
    let disk = disk_json(locals, name);
    assert_complete_selection(root, &disk);
    FinalReaderProbe::new(root, &disk, actual)
}

fn public_b_unload(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
    locals: &Py<PyDict>,
    engine: &AudioEngine,
    oracle: &BOracle,
    current: &FinalReaderProbe,
) {
    prepare_public_unload_loader(locals);
    controller_python(
        locals,
        r#"
b_unload_peer = (project.sample_paths[215], project.pad_content[215])
assert b2.is_current() and b2.effective_mode_status() == 'effective'
loader.unload_sample(0)
assert project.sample_paths[0] is None and project.pad_content[0] is None and project.stem_cache[0] is None
assert project.pad_stem_mix_mode[0] == 'full_mix' and project.pad_key_lock[0] is None
assert session.active_sample_ids == {215}
assert not controller._committed_residency.contains(0) and controller.stem_residency_status(0) is None
assert ('stems', 0) not in controller._assets._assignments
assert unload_notifications == [0]
del b2, b2_live, b_preparation, pair_selection
persistence.flush()
"#,
    );
    current.assert_retained();
    let before = playback.callback.feedback.events.len();
    let stopped_feedback =
        ExpectedStoppedFeedback::source_command(&playback.callback.mixer, consumer, 0);
    crate::audio_engine::audio_stream::drain_parameter_messages(
        parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    assert!(parameters.is_empty());
    assert_eq!(
        playback.drain(consumer),
        4,
        "B own public Unload/defaults ACK"
    );
    assert!(consumer.is_empty());
    assert!(playback.callback.mixer.bank_for_measurement()[0].is_none());
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(playback.callback.feedback.events.len(), before + 1);
    stopped_feedback.assert_message(&playback.callback.feedback.events[before]);
    complete::feedback(locals, &mut playback.callback);
    let consumed = playback.source_frame;
    for frames in [1, 31, 137, 257, 511] {
        inactive(playback, &oracle.peer, frames);
        assert_eq!(playback.source_frame, consumed);
    }
    fifo_completion(
        playback,
        current,
        "B2 own Unload production FIFO completion",
    );
    wait_cleanup(engine, locals, current);
    controller_python(
        locals,
        r#"
loader.poll_loader_events()
controller.on_frame_render()
assert session.active_sample_ids == {215} and not session.paused_sample_ids
assert (project.sample_paths[215], project.pad_content[215]) == b_unload_peer
assert audio.worker_call_count() == a_workers + 2
assert not session.sample_load_errors and not session.stem_generation_errors
persistence.mark_dirty()
persistence.flush()
b_unloaded_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert b_unloaded_reopened.load_error is None
assert b_unloaded_reopened.project.sample_paths[0] is None and b_unloaded_reopened.project.pad_content[0] is None
assert b_unloaded_reopened.project.stem_cache[0] is None and b_unloaded_reopened.project.pad_stem_mix_mode[0] == 'full_mix'
assert b_unloaded_reopened.project.pad_key_lock[0] is None
assert (b_unloaded_reopened.project.sample_paths[215], b_unloaded_reopened.project.pad_content[215]) == b_unload_peer
loader.shut_down()
"#,
    );
}
