//! Native cancellation through the real bridge and a deliberately held adapter double.

use super::*;
use pyo3::types::PyModule;
use std::ffi::CString;

struct HeldAdapter(Py<PyAny>);

impl HeldAdapter {
    fn install(installation: &str) -> Self {
        Python::attach(|py| {
            let code = CString::new(
                r#"
from pathlib import Path
from threading import Event
from types import SimpleNamespace
from flitzis_looper.analysis import source_map_worker as bridge
from flitzis_looper.analysis.contracts import BeatComponentResult, BeatModelIdentity

class HeldAdapter:
    def __init__(self, installation):
        self.installation = Path(installation)
        self.entered, self.observed, self.retired = Event(), Event(), Event()
        self.configuration = SimpleNamespace(model=BeatModelIdentity())
        self.original_load = bridge.load_worker_configuration
        self.original_adapter = bridge.BeatWorkerAdapter
        self.reader, self.path = None, None
        bridge.load_worker_configuration = self.load
        bridge.BeatWorkerAdapter = self.adapter
    def load(self, installation, scratch):
        if installation == self.installation:
            return self.configuration
        return self.original_load(installation, scratch)
    def adapter(self, configuration, limits):
        if configuration is self.configuration:
            return self
        return self.original_adapter(configuration, limits)
    def run(self, request, cancel):
        self.path = request.pcm.path
        self.reader = self.path.open('rb')
        self.entered.set()
        if not cancel.wait(5):
            raise RuntimeError('Native cancellation did not cross the bridge')
        self.observed.set()
        return BeatComponentResult(request.identity, request.model, 'failed',
                                   'held protocol reader', resources_released=False)
    def finish(self):
        if self.reader is not None:
            self.reader.close()
        self.retired.set()
    def restore(self):
        self.finish()
        bridge.load_worker_configuration = self.original_load
        bridge.BeatWorkerAdapter = self.original_adapter
"#,
            )
            .unwrap();
            let module = PyModule::from_code(
                py,
                &code,
                c"source_map_held_adapter.py",
                c"source_map_held_adapter",
            )
            .unwrap();
            Self(
                module
                    .getattr("HeldAdapter")
                    .unwrap()
                    .call1((installation,))
                    .unwrap()
                    .unbind(),
            )
        })
    }

    fn wait(&self, event: &str) {
        Python::attach(|py| {
            assert!(
                self.0
                    .bind(py)
                    .getattr(event)
                    .unwrap()
                    .call_method1("wait", (5.0,))
                    .unwrap()
                    .extract::<bool>()
                    .unwrap(),
                "{event}"
            );
        });
    }

    fn finish(&self) {
        Python::attach(|py| {
            self.0.call_method0(py, "finish").unwrap();
        });
    }
}

impl Drop for HeldAdapter {
    fn drop(&mut self) {
        Python::attach(|py| {
            self.0.call_method0(py, "restore").unwrap();
        });
    }
}

fn status_flag(job: &OfflineAnalysisJob, name: &str) -> bool {
    Python::attach(|py| {
        job.source_beatmap_async_status(py)
            .unwrap()
            .unwrap()
            .bind(py)
            .get_item(name)
            .unwrap()
            .unwrap()
            .extract::<bool>()
            .unwrap()
    })
}

#[test]
fn original_beat_this_cancel_keeps_native_owners_until_python_reader_retires() {
    let (fixture, _) = fixture(1);
    let installation = fixture
        .directory
        .path()
        .join("held-protocol-double")
        .to_string_lossy()
        .into_owned();
    let adapter = HeldAdapter::install(&installation);
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let epochs = fixture
        .engine
        .prepared_source_epochs
        .iter()
        .map(|epoch| epoch.load(Ordering::Acquire))
        .collect::<Vec<_>>();
    let job = fixture
        .engine
        .begin_source_beatmap_beat_this_raw_async(0, installation.clone(), generation(&fixture))
        .unwrap();
    adapter.wait("entered");
    let path = Python::attach(|py| {
        adapter
            .0
            .bind(py)
            .getattr("path")
            .unwrap()
            .str()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    });
    assert!(Path::new(&path).is_file());
    job.cancel();
    adapter.wait("observed");
    assert!(status_flag(&job, "cancellation_requested"));
    assert!(!status_flag(&job, "terminal"));
    assert!(!status_flag(&job, "raw_review_retired"));
    assert!(
        fixture
            .engine
            .project_assets
            .raw_review_busy
            .load(Ordering::Acquire)
    );
    assert!(
        job.state().snapshot.try_lock().is_err(),
        "actual source reader remains borrowed by the running bridge"
    );
    assert!(Path::new(&path).is_file());
    assert!(
        fixture
            .engine
            .begin_source_beatmap_beat_this_raw_async(0, installation.clone(), generation(&fixture))
            .is_err()
    );
    adapter.finish();
    assert_eq!(terminal(&job, Duration::from_secs(15)), "cancelled");
    assert!(status_flag(&job, "raw_review_retired"));
    assert!(!Path::new(&path).exists());
    no_staging(&fixture);
    Python::attach(|py| {
        while let Some(event) = fixture.engine.poll_loader_events(py).unwrap() {
            let kind = event
                .bind(py)
                .cast::<PyDict>()
                .unwrap()
                .get_item("type")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap();
            assert_ne!(kind, "source_beatmap_raw_completed");
            assert_ne!(kind, "source_beatmap_completed");
        }
    });
    drop(adapter);
    // The real bridge can be admitted again; an absent installation fails normally.
    let retry = fixture
        .engine
        .begin_source_beatmap_beat_this_raw_async(0, installation, generation(&fixture))
        .unwrap();
    assert_eq!(terminal(&retry, Duration::from_secs(15)), "failed");
    assert!(status_flag(&retry, "raw_review_retired"));
    no_staging(&fixture);
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    assert_eq!(
        fixture
            .engine
            .prepared_source_epochs
            .iter()
            .map(|epoch| epoch.load(Ordering::Acquire))
            .collect::<Vec<_>>(),
        epochs
    );
}
