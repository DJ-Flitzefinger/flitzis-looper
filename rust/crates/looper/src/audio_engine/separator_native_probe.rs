//! Opt-in genuine neural output through the shared controller and native renderer.
//! A test producer supplies the same native publication kernel without a CPAL stream.

use super::*;
use crate::audio_engine::PreparedSourceTicket;
use crate::messages::StemMixMode;
use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyModule};
use std::ffi::CString;
use std::process::Command;

const RATE: u32 = 44_100;
const TEST_NAME: &str = "audio_engine::cold_residency_tests::separator_native_probe::actual_separator_worker_publishes_renders_and_restores_without_model";
const CHILD_MARKER: &str = "FLITZIS_SEPARATOR_NATIVE_CHILD";
const CONFIG_ENV: &str = "FLITZIS_SEPARATOR_PROBE_CONFIG";

struct ProbeConfig {
    source_path: PathBuf,
    source_sha256: String,
    project_root: PathBuf,
    output_path: PathBuf,
}

impl ProbeConfig {
    fn from_bytes(bytes: &[u8]) -> Self {
        let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        assert!(value.is_object(), "probe config must be a JSON object");
        let required = |name: &str| {
            value
                .get(name)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| panic!("probe config requires nonempty string {name}"))
                .to_owned()
        };
        Self {
            source_path: PathBuf::from(required("source_path")),
            source_sha256: required("source_sha256"),
            project_root: PathBuf::from(required("project_root")),
            output_path: PathBuf::from(required("output_path")),
        }
    }
}

#[pyclass]
struct NativeBridge {
    #[pyo3(get)]
    engine: Py<AudioEngine>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
}

#[pymethods]
impl NativeBridge {
    fn publish_prepared_stems(
        &self,
        py: Python<'_>,
        id: usize,
        source_version: String,
        cache_dir: String,
        source_ticket: &PreparedSourceTicket,
    ) -> PyResult<()> {
        self.engine.borrow(py).publish_prepared_stems_with_producer(
            py,
            id,
            source_version,
            cache_dir,
            source_ticket,
            &self.producer,
        )
    }

    fn loaded_sample_shape(&self, py: Python<'_>, id: usize) -> PyResult<(u32, usize, usize)> {
        let engine = self.engine.borrow(py);
        let cache = engine.sample_cache.lock().unwrap();
        let sample = cache[id].as_ref().unwrap();
        Ok((RATE, sample.channels, sample.frame_count()))
    }
}

fn call(probe: &Py<PyAny>, method: &str) {
    Python::attach(|py| probe.call_method0(py, method).unwrap());
}

fn render(callback: &mut Callback, frames: usize) -> Vec<f32> {
    assert!(
        callback
            .mixer
            .play_sample_rt(0, 1.0, &mut callback.retirement)
    );
    let mut output = vec![0.0; frames * 2];
    callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        0,
        &mut callback.retirement,
    );
    callback.mixer.stop_sample_rt(0, &mut callback.retirement);
    assert!(output.iter().all(|sample| sample.is_finite()));
    assert!(output.iter().any(|sample| sample.abs() > 1.0e-8));
    output
}

