//! Actual cold queue/cancellation and immutable last-reader retirement costs.
use super::*;
use crate::audio_engine::cold_load::unload_for_producer;
use crate::audio_engine::complete_context::CompleteSourceReader;

struct Fixture {
    engine: AudioEngine,
    callback: Callback,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
    root: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new(root: PathBuf, source: &Path) -> Self {
        fs::create_dir_all(&root).unwrap();
        let engine = AudioEngine::new().unwrap();
        let callback = Callback::new(&engine, 48_000);
        let (producer, consumer) = rtrb::RingBuffer::new(128);
        Self {
            engine,
            callback,
            producer: Arc::new(Mutex::new(producer)),
            consumer,
            root,
            source: source.to_owned(),
        }
    }

    fn admit(&self, id: usize) -> u64 {
        admit_for_format_selected(
            &self.engine,
            id,
            self.source.to_string_lossy().into_owned(),
            (
                false,
                false,
                true,
                Some(ResidentLoadHint {
                    start_s: 42.0,
                    end_s: 42.5,
                    key_lock: false,
                }),
            ),
            self.producer.clone(),
            (2, 48_000, self.root.clone()),
        )
        .unwrap()
    }

    fn pending(&self, id: usize, request: u64) {
        wait_until(Duration::from_secs(300), || {
            self.engine.input_runtime_ownership.cold_status(id, request) == Some(0)
        });
        // begin_cold occurs inside publication's producer fence, before enqueue.
        // Acquire that same fence so a following drain sees the complete command.
        drop(self.producer.lock().unwrap());
    }

    fn drain(&mut self) {
        drain(&mut self.callback, &mut self.consumer);
    }

