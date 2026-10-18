//! HC13/15: actual Pair rejection/cancel and UI retarget/global Hold release.
//! The reference owns independent Native DSP history; neither fixture runs CPAL.

use super::*;
use crate::audio_engine::content_placement::{ContentKey, SlotSnapshot};
use crate::audio_engine::hold_action::NativePauseEffect;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::productive_source_history::ProductiveSourceHistory;
use crate::messages::SampleBuffer;
use std::sync::atomic::AtomicU64;

const JOIN_ROOT: &str = "FLITZI_R2_JOIN_ROOT";
const JOIN_CHILD: &str = "FLITZI_R2_JOIN_CHILD";

#[derive(Debug, PartialEq)]
struct NativeImage {
    requests: [RequestImage; 2],
    slots: [SlotSnapshot; 2],
    intents: [u64; 2],
    launches: [u64; 2],
    selected: (u64, Option<usize>),
}

impl NativeImage {
    fn assert_before(&self, before: &Self, reserved: bool) {
        assert_eq!(self.requests, before.requests);
        assert_eq!(self.intents, before.intents);
        assert_eq!(self.launches, before.launches);
        for (actual, original) in self.slots.iter().zip(before.slots) {
            assert!(
                actual.same_projection(original),
                "Pair changed slot/content/binding epoch"
            );
            // Actual reserve and release each publish one seqlock marker, not a layout move.
            assert_eq!(
                actual.layout_revision(),
                original.layout_revision() + if reserved { 2 } else { 4 }
            );
        }
        assert_eq!(
            self.selected,
            (
                before.selected.0,
                if reserved { None } else { before.selected.1 }
            )
        );
    }
}

struct LaneImage {
    lane: usize,
    key: ContentKey,
    generation: u64,
    lifetime: Arc<AtomicU64>,
    lifetime_value: u64,
    epoch: Arc<AtomicU64>,
    epoch_value: u64,
    native: usize,
    fifo: (usize, usize),
    position: FractionalSourcePosition,
    sample: SampleBuffer,
    accepted: Option<crate::audio_engine::constant_timing::AcceptedTimingProjection>,
    legacy_period: Option<f64>,
    legacy_origin: f64,
    history: Option<ProductiveSourceHistory>,
    dsp: Option<ProductiveSourceHistory>,
}

impl LaneImage {
    fn capture(mixer: &RtMixer, id: usize) -> Self {
        let lane = mixer
            .voices
            .iter()
            .position(|v| v.is_playing_sample(id))
            .unwrap();
        let voice = &mixer.voices[lane];
        let dsp = mixer.dsp_source_history_for_test(id);
        assert!(dsp.is_some_and(|history| history.fed_output_frames > 0));
        Self {
            lane,
            key: voice
                .source_admission
                .as_ref()
                .unwrap()
                .content_key()
                .unwrap(),
            generation: voice.generation,
            lifetime: voice.history_lifetime.clone(),
            lifetime_value: voice.history_lifetime.load(Ordering::Acquire),
            epoch: voice.stretch.source_epoch_owner().clone(),
            epoch_value: voice.stretch.source_epoch_owner().load(Ordering::Acquire),
            native: voice.stretch.native_state_address(),
            fifo: voice.stretch.pending_fifo_frames(),
            position: voice.source_playback.position(),
            sample: voice.sample.as_ref().unwrap().clone(),
            accepted: voice.source_timing.accepted,
            legacy_period: voice.source_timing.legacy_period_seconds,
            legacy_origin: voice.source_timing.legacy_origin_frame,
            history: voice.stretch.productive_history(),
            dsp,
        }
    }

