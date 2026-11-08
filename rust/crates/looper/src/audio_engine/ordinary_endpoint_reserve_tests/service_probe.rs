//! Same-thread, counterbalanced service observations of the original live mixer.

use super::*;
use crate::audio_engine::buffer_retirement::close_callback_and_join;
use crate::audio_engine::native_lifecycle_observation::{self as lifecycle, NativeEventKind};
use crate::audio_engine::native_render_observation::thread_cpu::{Point, Timer};
use std::io::Write;
use std::os::windows::io::{AsRawHandle, IntoRawHandle, OwnedHandle};
use std::sync::Arc;

#[path = "dependency_variant.rs"]
mod dependency_variant;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

#[derive(Clone, Copy, serde::Serialize)]
pub(super) struct Cell {
    round: usize,
    slot: usize,
    pub(super) instrumented: bool,
}

pub(super) struct Context {
    pub(super) effect: u64,
    pub(super) original: serde_json::Value,
}

pub(super) struct State {
    cell: Cell,
    workers: Vec<(OwnedHandle, Point)>,
    native_addresses: Vec<usize>,
    owner: std::sync::Weak<crate::audio_engine::input_runtime_binding::InputRuntimeOwnership>,
}

impl State {
    pub(super) fn prepare(
        cell: Cell,
        fixture: &Fixture,
        starts: &[CapturedPadStart],
        wet: bool,
    ) -> Self {
        let mut handles = fixture.mixer.thread_handles_for_test();
        handles.extend(fixture.control.engine.cold_jobs.thread_handles_for_test());
        let workers = handles
            .into_iter()
            .map(|h| {
                let p = Point::worker(&h);
                (h, p)
            })
            .collect();
        let native_addresses: Vec<_> = if wet {
            let runtime = fixture.mixer.pitch_runtime.as_ref().unwrap();
            starts
                .iter()
                .map(|start| {
                    runtime
                        .ready_ordinary_native_address(start.sequence())
                        .unwrap()
                })
                .collect()
        } else {
            Vec::new()
        };
        assert!(
            native_addresses
                .iter()
                .enumerate()
                .all(|(id, &address)| address != 0 && !native_addresses[..id].contains(&address))
        );
        Self {
            cell,
            workers,
            native_addresses,
            owner: Arc::downgrade(&fixture.control.engine.input_runtime_ownership),
        }
    }

