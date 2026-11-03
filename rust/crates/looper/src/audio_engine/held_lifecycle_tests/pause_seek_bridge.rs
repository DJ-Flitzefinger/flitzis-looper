//! Complete Loader sources through ordinary pause, resident seek and resume.

use super::controller_overlap::{script_value, settle, start};
use super::*;
use crate::audio_engine::productive_source_history::ProductiveSourceHistory;
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::source_reader::FrameRange;
use crate::messages::TransportContentEffect;

#[derive(Clone)]
struct PlaybackImage {
    lane: usize,
    generation: u64,
    pcm: usize,
    native: usize,
    paused: bool,
    playback: SourcePlayback,
    fifo: (usize, usize),
    pitch: Option<ProductiveSourceHistory>,
    dsp: Option<ProductiveSourceHistory>,
}

fn image(f: &mut Fixture, id: usize) -> PlaybackImage {
    let state = f.state();
    let (lane, voice) = state
        .mixer
        .voices
        .iter()
        .enumerate()
        .find(|(_, v)| v.is_playing_sample(id))
        .unwrap();
    PlaybackImage {
        lane,
        generation: voice.generation,
        pcm: voice.sample.as_ref().unwrap().samples.as_ptr() as usize,
        native: voice.stretch.native_state_address(),
        paused: voice.paused,
        playback: voice.source_playback,
        fifo: voice.stretch.pending_fifo_frames(),
        pitch: voice.stretch.productive_history(),
        dsp: state.mixer.dsp_source_history_for_test(id),
    }
}

fn same_owner(before: &PlaybackImage, after: &PlaybackImage) {
    assert_eq!(after.lane, before.lane);
    assert_eq!(after.pcm, before.pcm);
    assert_eq!(after.native, before.native);
}

fn same_history(before: Option<ProductiveSourceHistory>, after: Option<ProductiveSourceHistory>) {
    match (before, after) {
        (Some(before), Some(after)) => {
            assert_eq!(after.binding, before.binding);
            assert_eq!(after.next_position, before.next_position);
            assert_eq!(after.domain, before.domain);
            assert_eq!(after.fed_output_frames, before.fed_output_frames);
        }
        (None, None) => {}
        _ => panic!("pause/resume changed productive history"),
    }
}

fn frozen(before: &PlaybackImage, after: &PlaybackImage) {
    same_owner(before, after);
    assert_eq!(after.generation, before.generation);
    assert!(after.playback.matches_exact(&before.playback));
    assert_eq!(after.fifo, before.fifo);
    same_history(before.pitch, after.pitch);
    same_history(before.dsp, after.dsp);
}

fn source_image(f: &mut Fixture, id: usize) -> (usize, String) {
    let source = f.state().mixer.bank_for_measurement()[id].as_ref().unwrap();
    assert_eq!(source.frame_count(), 3 * RATE as usize);
    assert_eq!(
        (source.resident_start(), source.resident_end()),
        (0, source.frame_count())
    );
    assert_eq!(source.samples.len(), 2 * source.frame_count());
    let mut hash = Sha256::new();
    for sample in source.samples.iter() {
        hash.update(sample.to_le_bytes());
    }
    (
        source.samples.as_ptr() as usize,
        format!("{:x}", hash.finalize()),
    )
}

fn render_with_peer(
    f: &mut Fixture,
    frames: usize,
    paused: bool,
    paused_expected: Option<&PlaybackImage>,
) -> Vec<f32> {
    let peer = image(f, 1);
    assert!(!peer.paused);
    let mut expected = peer.playback;
    assert!(expected.advance(frames));
    let target = image(f, 0);
    assert_eq!(target.paused, paused);
    let pcm = f.render(frames);
    let after = image(f, 1);
    same_owner(&peer, &after);
    assert_eq!(after.generation, peer.generation);
    assert_eq!(after.playback.position(), expected.position());
    assert_eq!(after.playback.tempo_ratio(), 1.0);
    if paused {
        frozen(paused_expected.unwrap_or(&target), &image(f, 0));
        assert_eq!(f.state().pad_peaks[0], 0.0);
    }
    assert!(pcm.iter().all(|v| v.is_finite()));
    if frames >= 128 {
        assert!(
            f.state().pad_peaks[1] > 1e-6,
            "actual post-DSP peer contribution"
        );
        assert!(pcm.iter().any(|v| v.abs() > 1e-6));
    }
    pcm
}