    fn assert_retained(&self, mixer: &RtMixer, id: usize, resumed: bool) {
        let voice = &mixer.voices[self.lane];
        assert!(voice.active);
        assert_eq!(voice.sample_id, id);
        assert_eq!(voice.generation, self.generation);
        assert_eq!(
            voice.source_admission.as_ref().unwrap().content_key(),
            Some(self.key)
        );
        assert!(Arc::ptr_eq(&voice.history_lifetime, &self.lifetime));
        assert_eq!(
            voice.history_lifetime.load(Ordering::Acquire),
            self.lifetime_value
        );
        assert!(Arc::ptr_eq(voice.stretch.source_epoch_owner(), &self.epoch));
        assert_eq!(
            voice.stretch.source_epoch_owner().load(Ordering::Acquire),
            self.epoch_value + u64::from(resumed)
        );
        assert_eq!(voice.paused, !resumed);
        assert_eq!(voice.stretch.native_state_address(), self.native);
        assert_eq!(voice.stretch.pending_fifo_frames(), self.fifo);
        assert_eq!(voice.source_playback.position(), self.position);
        assert!(voice.sample.as_ref().unwrap().same_window(&self.sample));
        assert!(Arc::ptr_eq(
            &voice.sample.as_ref().unwrap().samples,
            &self.sample.samples
        ));
        assert_eq!(voice.source_timing.accepted, self.accepted);
        assert_eq!(
            voice.source_timing.legacy_period_seconds,
            self.legacy_period
        );
        assert_eq!(voice.source_timing.legacy_origin_frame, self.legacy_origin);
        assert_history(voice.stretch.productive_history(), self.history);
        assert_history(mixer.dsp_source_history_for_test(id), self.dsp);
    }
}

fn assert_history(
    actual: Option<ProductiveSourceHistory>,
    expected: Option<ProductiveSourceHistory>,
) {
    match (actual, expected) {
        (Some(actual), Some(expected)) => {
            assert_eq!(actual.binding, expected.binding);
            assert_eq!(actual.next_position, expected.next_position);
            assert_eq!(actual.domain, expected.domain);
            assert_eq!(actual.fed_output_frames, expected.fed_output_frames);
        }
        (None, None) => {}
        _ => panic!("original productive source/filter history was replaced"),
    }
}