    pub(super) fn finish(
        self,
        mut fixture: Fixture,
        rows: &[Period],
        pcm: &[f32],
        sources: &[SampleBuffer],
        parity: &Option<Parity>,
        context: Context,
    ) {
        let Context {
            effect,
            original: metadata,
        } = context;
        let count = sources.len();
        assert_eq!(rows.len(), 1792);
        assert_eq!(pcm.len(), 172_032 * 2);
        assert!(parity.is_some());
        let steady: Vec<_> = rows
            .iter()
            .filter(|row| row.first >= effect + 8192)
            .collect();
        assert_eq!(steady.len(), 1706);
        let render_identity = rows[0]
            .render_thread
            .map(|span| (span.before.thread_id, span.before.birth_100ns));
        for row in rows {
            if self.cell.instrumented {
                let render = row.render_thread.unwrap();
                assert_eq!(
                    Some((render.before.thread_id, render.before.birth_100ns)),
                    render_identity
                );
            }
        }
        for row in &steady {
            for (voice, &address) in row.voices.iter().zip(&self.native_addresses) {
                assert_eq!(voice.native, address);
                assert_ne!(address, 0);
            }
            if self.cell.instrumented {
                let render = row.render_thread.unwrap();
                let shifts = row.native.shift_thread.unwrap();
                assert_eq!(shifts.count as u64, row.native.shifts);
                for shift in shifts.spans.into_iter().flatten() {
                    assert_eq!(
                        (shift.before.thread_id, shift.before.birth_100ns),
                        (render.before.thread_id, render.before.birth_100ns)
                    );
                }
            } else {
                assert!(row.render_thread.is_none() && row.native.shift_thread.is_none());
            }
        }
        let pcm_dir = std::path::PathBuf::from(
            std::env::var_os("FLITZIS_SERVICE_PCM_DIR")
                .expect("explicit diagnostic output directory"),
        );
        let path = pcm_dir.join(format!(
            "round{}-slot{}-cpu{}.pcm",
            self.cell.round, self.cell.slot, self.cell.instrumented
        ));
        let mut file = std::io::BufWriter::new(
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .unwrap(),
        );
        let mut full_pcm = Sha256::new();
        for sample in pcm {
            let bytes = sample.to_bits().to_le_bytes();
            file.write_all(&bytes).unwrap();
            full_pcm.update(bytes);
        }
        file.flush().unwrap();
        file.get_ref().sync_all().unwrap();
        drop(file);
        let source_hashes: Vec<_> = sources
            .iter()
            .map(|source| {
                let mut hash = Sha256::new();
                for sample in source.samples.iter() {
                    hash.update(sample.to_bits().to_le_bytes());
                }
                format!("{:x}", hash.finalize())
            })
            .collect();
        let ownership = &fixture.control.engine.input_runtime_ownership;
        let acks: Vec<_> = (0..count)
            .map(|id| {
                let ack = ownership.callback_feedback_key(id);
                assert!(ack.is_some());
                assert_eq!(ack, ownership.slot_snapshot(id).unwrap().content_key());
                ack
            })
            .collect();
        let stop_frame = fixture.transport.output_frame();
        for id in 0..count {
            fixture.control.engine.stop_sample(id).unwrap();
        }
        fixture.drain();
        fixture.render(96);
        fixture.mixer.pitch_runtime.as_mut().unwrap().poll();
        assert!(fixture.scheduler.is_empty());
        assert!((0..count).all(|id| !fixture.mixer.voices.iter().any(|v| v.is_playing_sample(id))));
        assert_eq!(
            fixture
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .unused_action_capacity(),
            (64, 64)
        );
        for (id, ack) in acks.into_iter().enumerate() {
            assert_eq!(
                fixture
                    .control
                    .engine
                    .input_runtime_ownership
                    .callback_feedback_key(id),
                ack
            );
        }
        fixture.control.engine.shut_down().unwrap();
        let Fixture {
            control,
            mixer,
            scheduler,
            transport,
            clock,
            feedback,
            retirement,
            _retirement_worker,
            ..
        } = fixture;
        close_callback_and_join(
            (
                control, mixer, scheduler, transport, clock, feedback, retirement,
            ),
            _retirement_worker,
        )
        .unwrap();
        assert!(self.owner.upgrade().is_none());
        let workers: Vec<_> = self
            .workers
            .into_iter()
            .map(|(handle, before)| {
                // SAFETY: each owned duplicate identifies the exact joined worker.
                assert_eq!(unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) }, 0);
                let after = Point::worker(&handle);
                assert_eq!(
                    (before.thread_id, before.birth_100ns),
                    (after.thread_id, after.birth_100ns)
                );
                assert_ne!(unsafe { CloseHandle(handle.into_raw_handle()) }, 0);
                serde_json::json!({"before":before,"after":after,"signaled":true,"closed":true})
            })
            .collect();
        let native = lifecycle::snapshot();
        assert!(!native.overflowed && native.incomplete == 0);
        let created: Vec<_> = native
            .events
            .iter()
            .filter(|e| e.kind == NativeEventKind::Created)
            .map(|e| e.native_id)
            .collect();
        let deleted: Vec<_> = native
            .events
            .iter()
            .filter(|e| e.kind == NativeEventKind::DeleteCompleted)
            .map(|e| e.native_id)
            .collect();
        assert_eq!(created.len(), deleted.len());
        assert!(
            created
                .iter()
                .all(|id| deleted.iter().filter(|d| *d == id).count() == 1)
        );
        eprintln!(
            "native_service_rows {}",
            serde_json::json!({
                "cell": self.cell, "pads":count,"wet":!self.native_addresses.is_empty(),
                "original":metadata,
                "rate":96000,"channels":2,"frames":96,"effect":effect,"steady_offset_min":8192,
                "prepared_native_addresses":self.native_addresses,"source_sha256":source_hashes,
                "pcm_path":path,"full_pcm_frames":pcm.len()/2,"full_pcm_sha256":format!("{:x}",full_pcm.finalize()),
                "parity":parity,"rows":rows,"workers":workers,"engine_owner_end":true,
                "native_created_ids":created,"native_delete_completed_ids":deleted,
                "stop_scheduler_empty":true,"source_ack_unchanged":true,"checked_engine_and_retirement_join":true,
                "stop_frame":stop_frame,
                "limits":"Test-only original render thread, paired raw cycles and quantized user/kernel100ns; query skew/overhead included, no cycles-to-time or exact wall-minus-CPU scheduler inference. No device/deadline/hearing/numerical-quality/optimization acceptance. Shared original PCM intentionally alive until matrix final drop; exact worker handle END does not certify nested ignored panic results or unrelated toolhosts."
            })
        );
    }
}