fn remember_admission(f: &Fixture, paused: bool) {
    f.script(&format!(
        "probe.transport.playback.{}_pad(0)\nprobe.bridge_owner = probe.transport.playback._pause_intent._owners[probe.project.pad_content[0].instance_id]\nprobe.bridge_admission = probe.bridge_owner.admission\nassert probe.bridge_owner.pending and probe.bridge_admission is not None\nassert probe.bridge_admission.requested_paused is {}\nassert probe.bridge_admission.current_control_slot() == 0\nassert probe.bridge_admission.content_lifetime == probe._audio.current_input_runtime_pad_binding(0).metadata()['content_lifetime']\nprobe.bridge_before = probe.bridge_admission.current_observation()\nassert probe.bridge_before is not None and probe.bridge_before.active\nassert probe.bridge_before.ordinary_paused is {}\nassert not probe.bridge_before.settles_admission(probe.bridge_admission)\nassert probe.session.paused_sample_ids == {}",
        if paused { "pause" } else { "resume" },
        if paused { "True" } else { "False" },
        if paused { "False" } else { "True" },
        if paused { "set()" } else { "{0}" },
    ));
}

fn drain_effect(f: &mut Fixture, name: &str) -> serde_json::Value {
    let frame = f.state().transport.output_frame();
    let (revision, source, sequence, lifetime) = match f.state().consumer_in.peek().unwrap() {
        ControlMessage::TransportForPlacement {
            placement,
            transport_revision,
            source_epoch,
            original_sequence,
            effect,
        } => {
            match (name, effect) {
                ("pause", TransportContentEffect::Pause)
                | ("resume", TransportContentEffect::Resume) => {}
                _ => panic!("unexpected ordinary command"),
            }
            (
                *transport_revision,
                *source_epoch,
                *original_sequence,
                placement.content_key().unwrap().lifetime(),
            )
        }
        _ => panic!("public operation did not enqueue its source-qualified command"),
    };
    assert_eq!(f.drain(), 1);
    assert_eq!(f.state().transport.output_frame(), frame);
    f.script(&format!(
        "probe.bridge_effect = probe._audio._current_ordinary_transport_observation(0)\nassert probe.bridge_effect is not None\nassert probe.bridge_effect.current_control_slot() == 0\nassert probe.bridge_effect.transport_revision == {revision}\nassert probe.bridge_effect.content_lifetime == {lifetime}\nassert probe.bridge_effect.requested_effect == '{name}'\nassert probe.bridge_effect.outcome == 'applied' and probe.bridge_effect.active\nassert probe.project.selected_pad == 1\nassert probe._audio.input_selected_projection()[1] == 1"
    ));
    f.script("assert probe.bridge_effect.matches_admission(probe.bridge_admission)\nassert probe.bridge_effect.settles_admission(probe.bridge_admission)\nassert probe.bridge_effect.observation_sequence > probe.bridge_before.observation_sequence\nassert probe.bridge_effect.ordinary_paused is probe.bridge_admission.requested_paused\nprobe.transport.playback.poll_pause_observations()\nassert not probe.bridge_owner.pending and probe.bridge_owner.admission is None");
    serde_json::json!({"effect": name, "callback_frame": frame, "revision": revision,
        "source_epoch": source, "original_sequence": sequence, "content_lifetime": lifetime})
}