fn digest(samples: &[f32]) -> String {
    let mut hash = Sha256::new();
    for sample in samples {
        hash.update(sample.to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}

#[test]
#[ignore = "requires explicit local model/CUDA/private clip config; opens no audio device"]
fn actual_separator_worker_publishes_renders_and_restores_without_model() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        assert!(
            std::env::var_os(CONFIG_ENV).is_some(),
            "missing probe config"
        );
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                TEST_NAME,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_MARKER, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "actual separator/native probe failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        return;
    }
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    let config_path = PathBuf::from(std::env::var_os(CONFIG_ENV).unwrap());
    assert!(config_path.is_absolute());
    let config_bytes = fs::read(&config_path).unwrap();
    let config = ProbeConfig::from_bytes(&config_bytes);
    assert!(config.source_path.is_absolute());
    assert!(config.project_root.is_absolute());
    assert!(config.output_path.is_absolute());
    assert!(
        config
            .project_root
            .starts_with(repository.parent().unwrap().join("scratch"))
    );
    assert!(
        config
            .output_path
            .starts_with(repository.parent().unwrap().join("scratch"))
    );
    let source = fs::read(&config.source_path).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&source)),
        config.source_sha256
    );
    Python::initialize();
    let frames = Python::attach(|py| {
        let reader = py
            .import("wave")
            .unwrap()
            .getattr("open")
            .unwrap()
            .call1((config.source_path.to_string_lossy().as_ref(), "rb"))
            .unwrap();
        let rate: u32 = reader
            .call_method0("getframerate")
            .unwrap()
            .extract()
            .unwrap();
        let channels: usize = reader
            .call_method0("getnchannels")
            .unwrap()
            .extract()
            .unwrap();
        let width: usize = reader
            .call_method0("getsampwidth")
            .unwrap()
            .extract()
            .unwrap();
        let compression: String = reader
            .call_method0("getcomptype")
            .unwrap()
            .extract()
            .unwrap();
        let frames: usize = reader
            .call_method0("getnframes")
            .unwrap()
            .extract()
            .unwrap();
        reader.call_method0("close").unwrap();
        assert_eq!(rate, RATE);
        assert_eq!(channels, 2);
        assert_eq!(width, 2);
        assert_eq!(compression, "NONE");
        frames
    });
    assert!((4_096..=RATE as usize * 30).contains(&frames));
    assert!(!config.project_root.exists(), "probe project must be fresh");
    fs::create_dir_all(config.project_root.join("samples")).unwrap();
    fs::write(config.project_root.join("samples/source.wav"), &source).unwrap();
    struct CurrentDirectory(PathBuf);
    impl Drop for CurrentDirectory {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).unwrap();
        }
    }
    let _restore_directory = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(&config.project_root).unwrap();
    let engine = Python::attach(|py| Py::new(py, AudioEngine::new().unwrap()).unwrap());
    let (producer, mut consumer) = rtrb::RingBuffer::new(16);
    let producer = Arc::new(Mutex::new(producer));
    let mut callback = Python::attach(|py| Callback::new(&engine.borrow(py), RATE));
    let request = Python::attach(|py| {
        admit_for_format_selected(
            &engine.borrow(py),
            0,
            "samples/source.wav".into(),
            (false, false, false, None),
            producer.clone(),
            (2, RATE, config.project_root.join("samples")),
        )
        .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let cached_path = loop {
        callback.drain(&mut consumer);
        let event = Python::attach(|py| engine.borrow(py).loader_rx.lock().unwrap().try_recv());
        match event {
            Ok(LoaderEvent::Success {
                request_id,
                cached_path,
                ..
            }) if request_id == request => {
                break cached_path;
            }
            Ok(LoaderEvent::Error { error, .. }) => panic!("source load failed: {error}"),
            _ => {}
        }
        assert!(Instant::now() < deadline, "actual source load timed out");
        std::thread::sleep(Duration::from_millis(1));
    };
    let full_mix = render(&mut callback, frames);
    let probe = Python::attach(|py| {
        py.import("sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .call_method1(
                "insert",
                (0, repository.join("src").to_string_lossy().as_ref()),
            )
            .unwrap();
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!("separator_native_probe.py")).unwrap(),
            c"separator_native_probe.py",
            c"separator_native_probe",
        )
        .unwrap();
        let bridge = Py::new(
            py,
            NativeBridge {
                engine: engine.clone_ref(py),
                producer: producer.clone(),
            },
        )
        .unwrap();
        module
            .getattr("Probe")
            .unwrap()
            .call1((
                bridge,
                cached_path,
                frames,
                RATE,
                config_path.to_string_lossy().as_ref(),
            ))
            .unwrap()
            .unbind()
    });
    let version: String = Python::attach(|py| {
        probe
            .call_method0(py, "begin")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    assert_eq!(callback.drain(&mut consumer), 1);
    call(&probe, "accepted");
    let version_hash = super::super::stem_cache::source_version_hash(&version);
    {
        let mut producer = producer.lock().unwrap();
        producer
            .push(ControlMessage::SetStemMixMode {
                id: 0,
                mode: StemMixMode::AllStems,
                source_version_hash: version_hash,
            })
            .unwrap();
        producer
            .push(ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask: 1,
                source_version_hash: version_hash,
            })
            .unwrap();
    }
    assert_eq!(callback.drain(&mut consumer), 2);
    let vocals = render(&mut callback, frames);
    let steady = 256 * 2;
    assert!(
        full_mix[steady..]
            .iter()
            .zip(&vocals[steady..])
            .any(|(a, b)| (a - b).abs() > 1.0e-6)
    );
    call(&probe, "restore");
    assert_eq!(callback.drain(&mut consumer), 1);
    call(&probe, "restored_accepted");
    let restored = render(&mut callback, frames);
    assert_eq!(digest(&vocals[steady..]), digest(&restored[steady..]));
    call(&probe, "begin_stale_restore");
    Python::attach(|py| {
        let engine = engine.borrow(py);
        super::super::next_pad_request_id(
            &engine.pad_request_ids,
            0,
            &engine.prepared_source_epochs[0],
        )
        .unwrap();
    });
    assert_eq!(callback.drain(&mut consumer), 1);
    call(&probe, "rejected");
    let after_rejection = render(&mut callback, frames);
    assert_eq!(
        digest(&vocals[steady..]),
        digest(&after_rejection[steady..])
    );
    call(&probe, "finish");
    let mut evidence: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.output_path).unwrap()).unwrap();
    evidence["native_render"] = serde_json::json!({
        "sample_rate_hz": RATE,
        "frames": frames,
        "finite": true,
        "nonzero": true,
        "vocals_distinct_from_full_mix": true,
        "full_mix_sha256": digest(&full_mix[steady..]),
        "vocals_sha256": digest(&vocals[steady..]),
        "restored_sha256": digest(&restored[steady..]),
        "after_stale_rejection_sha256": digest(&after_rejection[steady..]),
        "excluded_ramp_frames": 256,
    });
    evidence["config_sha256"] = format!("{:x}", Sha256::digest(&config_bytes)).into();
    evidence["executing_native_test_exe"] = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned()
        .into();
    fs::write(
        &config.output_path,
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    Python::attach(|py| engine.borrow_mut(py).shut_down().unwrap());
}
