//! Fresh-process App/ASIO software integration. Never opens a physical device.

use super::asio_output::test_driver::{Factory, TickObservation};
use super::*;
use pyo3::types::{PyDict, PyModule};
use std::cell::Cell;
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../src/tests/flitzis_looper/controller/audio_output_probe.py"
));
const SLOTS: usize = 1024;
const PERIOD: Duration = Duration::from_millis(1);

#[test]
#[ignore = "fresh-process actual assigned Pair Apply/Auto/rollback; no device/model or WET profile"]
fn pair_crossrate_actual_assigned_app_apply_auto_and_rollback() {
    let root = PathBuf::from(
        std::env::var_os("FLITZI_AUDIO_OUTPUT_PROFILE_ROOT").expect("owned root required"),
    );
    assert!(!root.exists());
    fs::create_dir(&root).unwrap();
    let readers_ended = Arc::new(AtomicBool::new(false));
    let _cwd = DirectoryGuard {
        original: std::env::current_dir().unwrap(),
        readers_ended,
    };
    std::env::set_current_dir(&root).unwrap();
    let factory = Factory::new().unwrap();
    let mut fixture = Fixture::new(&root, factory.clone());
    fixture.ready();
    fixture.seed_leases.clear();
    fixture.call("install_legacy_intent");
    fixture.ready();
    fixture.call("install_assigned_pair");
    fixture.settle("assigned-pair", |f, _| f.predicate("assigned_pair_ready"));
    fixture.call("_save_assigned_pair_baseline");
    let identity = |f: &Fixture| {
        Python::attach(|py| {
            f.probe()
                .call_method0(py, "verify_assigned_pair")
                .unwrap()
                .extract::<String>(py)
                .unwrap()
        })
    };
    let initial = identity(&fixture);
    let before = fixture.sources();
    fixture.call("_arm_restore_scope_capture");
    fixture.apply(96_000);
    assert!(
        fixture.predicate("output_change_succeeded"),
        "{}",
        fixture.status()
    );
    assert_eq!(fixture.sources()[0].rate, 96_000, "{}", fixture.status());
    let converted = identity(&fixture);
    assert_ne!(initial, converted);
    fresh_sources(&before, &fixture.sources(), 96_000);
    fixture.call("_verify_stale_restore_scope");
    fixture.call("apply_auto");
    fixture.ready();
    assert!(
        fixture.predicate("output_change_succeeded"),
        "{}",
        fixture.status()
    );
    assert_eq!(fixture.sources()[0].rate, 96_000, "{}", fixture.status());
    assert_eq!(identity(&fixture), converted);
    fixture.call("arm_pair_commit_failure");
    fixture.apply(48_000);
    assert_eq!(identity(&fixture), converted);
    assert!(
        fixture
            .status()
            .contains("Injected failure after actual Pair selection commit")
    );
    assert_eq!(fixture.sources()[0].rate, 96_000);
    fixture.apply(48_000);
    assert!(
        fixture.predicate("output_change_succeeded"),
        "{}",
        fixture.status()
    );
    assert_eq!(fixture.sources()[0].rate, 48_000, "{}", fixture.status());
    assert_eq!(identity(&fixture), initial);
    fixture.settle("pair-owner-close", |f, _| f.predicate("close"));
    fixture.closed.set(true);
    fixture.release_owners().unwrap();
    fs::write(
        root.join("pair-crossrate-app.provisional.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "initial":initial,"converted":converted,"rates":[48000,96000,96000,96000,48000],
            "actual_assigned_pair_apply_auto_rollback":true,"all_five_hashes":true,
            "reader_end":"pending exact original EXE and owning Job END then parent sealed proof"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct SourceImage {
    generation: u64,
    request: u64,
    prepared_epoch: u64,
    cell: usize,
    lifetime: u64,
    digest: String,
    original_path: PathBuf,
    rate: u32,
    frames: usize,
}

#[derive(Clone, Copy, Default, serde::Serialize)]
struct PacedRow {
    lateness_ns: u128,
    command_roundtrip_ns: u128,
    interarrival_ns: u128,
    skipped_slots: u64,
    observed: TickObservation,
}

struct DirectoryGuard {
    original: PathBuf,
    readers_ended: Arc<AtomicBool>,
}
impl Drop for DirectoryGuard {
    fn drop(&mut self) {
        if !self.readers_ended.load(Ordering::Acquire) {
            eprintln!("private test cwd retained: original reader END has not been proved");
        } else if let Err(error) = std::env::set_current_dir(&self.original) {
            eprintln!("original readers ended but test cwd restoration failed: {error}");
        }
    }
}

struct Fixture {
    engine: Option<Py<AudioEngine>>,
    probe: Option<Py<PyAny>>,
    seed_leases: Vec<cold_store::CommittedColdLease>,
    closed: Cell<bool>,
}

fn import_sources(root: &Path, inputs: &[String]) -> Vec<cold_store::CommittedColdLease> {
    assert_eq!(inputs.len(), 6);
    let samples = root.join("samples");
    fs::create_dir(&samples).unwrap();
    inputs
        .iter()
        .map(|input| {
            let path = Path::new(input);
            let mut transaction =
                cold_store::ColdTransaction::capture(&samples, path, true, &|| false).unwrap();
            let _gate = transaction.preparation_gate(48_000, 2, &|| false).unwrap();
            material_migration::prepare_disk_pcm(
                &mut transaction,
                path,
                48_000,
                2,
                cold_jobs::PCM_LIMIT_BYTES,
                &|| false,
                &mut |_| {},
            )
            .unwrap();
            transaction.commit(&|| false).unwrap();
            transaction.into_lease()
        })
        .collect()
}

impl Fixture {
    fn new(root: &Path, factory: Arc<Factory>) -> Self {
        Python::initialize();
        Python::attach(|py| {
            let modules = py
                .import("sys")
                .unwrap()
                .getattr("modules")
                .unwrap()
                .cast_into::<PyDict>()
                .unwrap();
            let names: Vec<String> = modules
                .keys()
                .iter()
                .map(|name| name.extract().unwrap())
                .collect();
            assert!(
                !names.iter().any(|name| name.starts_with("flitzis_looper")),
                "requires a fresh process before any App/Native import"
            );
            let native = crate::flitzis_looper_audio::embedded_test_module(py).unwrap();
            assert!(
                native
                    .getattr("AudioEngine")
                    .unwrap()
                    .is(py.get_type::<AudioEngine>())
            );
            assert!(
                native
                    .getattr("ExistingLegacyStemWork")
                    .unwrap()
                    .is(py.get_type::<ExistingLegacyStemWork>())
            );
            modules.set_item("flitzis_looper_audio", &native).unwrap();
            modules
                .set_item("flitzis_looper_audio.flitzis_looper_audio", &native)
                .unwrap();
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
            let mut engine = AudioEngine::new().unwrap();
            assert!(engine.test_control_channels.is_none() && engine.test_output_format.is_none());
            engine.test_asio_driver = Some(factory);
            let engine = Py::new(py, engine).unwrap();
            let module = PyModule::from_code(
                py,
                &CString::new(SCRIPT).unwrap(),
                c"audio_output_probe.py",
                c"audio_output_probe",
            )
            .unwrap();
            let inputs = module
                .getattr("write_sources")
                .unwrap()
                .call1((root.to_string_lossy().as_ref(),))
                .unwrap()
                .extract::<Vec<String>>()
                .unwrap();
            let seed_leases = py.detach(|| import_sources(root, &inputs));
            let source_paths: Vec<String> = seed_leases
                .iter()
                .map(|lease| material_paths::ordinary_windows_path(&lease.original_path))
                .collect();
            let probe = module
                .getattr("OutputProbe")
                .unwrap()
                .call1((
                    engine.bind(py),
                    root.to_string_lossy().as_ref(),
                    source_paths,
                ))
                .unwrap()
                .unbind();
            Self {
                engine: Some(engine),
                probe: Some(probe),
                seed_leases,
                closed: Cell::new(false),
            }
        })
    }

    fn engine(&self) -> &Py<AudioEngine> {
        self.engine
            .as_ref()
            .expect("original Engine already released")
    }
    fn probe(&self) -> &Py<PyAny> {
        self.probe.as_ref().expect("original App already released")
    }

    fn release_owners(&mut self) -> PyResult<()> {
        Python::attach(|py| {
            if let Some(probe) = self.probe.take() {
                drop(probe.into_bound(py));
            }
            if let Some(engine) = self.engine.take() {
                drop(engine.into_bound(py));
            }
            // Collect original Python controller cycles after real shutdown and
            // joins. No production cache is cleared to manufacture reader END.
            py.import("gc")?.call_method0("collect")?;
            Ok(())
        })
    }

    fn cleanup_step(&self) -> PyResult<bool> {
        Python::attach(|py| {
            let probe = self.probe().bind(py);
            if probe.call_method0("close")?.extract::<bool>()? {
                return Ok(true);
            }
            probe.call_method0("poll")?;
            if probe.call_method0("tick_allowed")?.extract::<bool>()?
                && let Ok(engine) = self.engine().bind(py).try_borrow()
                && let Some(stream) = engine.stream_handle.as_ref()
            {
                py.detach(|| stream.test_tick())
                    .map_err(PyRuntimeError::new_err)?;
            }
            Ok(false)
        })
    }

    fn call(&self, method: &str) {
        Python::attach(|py| {
            self.probe().call_method0(py, method).unwrap();
        });
    }

    fn predicate(&self, method: &str) -> bool {
        Python::attach(|py| {
            self.probe()
                .call_method0(py, method)
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn status(&self) -> String {
        Python::attach(|py| {
            self.probe()
                .call_method0(py, "status")
                .unwrap()
                .extract(py)
                .unwrap()
        })
    }

    fn tick(&self) -> Option<TickObservation> {
        if !self.predicate("tick_allowed") {
            return None;
        }
        Python::attach(|py| {
            let engine = self.engine().bind(py).try_borrow().ok()?;
            let stream = engine.stream_handle.as_ref()?;
            Some(py.detach(|| stream.test_tick()).unwrap())
        })
    }

    fn settle(
        &self,
        label: &str,
        predicate: impl Fn(&Self, Option<&TickObservation>) -> bool,
    ) -> TickObservation {
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut last = None;
        loop {
            self.call("poll");
            if predicate(self, last.as_ref()) {
                return last.unwrap_or_default();
            }
            if let Some(row) = self.tick() {
                last = Some(row);
            }
            if Instant::now() >= deadline {
                let status = self.status();
                let diagnostic = serde_json::json!({
                    "stage": label,
                    "status": status,
                    "last_callback": last,
                });
                if let Err(error) = fs::write(
                    "settlement-failure.json",
                    serde_json::to_vec_pretty(&diagnostic).unwrap(),
                ) {
                    eprintln!("original failure diagnostic could not be saved: {error}");
                }
                panic!("{label} did not settle: {status}; last callback: {last:?}");
            }
            thread::sleep(PERIOD);
        }
    }

    fn ready(&self) {
        self.settle("output-ready", |f, _| f.predicate("ready"));
    }

    fn sources(&self) -> [SourceImage; 6] {
        Python::attach(|py| {
            let engine = self.engine().borrow(py);
            let requests = engine.pad_request_ids.lock().unwrap();
            std::array::from_fn(|id| {
                let binding = input_runtime_binding::capture_under_request_lock(&engine, id)
                    .unwrap()
                    .unwrap();
                assert!(binding.current() && binding.available());
                assert!(
                    engine
                        .input_runtime_ownership
                        .binding_source_adoption_accepted(id, binding.binding)
                );
                let placement = engine.input_runtime_ownership.slot_snapshot(id).unwrap();
                let lease = engine.cold_leases.lock().unwrap()[id].clone().unwrap();
                let sample = engine.sample_cache.lock().unwrap()[id].clone().unwrap();
                lease.verify_reference(&sample).unwrap();
                assert_eq!(sample.channels, 2);
                assert_eq!(sample.frame_count(), 3 * binding.sample_rate_hz() as usize);
                assert_eq!(sample.source_sample_count(), sample.frame_count() * 2);
                assert_eq!(
                    engine.cold_lease_generations[id].load(Ordering::Acquire),
                    binding.source_generation
                );
                SourceImage {
                    generation: binding.source_generation,
                    request: requests[id],
                    prepared_epoch: engine.prepared_source_epochs[placement.cell().index()]
                        .load(Ordering::Acquire),
                    cell: placement.cell().index(),
                    lifetime: placement.lifetime(),
                    digest: binding.source_digest().to_owned(),
                    original_path: lease.original_path.clone(),
                    rate: binding.sample_rate_hz(),
                    frames: sample.frame_count(),
                }
            })
        })
    }

    fn verify(&self, rate: u32, offsets: Vec<i32>) {
        Python::attach(|py| {
            self.probe()
                .call_method1(py, "verify", (rate, offsets))
                .unwrap();
        });
        let row = self.tick().unwrap();
        assert!(row.pads.iter().all(|pad| !pad.active));
        assert_eq!((row.pads[0].stem_mode, row.pads[0].stem_mask), (1, 5));
    }

    fn apply(&self, rate: u32) {
        Python::attach(|py| {
            self.probe()
                .call_method1(py, "apply_rate", (rate,))
                .unwrap();
        });
        self.ready();
    }

    fn paced(&self, count: usize) -> Vec<PacedRow> {
        let mut rows = Vec::with_capacity(count);
        let mut deadline = Instant::now() + PERIOD;
        let mut previous = Instant::now();
        for index in 0..count {
            if index % 16 == 0 {
                self.call("poll");
            }
            if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
            let begin = Instant::now();
            let observed = self
                .tick()
                .expect("original owner unavailable during measured callbacks");
            let end = Instant::now();
            let next = deadline + PERIOD;
            let skipped = end
                .checked_duration_since(next)
                .map_or(0, |late| (late.as_nanos() / PERIOD.as_nanos() + 1) as u64);
            rows.push(PacedRow {
                lateness_ns: begin.saturating_duration_since(deadline).as_nanos(),
                command_roundtrip_ns: end.duration_since(begin).as_nanos(),
                interarrival_ns: begin.duration_since(previous).as_nanos(),
                skipped_slots: skipped,
                observed,
            });
            previous = begin;
            deadline = next + PERIOD * u32::try_from(skipped).unwrap();
        }
        rows
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !self.closed.get() && self.probe.is_some() {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                match self.cleanup_step() {
                    Ok(true) => {
                        self.closed.set(true);
                        break;
                    }
                    Ok(false) if Instant::now() < deadline => thread::sleep(PERIOD),
                    Ok(false) => {
                        eprintln!("original test cleanup remains incomplete; no reader END claim");
                        break;
                    }
                    Err(error) => {
                        eprintln!("original test cleanup failed; preserve primary error: {error}");
                        break;
                    }
                }
            }
        }
        if (self.probe.is_some() || self.engine.is_some())
            && let Err(error) = self.release_owners()
        {
            eprintln!("original owner release failed: {error}");
        }
    }
}

fn fresh_sources(before: &[SourceImage; 6], after: &[SourceImage; 6], rate: u32) {
    for (old, new) in before.iter().zip(after) {
        assert_eq!(
            (&new.digest, &new.original_path, new.cell, new.lifetime),
            (&old.digest, &old.original_path, old.cell, old.lifetime)
        );
        assert!(
            new.generation > old.generation
                && new.request > old.request
                && new.prepared_epoch > old.prepared_epoch
        );
        assert_eq!(new.rate, rate);
    }
}

fn productive(rows: &[PacedRow], wet: bool) {
    assert_eq!(rows.len(), SLOTS);
    for row in rows {
        assert_eq!(row.observed.callback.frames, 96);
        assert_eq!(row.observed.callback.guards_intact, 1);
        assert!(row.observed.callback.left_peak > 0.0 && row.observed.callback.right_peak > 0.0);
        assert_eq!(row.observed.native.adapter_shortfall_channel_frames, 0);
        assert_eq!(row.observed.native.source_refusals, 0);
        for pad in &row.observed.pads {
            assert!(pad.active && pad.post_dsp_peak > 0.0 && !pad.wet_failed);
            assert_eq!(pad.key_lock, wet);
            assert!(pad.source_pcm != 0 && pad.source_samples == 3 * 96_000 * 2);
        }
    }
    if wet {
        assert!(
            rows.iter()
                .map(|row| row.observed.native.shifts)
                .sum::<u64>()
                > 0
        );
        for id in 0..6 {
            let first = rows[0].observed.pads[id];
            let last = rows.last().unwrap().observed.pads[id];
            assert!(
                rows.iter()
                    .all(|row| row.observed.pads[id].generation == first.generation)
            );
            assert!(
                rows.iter()
                    .all(|row| row.observed.pads[id].source_pcm == first.source_pcm)
            );
            assert!(
                last.productive_fed_frames
                    > first.productive_fed_frames + first.output_fifo_capacity as u64
            );
            assert!(
                SLOTS * 96
                    > first.input_fifo_capacity
                        + first.output_fifo_capacity
                        + first.native_start_delay
            );
            assert!(first.native_state != 0 && first.native_block_size > 0);
            assert!(
                rows.iter()
                    .any(|row| row.observed.pads[id].wet_current_source_output)
            );
            assert_ne!((first.frame, first.fraction), (last.frame, last.fraction));
        }
    } else {
        assert!(rows.iter().all(|row| row.observed.native.shifts == 0));
    }
}

#[test]
#[ignore = "fresh-process actual App48->96/legacy/rollback and paced six-WET96 software profile; no device/model"]
fn actual_audio_output_app_restore_and_six_wet_96k_96_frames() {
    let root = PathBuf::from(
        std::env::var_os("FLITZI_AUDIO_OUTPUT_PROFILE_ROOT")
            .expect("explicit owned artifact root required"),
    );
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir(&root).unwrap();
    let readers_ended = Arc::new(AtomicBool::new(false));
    let _cwd = DirectoryGuard {
        original: std::env::current_dir().unwrap(),
        readers_ended: readers_ended.clone(),
    };
    std::env::set_current_dir(&root).unwrap();
    let factory = Factory::new().unwrap();
    let mut fixture = Fixture::new(&root, factory.clone());
    fixture.ready();
    let initial = fixture.sources();
    for (seed, loaded) in fixture.seed_leases.iter().zip(&initial) {
        assert_eq!(
            material_paths::ordinary_windows_path(&seed.original_path),
            material_paths::ordinary_windows_path(&loaded.original_path),
        );
        assert_eq!(
            seed.manifest.descriptor["decoder"]["original"]["sha256"].as_str(),
            Some(loaded.digest.as_str()),
        );
    }
    // Original import leases remain until all six real Loader source OwnACKs.
    // Release those producer readers on this control thread before any Apply.
    fixture.seed_leases.clear();
    fixture.call("install_legacy_intent");
    fixture.ready();
    fixture.verify(48_000, (7..13).collect());
    let legacy = fixture.sources();
    fresh_sources(&initial, &legacy, 48_000);
    fixture.apply(96_000);
    fixture.verify(96_000, (7..13).map(|value| value * 2).collect());
    let changed = fixture.sources();
    fresh_sources(&legacy, &changed, 96_000);
    fixture.call("make_grid_offset_odd");
    fixture.settle("grid-edit-settled", |f, _| {
        f.predicate("configuration_idle")
    });
    factory
        .fail_start_of_session(factory.info().unwrap().created + 1)
        .unwrap();
    fixture.apply(48_000); // Actual failed candidate closes, then original96k restores.
    fixture.verify(96_000, vec![15, 16, 18, 20, 22, 24]);
    let rolled_back = fixture.sources();
    fresh_sources(&changed, &rolled_back, 96_000);
    assert!(fixture.status().contains("Failed to start audio stream"));
    factory
        .fail_callback_of_session(factory.info().unwrap().created + 1)
        .unwrap();
    fixture.apply(48_000); // Successful Open/grid15->8, then callback failure.
    fixture.verify(96_000, vec![15, 16, 18, 20, 22, 24]);
    let callback_rollback = fixture.sources();
    fresh_sources(&rolled_back, &callback_rollback, 96_000);
    fixture.call("apply_auto");
    fixture.ready();
    fixture.verify(96_000, vec![15, 16, 18, 20, 22, 24]);
    let automatic = fixture.sources();
    fresh_sources(&callback_rollback, &automatic, 96_000);
    let mut cohorts = Vec::with_capacity(2);
    for wet in [true, false] {
        Python::attach(|py| {
            fixture
                .probe()
                .call_method1(py, "start", (if wet { "wet" } else { "off" },))
                .unwrap();
        });
        fixture.settle(
            if wet {
                "six-wet-active"
            } else {
                "six-off-active"
            },
            |_, row| row.is_some_and(|row| row.pads.iter().all(|pad| pad.active)),
        );
        let _warmup = fixture.paced(512);
        let rows = fixture.paced(SLOTS);
        // Preserve the original raw timings and failed pacing even when the
        // following functional assertions fail. No green-only evidence filter.
        fs::write(
            root.join(if wet {
                "six-wet-raw.json"
            } else {
                "six-off-raw.json"
            }),
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
        productive(&rows, wet);
        cohorts.push((wet, rows));
        fixture.call("stop");
        fixture.settle(
            if wet {
                "six-wet-stopped"
            } else {
                "six-off-stopped"
            },
            |f, row| {
                f.predicate("stopped")
                    && row.is_some_and(|row| row.pads.iter().all(|pad| !pad.active))
            },
        );
    }
    let seen = Python::attach(|py| {
        fixture
            .engine()
            .borrow(py)
            .stream_handle
            .as_ref()
            .unwrap()
            .test_owner_observer()
    });
    fixture.settle("original-owner-close", |f, _| f.predicate("close"));
    fixture.closed.set(true);
    let ended = seen.lock().unwrap().clone();
    assert!(
        ended.backend_released
            && ended.retirement_verified
            && ended.owner_ended
            && ended.owner_join_verified
    );
    let device = factory.info().unwrap();
    assert_eq!(device.active, 0);
    assert_eq!(device.created, device.released);
    fixture.release_owners().unwrap();
    // The original ProjectAssets service retains saved cold-file guards for
    // this process. Only the supervising parent can prove all-file reader END
    // after this exact EXE and its owning Job end. Keep the cwd guard pending.
    fs::write(root.join("actual-audio-output-software.provisional.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "scope": "actual App/ASIO software path; normal owner thread; no physical device/deadline/hearing acceptance",
        "acceptance": "provisional runtime/source/callback evidence",
        "reader_end_boundary": "pending supervised read-only/share-zero proof after original Native EXE and owning Job END",
        "cwd_retained_until_process_exit": true,
        "callback_budget_ns": 1_000_000, "source_initial": initial, "source96": changed,
        "source_after_rollback": rolled_back, "device_after_close": device,
        "source_after_callback_failure_rollback": callback_rollback,
        "source_after_auto_reopen": automatic,
        "all_original_file_readers_exclusive_open_verified": false,
        "cohorts": cohorts,
    })).unwrap()).unwrap();
}