#[test]
#[ignore = "counterbalanced original 96k stereo96 same-thread CPU/cycle service matrix; isolated child"]
fn original_96_native_thread_service_matrix() {
    dependency_variant::assert_selected();
    lifecycle::enable();
    let empty: Vec<_> = (0..256).map(|_| Timer::begin().finish()).collect();
    let empty_ffi: Vec<_> = (0..256)
        .map(|_| {
            let scope = Scope::begin_service();
            let begin = crate::audio_engine::native_render_observation::shift_begin();
            crate::audio_engine::native_render_observation::shift_end(begin);
            scope.finish()
        })
        .collect();
    eprintln!(
        "native_service_empty {}",
        serde_json::json!({"spans":empty,"ffi_hooks":empty_ffi,"subtract_as_correction":false})
    );
    let originals = sources(96_000);
    let readers: Vec<_> = originals
        .iter()
        .map(|source| Arc::downgrade(&source.samples))
        .collect();
    let cohorts = [(2, true), (4, true), (6, true), (6, false)];
    let mut previous = std::collections::BTreeMap::new();
    for round in 0..2 {
        let order = if round == 0 {
            [0, 1, 2, 3]
        } else {
            [3, 2, 1, 0]
        };
        for slot in order {
            let (count, lock) = cohorts[slot];
            let sides = if (round + slot) % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            };
            let mut pair = Vec::new();
            for instrumented in sides {
                let workload = Workload {
                    rate: 96_000,
                    frames: 96,
                    periods: 1792,
                    require_loop_crossings: false,
                    measure_source: false,
                    service: Some(Cell {
                        round,
                        slot,
                        instrumented,
                    }),
                };
                pair.push(
                    case(&originals, workload, count, lock, 100)
                        .expect("qualified original service cell"),
                );
            }
            assert_eq!(pair[0], pair[1], "complete PCM and actual full trajectory");
            if let Some(prior) = previous.insert(slot, pair.remove(0)) {
                assert_eq!(prior, pair[0]);
            }
        }
    }
    drop(originals);
    assert!(readers.iter().all(|reader| reader.upgrade().is_none()));
    let empty_after: Vec<_> = (0..256).map(|_| Timer::begin().finish()).collect();
    eprintln!(
        "native_service_empty_after {}",
        serde_json::json!({"spans":empty_after,"subtract_as_correction":false})
    );
    eprintln!(
        "native_service_closed all16cells complete_pcm_trajectory_equal; all6originalPCMweakEND; originalSTOP/SourceACK/engine/worker/retirement/Native_deleteEND"
    );
}