impl Fixture {
    pub(super) fn load_join_slot(&mut self, slot: usize, path: &Path) {
        Python::attach(|py| {
            self.probe
                .call_method1(py, "load", (slot, path.to_string_lossy().as_ref()))
                .unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.drain();
            let ready = Python::attach(|py| {
                self.probe
                    .call_method1(py, "poll_load", (slot,))
                    .unwrap()
                    .extract::<bool>(py)
                    .unwrap()
            });
            if ready {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "public Cold load lacked its own ACK"
            );
            thread::sleep(Duration::from_millis(1));
        }
        self.drain();
        assert!(self.state().mixer.bank_for_measurement()[slot].is_some());
    }

    pub(super) fn start_join_slot(&mut self, slot: usize) {
        Python::attach(|py| {
            self.probe.call_method1(py, "start", (slot,)).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.script(&format!("probe.transport.residency.poll()\nassert probe.transport.residency.status({slot})[1] is None"));
            self.drain();
            if self
                .state()
                .mixer
                .voices
                .iter()
                .any(|v| v.is_playing_sample(slot))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "ordinary start lacked prepared Native adoption"
            );
            thread::sleep(Duration::from_millis(1));
        }
        self.script(&format!("probe.transport.residency.poll()\nassert probe.transport.residency.status({slot}) == (None, None)"));
        let output = self.render(37);
        assert!(output.iter().all(|v| v.is_finite()));
        assert!(output.iter().any(|v| v.abs() > 1e-6));
        self.call("poll_feedback");
    }

    fn join_request_image(&self, slot: usize) -> RequestImage {
        Python::attach(|py| {
            let engine = self.engine.borrow(py);
            let ownership = &engine.input_runtime_ownership;
            let key = ownership
                .slot_snapshot(slot)
                .unwrap()
                .content_key()
                .unwrap();
            let requests = engine.pad_request_ids.lock().unwrap();
            let cache = engine.sample_cache.lock().unwrap();
            let sample = cache[slot].as_ref().unwrap();
            let digests = engine.loaded_source_digests.lock().unwrap();
            let leases = engine.cold_leases.lock().unwrap();
            let lease = leases[slot].as_ref().unwrap();
            let loading = engine.loading_sample_ids.lock().unwrap();
            assert!(!lease.assignment_retired());
            RequestImage {
                request: requests[slot],
                prepared_epoch: engine.prepared_source_epochs[slot].load(Ordering::Acquire),
                logical_request: ownership.key_source_request_incarnation(key).unwrap(),
                authority: ownership.authority_revision(slot),
                cache: (
                    sample.samples.as_ptr() as usize,
                    sample.samples.len(),
                    sample.channels,
                ),
                digest: digests[slot].clone().unwrap(),
                lease: (
                    lease.assignment_id(),
                    lease.assignment_retired(),
                    lease.original_path.clone(),
                    lease.cache_path.clone(),
                ),
                lease_generation: engine.cold_lease_generations[slot].load(Ordering::Acquire),
                cold_loading: engine.cold_loading[slot].load(Ordering::Acquire),
                loading: loading.contains(&slot),
            }
        })
    }

    fn join_native_image(&self) -> NativeImage {
        let requests = [self.join_request_image(0), self.join_request_image(1)];
        Python::attach(|py| {
            let engine = self.engine.borrow(py);
            let ownership = &engine.input_runtime_ownership;
            let slots = [
                ownership.slot_snapshot(0).unwrap(),
                ownership.slot_snapshot(1).unwrap(),
            ];
            NativeImage {
                requests,
                slots,
                intents: slots.map(|slot| {
                    ownership
                        .transport_intent_for_cell(slot.cell())
                        .load(Ordering::Acquire)
                }),
                launches: [ownership.launch_revision(0), ownership.launch_revision(1)],
                selected: engine
                    .input_runtime
                    .as_ref()
                    .unwrap()
                    .selected_projection()
                    .unwrap(),
            }
        })
    }

    fn join_ready_view(&mut self) {
        self.call("setup_join_ui");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let ready = Python::attach(|py| {
                self.probe
                    .call_method0(py, "prime_join_view")
                    .unwrap()
                    .extract::<bool>(py)
                    .unwrap()
            });
            if ready {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "real Native waveform worker did not become ready"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn reject_and_cancel_join(&mut self, before: &NativeImage) {
        let pressure = Python::attach(|py| {
            let mut engine = self.engine.borrow_mut(py);
            let mut count = 0;
            while engine.ping().is_ok() {
                count += 1;
            }
            count
        });
        assert_eq!(pressure, 8);
        self.call("begin_rejected_join");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let ready = Python::attach(|py| {
                self.probe
                    .call_method0(py, "poll_rejected_join")
                    .unwrap()
                    .extract::<bool>(py)
                    .unwrap()
            });
            if ready {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "public Pair did not reach actual queue-full preclaim failure"
            );
            thread::sleep(Duration::from_millis(1));
        }
        self.join_native_image().assert_before(before, true);
        self.call("cancel_rejected_join");
        self.join_native_image().assert_before(before, false);
        Python::attach(|py| {
            let engine = self.engine.borrow(py);
            assert_eq!(engine.input_runtime_ownership.pair_reservation(0), 0);
            assert_eq!(engine.input_runtime_ownership.pair_reservation(1), 0);
        });
        assert_eq!(self.drain(), pressure);
        self.call("poll_feedback");
    }
}

fn hold_effect(hold: &Py<HoldActionTicket>) -> NativePauseEffect {
    Python::attach(|py| {
        let ticket = hold.borrow(py);
        assert_eq!(ticket.action.state(), HoldActionState::PausedOwned);
        let effect = ticket.action.effect().unwrap();
        assert_ne!(effect.changed_mask, 0);
        effect
    })
}

fn join_profile(root: &Path, input: &Path, retarget: bool) -> Vec<f32> {
    fs::create_dir(root).unwrap();
    let mut fixture = Fixture::new(root, false);
    fixture.load_join_slot(0, input);
    fixture.load_join_slot(1, input);
    fixture.start_join_slot(0);
    fixture.start_join_slot(1);
    fixture.script("probe.transport.playback.pause_pad(1)");
    fixture.drain();
    fixture.join_ready_view();
    let hold = Python::attach(|py| {
        fixture
            .probe
            .call_method0(py, "press_join")
            .unwrap()
            .extract::<Py<HoldActionTicket>>(py)
            .unwrap()
    });
    assert_eq!(fixture.drain(), 1);
    fixture.call("observe_owned");
    fixture.call("remember_join_before");
    let effect = hold_effect(&hold);
    let a = LaneImage::capture(&fixture.state().mixer, 0);
    let b = LaneImage::capture(&fixture.state().mixer, 1);
    assert_eq!(effect.key, a.key);
    assert_ne!(a.key, b.key);
    assert_eq!(effect.changed_mask, 1 << a.lane);
    assert_eq!(effect.cohort_mask, 1 << a.lane);
    a.assert_retained(&fixture.state().mixer, 0, false);
    b.assert_retained(&fixture.state().mixer, 1, false);
    let before = fixture.join_native_image();
    if retarget {
        fixture.reject_and_cancel_join(&before);
        assert_eq!(hold_effect(&hold), effect);
        a.assert_retained(&fixture.state().mixer, 0, false);
        b.assert_retained(&fixture.state().mixer, 1, false);
        fixture.call("retarget_join");
        fixture.call("outside_join_tick");
        a.assert_retained(&fixture.state().mixer, 0, false);
        b.assert_retained(&fixture.state().mixer, 1, false);
    } else {
        fixture.call("release");
    }
    assert_eq!(
        fixture.drain(),
        1,
        "original H must admit exactly one matching release"
    );
    a.assert_retained(&fixture.state().mixer, 0, true);
    b.assert_retained(&fixture.state().mixer, 1, false);
    if retarget {
        fixture.call("outside_join_tick");
        fixture.call("assert_join_released");
        for _ in 0..3 {
            fixture.call("outside_join_tick");
            assert_eq!(
                fixture.drain(),
                0,
                "global tick duplicated the original release"
            );
        }
    } else {
        fixture.call("poll_terminal");
    }
    Python::attach(|py| {
        let ticket = hold.borrow(py);
        assert_eq!(ticket.action.state(), HoldActionState::Resumed);
        assert_eq!(ticket.action.effect(), Some(effect));
    });
    a.assert_retained(&fixture.state().mixer, 0, true);
    b.assert_retained(&fixture.state().mixer, 1, false);
    let output = fixture.render(96);
    assert!(output.iter().all(|sample| sample.is_finite()));
    assert!(output.iter().any(|sample| sample.abs() > 1e-6));
    fixture.close_callback();
    fixture.call("close");
    output
}

fn join_child(root: &Path) {
    // A real worker-backed view requires >4096 frames. Keep the old four inputs intact.
    let input = write_input(root);
    let mut bytes = fs::read(&input).unwrap();
    let body = bytes[44..].to_vec();
    for _ in 0..3 {
        bytes.extend_from_slice(&body);
    }
    let size = (bytes.len() - 44) as u32;
    bytes[4..8].copy_from_slice(&(36 + size).to_le_bytes());
    bytes[40..44].copy_from_slice(&size.to_le_bytes());
    fs::write(&input, bytes).unwrap();
    // Sequential independently initialized fixtures prevent shared cwd/material state.
    let reference = join_profile(&root.join("reference"), &input, false);
    let actual = join_profile(&root.join("retarget"), &input, true);
    assert_eq!(
        actual, reference,
        "rejected Pair/real UI retarget changed continuing PCM"
    );
}

#[test]
fn public_rejected_pair_then_editor_retarget_releases_original_hold() {
    if std::env::var_os(JOIN_CHILD).is_some() {
        join_child(&PathBuf::from(std::env::var_os(JOIN_ROOT).unwrap()));
        eprintln!(
            "R2 actual Pair rejection/UI retarget child PID={}",
            std::process::id()
        );
        return;
    }
    let parent = std::env::var_os(JOIN_ROOT).map(PathBuf::from).unwrap_or_else(|| {
        let temporary = std::env::temp_dir().join("codex");
        fs::create_dir_all(&temporary).unwrap();
        let owned = tempfile::Builder::new().prefix("r2-retarget-reject-").tempdir_in(temporary).unwrap().keep();
        fs::write(owned.join("OWNER.md"), "Owner: device-free R2 real Pair rejection/UI retarget test. Reproducible outputs; preserve results before cleanup.\n").unwrap();
        owned
    });
    assert!(parent.is_dir() && parent.join("OWNER.md").is_file());
    let root = tempfile::Builder::new()
        .prefix("retarget-")
        .tempdir_in(parent)
        .unwrap()
        .keep();
    fs::write(
        root.join("OWNER.md"),
        "Owner: device-free R2 real Pair/UI join child; no App/device/private input.\n",
    )
    .unwrap();
    let (_, module) = module_path!().split_once("::").unwrap();
    let test =
        format!("{module}::public_rejected_pair_then_editor_retarget_releases_original_hold");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test, "--nocapture", "--test-threads=1"])
        .env(JOIN_CHILD, "1")
        .env(JOIN_ROOT, root.as_os_str())
        .current_dir(&root)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprint!("{stderr}");
    assert!(
        output.status.success(),
        "actual Pair/UI join failed:\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains("1 passed; 0 failed"));
    assert!(stderr.contains("R2 actual Pair rejection/UI retarget child PID="));
}