    fn drain_all(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.consumer.is_empty() {
            self.drain();
            assert!(
                Instant::now() < deadline,
                "actual native queue drain timeout"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            self.drain();
            if self.engine.cold_jobs.counts_for_test() == (0, 0, 0) {
                self.drain_all();
                break;
            }
            assert!(
                Instant::now() < deadline,
                "actual cold lane settlement timeout"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn events(&self) -> Vec<Value> {
        self.engine
            .loader_rx
            .lock()
            .unwrap()
            .try_iter()
            .map(|event| match event {
                LoaderEvent::Success { id, request_id, .. } => {
                    json!({"kind":"success","id":id,"request":request_id})
                }
                LoaderEvent::Error {
                    id,
                    request_id,
                    error,
                    ..
                } => json!({"kind":"error","id":id,"request":request_id,"error":error}),
                LoaderEvent::Started { id, request_id, .. } => {
                    json!({"kind":"started","id":id,"request":request_id})
                }
                _ => json!({"kind":"other"}),
            })
            .collect()
    }

    fn inventory(&self) -> Vec<Value> {
        let mut pending = vec![self.root.clone()];
        let mut files = Vec::new();
        let mut visited = 0;
        while let Some(directory) = pending.pop() {
            if !directory.exists() {
                continue;
            }
            let entries = match fs::read_dir(directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => panic!("fixture inventory: {error}"),
            };
            for entry in entries {
                let entry = entry.unwrap();
                visited += 1;
                assert!(visited <= 4096, "fixture inventory bound");
                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => panic!("fixture metadata: {error}"),
                };
                if metadata.is_dir() {
                    pending.push(entry.path());
                } else {
                    files.push(json!({"path":entry.path(),"bytes":metadata.len()}));
                }
            }
        }
        files
    }

    fn orphan_cleanup(&self) -> Value {
        measure(|| {
            let before = self.inventory();
            let deadline = Instant::now() + Duration::from_secs(60);
            while !self.inventory().is_empty() {
                assert!(
                    Instant::now() < deadline,
                    "orphan cleanup retained {:?}",
                    self.inventory()
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            json!({"owned_files_before":before,"owned_files_after":[],"actual_cleanup_status":self.engine.project_asset_cleanup_status().unwrap()})
        })
    }

    fn retire_loaded(&mut self, ids: &[usize]) {
        let originals: Vec<_> = {
            let leases = self.engine.cold_leases.lock().unwrap();
            ids.iter()
                .map(|id| leases[*id].as_ref().unwrap().original_path.clone())
                .collect()
        };
        for original in &originals {
            self.engine
                .project_assets
                .retire(&self.root, original, false)
                .unwrap();
        }
        for id in ids {
            unload_for_producer(&self.engine, *id, &self.producer).unwrap();
        }
        self.drain_all();
    }
}

#[test]
#[ignore = "isolated real long-source cancellation and last-reader resource measurements"]
fn c3_productive_lifecycle_measurement() {
    Python::initialize();
    let python_runtime = python_runtime_identity();
    let config: Value = serde_json::from_slice(
        &fs::read(PathBuf::from(
            std::env::var_os("FLITZI_C3_LIFECYCLE_CONFIG").unwrap(),
        ))
        .unwrap(),
    )
    .unwrap();
    let root = PathBuf::from(config["project_root"].as_str().unwrap());
    let source = PathBuf::from(config["source"].as_str().unwrap());
    let source_before = full_hash(&source);
    let native_before = crate::audio_engine::c3_observation::snapshot();
    let running = measure(|| {
        println!("C3 lifecycle: worker-entry cancellation");
        let mut fixture = Fixture::new(root.join("running/samples"), &source);
        let request = fixture.admit(0);
        wait_until(Duration::from_secs(30), || {
            fixture.engine.loader_rx.lock().unwrap().try_iter().any(|event| matches!(event,LoaderEvent::Started{id:0,request_id,..} if request_id==request))
        });
        let cancellation = measure(|| {
            unload_for_producer(&fixture.engine, 0, &fixture.producer).unwrap();
            fixture.drain();
            fixture.settle();
            assert!(fixture.callback.mixer.bank_for_measurement()[0].is_none());
            assert!(fixture.engine.sample_cache.lock().unwrap()[0].is_none());
            let events = fixture.events();
            assert!(!events.iter().any(|event| event["kind"] == "success"));
            json!({"events":events,"request":request,"remaining_jobs":fixture.engine.cold_jobs.counts_for_test(),"native_source_absent":true})
        });
        fixture.engine.shut_down().unwrap();
        let cleanup = fixture.orphan_cleanup();
        json!({"cancel_after_actual_started":cancellation,"actual_orphan_cleanup":cleanup,"stage_limit":"worker-entry Started, no claimed decoder-specific checkpoint"})
    });
    let queued = measure(|| {
        println!("C3 lifecycle: queued cancellation");
        let mut fixture = Fixture::new(root.join("queued/samples"), &source);
        for id in 0..2 {
            let request = fixture.admit(id);
            fixture.pending(id, request);
        }
        let request = fixture.admit(2);
        assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 1, 3));
        let cancellation = measure(|| {
            unload_for_producer(&fixture.engine, 2, &fixture.producer).unwrap();
            fixture.drain();
            fixture.settle();
            let events = fixture.events();
            assert!(
                !events
                    .iter()
                    .any(|event| event["kind"] == "success" && event["id"] == 2)
            );
            assert!(fixture.callback.mixer.bank_for_measurement()[2].is_none());
            assert!((0..2).all(|id| fixture.callback.mixer.bank_for_measurement()[id].is_some()));
            json!({"request":request,"events":events,"other_assignments_acked":2,"remaining_jobs":fixture.engine.cold_jobs.counts_for_test()})
        });
        fixture.retire_loaded(&[0, 1]);
        fixture.engine.shut_down().unwrap();
        let cleanup = fixture.orphan_cleanup();
        json!({"actual_queued_unload":cancellation,"actual_peer_and_orphan_cleanup":cleanup})
    });
    let pending = measure(|| {
        println!("C3 lifecycle: pending ACK cancellation");
        let mut fixture = Fixture::new(root.join("pending/samples"), &source);
        let request = fixture.admit(0);
        fixture.pending(0, request);
        assert!(fixture.callback.mixer.bank_for_measurement()[0].is_none());
        let cancellation = measure(|| {
            unload_for_producer(&fixture.engine, 0, &fixture.producer).unwrap();
            fixture.drain();
            fixture.settle();
            let events = fixture.events();
            assert!(!events.iter().any(|event| event["kind"] == "success"));
            assert!(fixture.callback.mixer.bank_for_measurement()[0].is_none());
            json!({"request":request,"events":events,"stale_payload_rejected_at_actual_drain":true})
        });
        fixture.engine.shut_down().unwrap();
        let cleanup = fixture.orphan_cleanup();
        json!({"actual_pending_ack_unload":cancellation,"actual_orphan_cleanup":cleanup})
    });
    let shutdown = measure(|| {
        println!("C3 lifecycle: shutdown with 32 queued");
        let mut fixture = Fixture::new(root.join("shutdown/samples"), &source);
        for id in 0..2 {
            let request = fixture.admit(id);
            fixture.pending(id, request);
        }
        for id in 2..34 {
            fixture.admit(id);
        }
        assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (2, 32, 34));
        let cancellation = measure(|| {
            fixture.engine.shut_down().unwrap();
            assert_eq!(fixture.engine.cold_jobs.counts_for_test(), (0, 0, 0));
            assert!((0..34).all(|id| fixture.engine.cold_loading[id].load(Ordering::Acquire) == 0));
            fixture.drain_all();
            let events = fixture.events();
            assert!(!events.iter().any(|event| event["kind"] == "success"));
            assert!(!events.iter().any(|event| event["kind"] == "started"
                && event["id"].as_u64().is_some_and(|id| id >= 2)));
            assert!(
                fixture
                    .callback
                    .mixer
                    .bank_for_measurement()
                    .iter()
                    .all(Option::is_none)
            );
            json!({"actual_counts_before":[2,32,34],"actual_counts_after":[0,0,0],"events":events,"queued_jobs_never_started":true,"native_stale_payloads_rejected":true})
        });
        let cleanup = fixture.orphan_cleanup();
        json!({"actual_closed_admission_join":cancellation,"actual_orphan_cleanup":cleanup})
    });
    let last_reader = measure(|| {
        println!("C3 lifecycle: shared last-reader cleanup");
        let mut fixture = Fixture::new(root.join("readers/samples"), &source);
        for id in 0..2 {
            let request = fixture.admit(id);
            fixture.pending(id, request);
            fixture.drain();
            fixture.settle();
            assert!(
                fixture.engine.input_runtime_ownership.source_current(
                    id,
                    fixture.callback.mixer.bank_for_measurement()[id]
                        .as_ref()
                        .unwrap(),
                    48_000
                )
            );
        }
        let reader = CompleteSourceReader::capture(
            &fixture.engine,
            0,
            fixture.engine.sample_cache.lock().unwrap()[0]
                .clone()
                .unwrap(),
        )
        .unwrap();
        let (cache, originals) = {
            let leases = fixture.engine.cold_leases.lock().unwrap();
            assert_eq!(
                leases[0].as_ref().unwrap().cache_path,
                leases[1].as_ref().unwrap().cache_path
            );
            assert_ne!(
                leases[0].as_ref().unwrap().assignment_id(),
                leases[1].as_ref().unwrap().assignment_id()
            );
            assert_ne!(
                leases[0].as_ref().unwrap().original_path,
                leases[1].as_ref().unwrap().original_path
            );
            (
                leases[0].as_ref().unwrap().cache_path.clone(),
                vec![
                    leases[0].as_ref().unwrap().original_path.clone(),
                    leases[1].as_ref().unwrap().original_path.clone(),
                ],
            )
        };
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            reader.visit_region(0..128, &|| false, |first, values| {
                assert_eq!(first, 0);
                for (frame, actual) in values.chunks_exact(2).enumerate() {
                    let expected = (((frame * 211 + 37) % 16384) as i32 - 8192) as f32 / 32768.0;
                    assert_eq!(actual, [expected; 2]);
                }
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        for original in &originals {
            fixture
                .engine
                .project_assets
                .retire(&fixture.root, original, false)
                .unwrap();
        }
        unload_for_producer(&fixture.engine, 0, &fixture.producer).unwrap();
        fixture.drain();
        std::thread::sleep(Duration::from_millis(100));
        assert!(cache.join("playback.f32le").is_file());
        assert!(fixture.callback.mixer.bank_for_measurement()[1].is_some());
        unload_for_producer(&fixture.engine, 1, &fixture.producer).unwrap();
        fixture.drain();
        std::thread::sleep(Duration::from_millis(100));
        assert!(cache.join("playback.f32le").is_file());
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .open(cache.join("playback.f32le"))
                .is_err()
        );
        let held = process::snapshot();
        let cleanup = measure(|| {
            release_tx.send(()).unwrap();
            thread.join().unwrap().unwrap();
            wait_until(Duration::from_secs(60), || {
                !cache.exists() && originals.iter().all(|path| !path.exists())
            });
            json!({"all_owned_originals_removed":true,"shared_cache_removed":true,"external_frozen_source_preserved":true,
                "cleanup_status":fixture.engine.project_asset_cleanup_status().unwrap()})
        });
        fixture.engine.shut_down().unwrap();
        json!({"process_while_final_reader_pinned":held,"cache_held_after_first_unload":true,"cache_held_after_last_unload_until_reader_return":true,"actual_final_reader_cleanup":cleanup})
    });
    Python::attach(|_| {});
    assert_eq!(source_before, full_hash(&source));
    let native_after = crate::audio_engine::c3_observation::snapshot();
    assert_eq!(native_after.1, native_before.1);
    let report = json!({"schema":"c3-actual-lifecycle-v1","configuration":config,"source_sha256":source_before,
        "python_runtime":python_runtime,"native_runtime":process::runtime_identity().unwrap(),
        "running":running,"queued":queued,"pending_ack":pending,"shutdown":shutdown,"shared_last_reader":last_reader,
        "native_before":native_before,"native_after":native_after,
        "limits":"actual long120s source, bounded native workers and drains, no device; process RAM need not return to zero because allocators and services may retain arenas"});
    fs::write(
        PathBuf::from(std::env::var_os("FLITZI_C3_OUTPUT").unwrap()),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