#[test]
#[ignore = "explicit complete-Loader pause/seek/resume bridge; no device or latency claim"]
fn actual_loader_pause_seek_resume_preserves_peer() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut f = Fixture::new(root, false);
    Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/controller_overlap_probe.py"
            )))
            .unwrap(),
            c"controller_overlap_probe.py",
            c"controller_overlap_probe",
        )
        .unwrap();
        module
            .call_method1(
                "write_source",
                (root.join("source.wav").to_string_lossy().as_ref(), RATE, 3),
            )
            .unwrap();
    });
    let path = root.join("source.wav");
    f.load(&path);
    f.script(&format!("probe.load(1, {:?})", path.to_string_lossy()));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !script_value(&f, "probe.poll_load(1)") {
        f.drain();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    f.drain();
    let sources = [0, 1].map(|id| source_image(&mut f, id));
    f.script("probe.bridge_sources = tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(2))\nprobe.bridge_markers = tuple(probe.transport.loop.requested_region(i) for i in range(2))");
    start(&mut f, 0, true);
    settle(&mut f, &[0]);
    start(&mut f, 1, true);
    settle(&mut f, &[0, 1]);
    f.script("probe.select_pad(1)\nprobe.input_mapping.on_frame_render()\nassert probe.session.active_sample_ids == {0, 1}");
    render_with_peer(&mut f, 4096, false, None);
    let before = image(&mut f, 0);
    assert!(before.dsp.is_some());
    let peer_birth = image(&mut f, 1);
    let markers = [0, 1].map(|id| f.state().mixer.loop_region_frames(id));
    assert_ne!(before.playback.position().frame, 36_000);
    assert!(markers[0].0 < 36_000 && markers[0].1.unwrap_or(3 * RATE as usize) > 36_000);
    remember_admission(&f, true);
    frozen(&before, &image(&mut f, 0));
    assert!(!image(&mut f, 0).paused);
    let pause = drain_effect(&mut f, "pause");
    assert!(image(&mut f, 0).paused);
    frozen(&before, &image(&mut f, 0));
    let mut pcm_hash = Sha256::new();
    for frames in [128, 385, 512] {
        for sample in render_with_peer(&mut f, frames, true, None) {
            pcm_hash.update(sample.to_le_bytes());
        }
    }
    f.call("poll_feedback");
    f.script("probe.transport.on_frame_render()\nassert probe.session.paused_sample_ids == {0}\nprobe.bridge_old_playhead = probe.session.pad_playhead_s[0]\nprobe.transport.playback.seek_pad(0, 0.75)\nprobe.bridge_seek = probe.transport.residency._pending[0].ticket\nassert probe.bridge_seek is not None and probe.transport.residency._pending[0].seek_only\nassert probe.bridge_seek.key_lock_request_id is None\nassert probe.bridge_seek.effective_seek_seconds is None\nassert probe.session.pad_playhead_s[0] == probe.bridge_old_playhead");
    // Complete Loader sources use the actual resident capture/adoption route.
    frozen(&before, &image(&mut f, 0));
    assert!(image(&mut f, 0).paused);
    let capture_frame = f.state().transport.output_frame();
    let (seek_binding, seek_publication) = match f.state().consumer_in.peek().unwrap() {
        ControlMessage::CaptureResidentSeek(capture) => {
            assert_eq!(capture.id, 0);
            assert_eq!(capture.position_s, 0.75);
            (capture.binding, capture.publication.clone())
        }
        _ => panic!("public resident Seek must first capture its actual voice"),
    };
    // Handle only the real queued capture through the production callback handler.
    // A full drain could also consume a fast worker's relocation before observation.
    {
        let state = f.state();
        let message = state.consumer_in.pop().unwrap();
        process_control_message(
            message,
            &mut state.scheduler,
            capture_frame,
            &mut state.trigger_quantization,
            &mut state.transport,
            &mut state.mixer,
            &mut state.producer_out,
            &mut state.retired_buffers,
        );
    }
    frozen(&before, &image(&mut f, 0));
    assert!(image(&mut f, 0).paused);
    f.script("assert probe.bridge_seek.publication_status() != 'accepted'\nassert probe.bridge_seek.effective_seek_seconds is None");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(
        f.state().consumer_in.peek(),
        Ok(ControlMessage::RelocateResident(_))
    ) {
        f.script("assert probe.bridge_seek.publication_status() in {'preparing', 'pending'}\nassert probe.bridge_seek.effective_seek_seconds is None");
        render_with_peer(&mut f, 512, true, None);
        assert!(
            Instant::now() < deadline,
            "real resident Seek never prepared"
        );
        thread::sleep(Duration::from_millis(1));
    }
    frozen(&before, &image(&mut f, 0));
    f.script("assert probe.bridge_seek.publication_status() == 'pending'");
    match f.state().consumer_in.peek().unwrap() {
        ControlMessage::RelocateResident(transaction) => {
            assert_eq!(transaction.id, 0);
            assert_eq!(transaction.binding, seek_binding);
            assert_eq!(transaction.intent.seek_position_s, Some(0.75));
            assert!(Arc::ptr_eq(
                &transaction.publication.epoch,
                &seek_publication.epoch
            ));
            assert_eq!(transaction.publication.expected, seek_publication.expected);
            let pin = transaction.seek_pin.as_ref().unwrap();
            assert_eq!(pin.voice_index, before.lane);
            assert_eq!(pin.generation, before.generation);
            assert_eq!(pin.sample.samples.as_ptr() as usize, before.pcm);
            assert_eq!(pin.loop_region.start, markers[0].0);
            assert_eq!(
                pin.loop_region.end,
                markers[0].1.unwrap_or(3 * RATE as usize)
            );
            assert!(!pin.stopped);
        }
        _ => unreachable!(),
    }
    let adoption_frame = f.state().transport.output_frame();
    assert_eq!(f.drain(), 1, "only own resident relocation adopts the Seek");
    f.script("assert probe.bridge_seek.publication_status() == 'accepted'\nassert probe.bridge_seek.is_current() and probe.bridge_seek.error() is None\nassert probe.bridge_seek.effective_seek_seconds == 0.75\nassert probe.transport.residency._pending[0].ticket is probe.bridge_seek\nprobe.transport.on_frame_render()\nassert 0 not in probe.transport.residency._pending\nassert probe.session.pad_playhead_s[0] == 0.75\nassert probe.session.paused_sample_ids == {0}\nassert probe.project.selected_pad == 1\nassert probe._audio.input_selected_projection()[1] == 1");
    let window_revision: u64 = Python::attach(|py| {
        f.probe
            .bind(py)
            .getattr("bridge_seek")
            .unwrap()
            .getattr("window_revision")
            .unwrap()
            .extract()
            .unwrap()
    });
    let seek = serde_json::json!({"effect": "resident_seek", "capture_frame": capture_frame,
        "adoption_frame": adoption_frame, "window_revision": window_revision,
        "source_address": seek_binding.source_address, "publication_expected": seek_publication.expected,
        "effective_seek_seconds": 0.75, "accepted_current": true});
    let sought = image(&mut f, 0);
    same_owner(&before, &sought);
    assert!(sought.paused);
    assert_eq!(sought.generation, before.generation + 1);
    assert_eq!(
        (
            sought.playback.position().frame,
            sought.playback.position().fraction
        ),
        (36_000, 0.0)
    );
    assert_eq!(sought.playback.tempo_ratio(), before.playback.tempo_ratio());
    assert_eq!(sought.fifo, (0, 0));
    assert!(sought.pitch.is_none() && sought.dsp.is_none());
    // Seek clears the configured domain. The first paused render restores the
    // independently retained physical geometry without advancing the cursor.
    assert!(
        f.state().mixer.voices[sought.lane]
            .source_timing
            .accepted
            .is_none()
    );
    let mut normalized = sought.clone();
    assert!(normalized.playback.configure(
        3 * RATE as usize,
        FrameRange {
            start: markers[0].0,
            end: markers[0].1.unwrap_or(3 * RATE as usize)
        },
    ));
    assert_eq!(normalized.playback.position(), sought.playback.position());
    render_with_peer(&mut f, 512, true, Some(&normalized));
    remember_admission(&f, false);
    frozen(&normalized, &image(&mut f, 0));
    assert!(image(&mut f, 0).paused);
    let resume = drain_effect(&mut f, "resume");
    frozen(&normalized, &image(&mut f, 0));
    assert!(!image(&mut f, 0).paused);
    f.script("assert not probe.session.paused_sample_ids\nassert probe._audio.current_explicit_playback_target().current_control_slot() == 0");
    let mut expected = normalized.playback;
    for frames in [1, 127, 385, 512] {
        assert!(expected.advance(frames));
        for sample in render_with_peer(&mut f, frames, false, None) {
            pcm_hash.update(sample.to_le_bytes());
        }
        let after = image(&mut f, 0);
        same_owner(&sought, &after);
        assert_eq!(after.generation, sought.generation);
        assert_eq!(after.playback.position(), expected.position());
        assert!(
            f.state().pad_peaks[0] > 1e-6,
            "resumed target contributes actual PCM"
        );
    }
    let peer_end = image(&mut f, 1);
    same_owner(&peer_birth, &peer_end);
    assert_eq!(peer_end.generation, peer_birth.generation);
    for id in [0, 1] {
        assert_eq!(source_image(&mut f, id), sources[id]);
        assert_eq!(f.state().mixer.loop_region_frames(id), markers[id]);
    }
    f.call("poll_feedback");
    f.script("probe.transport.on_frame_render()\nprobe.input_mapping.on_frame_render()\nassert probe.session.active_sample_ids == {0, 1}\nassert not probe.session.paused_sample_ids\nassert probe.project.selected_pad == 1\nassert probe._audio.input_selected_projection()[1] == 1\nassert tuple(probe.transport.loop.requested_region(i) for i in range(2)) == probe.bridge_markers\nassert tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(2)) == probe.bridge_sources\nassert all(probe._audio.current_input_runtime_pad_binding(i).is_current() for i in range(2))\nprobe.transport.playback.stop_all_pads()");
    f.drain();
    f.render(512);
    f.call("poll_feedback");
    f.script("assert not probe.session.active_sample_ids");
    let joined = Python::attach(|py| {
        let mut engine = f.engine.borrow_mut(py);
        engine.shut_down().unwrap();
        engine.cold_jobs.counts_for_test()
    });
    assert_eq!(joined, (0, 0, 0));
    f.close_callback();
    eprintln!(
        "pause_seek_bridge_raw {}",
        serde_json::json!({"rate": RATE, "channels": 2,
        "frames_per_source": 3 * RATE, "sources": sources, "effects": [pause, seek, resume],
        "seek_frame": 36_000, "generation_before": before.generation, "generation_after_seek": sought.generation,
        "peer_generation": peer_birth.generation, "peer_post_dsp_contribution": true,
        "mixed_pcm_sha256": format!("{:x}", pcm_hash.finalize()), "joined_counts": joined,
        "callback_retired": true, "limits": "One software callback bridge; no device, latency, hearing, NN, quality or full-matrix acceptance"})
    );
}
