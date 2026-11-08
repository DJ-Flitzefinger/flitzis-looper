//! Real worker boundaries preserve ordering, endpoint priority and complete custody.
use super::*;
use crate::audio_engine::pitch_actions::{PitchActionPreparation, PreparedActionWork};
use crate::audio_engine::preparation_observation::{SpawnFailureProbe, with_workers};

fn readers(fixture: &Fixture) -> Vec<std::sync::Weak<[f32]>> {
    fixture
        .control
        .engine
        .sample_cache
        .lock()
        .unwrap()
        .iter()
        .take(2)
        .map(|sample| Arc::downgrade(&sample.as_ref().unwrap().samples))
        .collect()
}

#[test]
fn second_spawn_failure_joins_the_actual_first_worker() {
    let probe = SpawnFailureProbe::begin(1);
    let result = with_workers(2, || PitchActionPreparation::new(64));
    assert!(result.is_err());
    let handles = probe.finish();
    assert_eq!(
        handles.len(),
        1,
        "one real worker preceded the injected failure"
    );
    close_handles(handles);
}

#[test]
fn reversed_group_completion_keeps_original_order_and_global_endpoint_priority() {
    let mut fixture = with_workers(2, || fixture_96(2));
    let handles = fixture.mixer.thread_handles_for_test();
    assert_eq!(handles.len(), 3);
    let readers = readers(&fixture);
    fixture.refresh_clock();
    let captured = capture_playback_group(&fixture, 2);
    fixture.drain();
    let starts: Vec<_> = captured
        .resolve()
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|m| m.start)
        .collect();
    let gates: Vec<_> = starts.iter().map(WorkerGate::for_ordinary).collect();
    assert!(
        fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .unwrap()
            .is_some()
    );
    fixture.drain();
    for gate in &gates {
        gate.wait_for(false);
    }
    let observation = fixture
        .mixer
        .pitch_runtime
        .as_ref()
        .unwrap()
        .preparation_pool_observation();
    fixture.control.engine.select_input_pad(0).unwrap();
    fixture.drain();
    assert!(render96(&mut fixture).0.iter().all(|v| *v == 0.0));
    assert!(
        fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .selected_endpoint_request_queued(),
        "both workers are held and the actual singleton request cell must be occupied"
    );
    let before = observation.snapshot();
    // Worker zero can finish while the other original remains genuinely on worker one.
    gates[0].release();
    wait_ready(&mut fixture, &[starts[0].sequence()]);
    let deadline = Instant::now() + WORKER_WAIT;
    let checked = loop {
        let checked = observation.snapshot();
        if checked.0 > before.0 && checked.2 > 0 {
            break checked;
        }
        assert!(Instant::now() < deadline, "actual endpoint job boundary");
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(
        checked.1, before.1,
        "another shard's accepted owner fences endpoint work"
    );
    assert!(
        !fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .selected_endpoint_ready()
    );
    // A later genuinely prepared original must still wait for this live earlier group.
    fixture.refresh_clock();
    let third = capture(&fixture, 1.0);
    publish(&mut fixture, &third);
    wait_ready(&mut fixture, &[third.sequence()]);
    assert!(render96(&mut fixture).0.iter().all(|v| *v == 0.0));
    assert!(starts.iter().all(|s| s.effective_frame().is_none()));
    assert!(third.effective_frame().is_none());
    gates[1].release();
    wait_ready(
        &mut fixture,
        &starts
            .iter()
            .map(CapturedPadStart::sequence)
            .collect::<Vec<_>>(),
    );
    let execution = fixture.transport.output_frame();
    let (_, allocations) = observe_current_thread(|| render96(&mut fixture));
    assert_eq!(allocations, ThreadAllocations::default());
    assert!(
        starts
            .iter()
            .all(|s| s.effective_frame() == Some(execution))
    );
    assert!(third.effective_frame().is_none());
    render96(&mut fixture);
    assert_eq!(third.effective_frame(), Some(execution + 96));
    let deadline = Instant::now() + WORKER_WAIT;
    while observation.snapshot().1 == before.1 {
        fixture.mixer.pitch_runtime.as_mut().unwrap().poll();
        assert!(
            Instant::now() < deadline,
            "endpoint becomes eligible after accepted preparation"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.starts(0), 2);
    assert_eq!(fixture.starts(1), 1);
    drop(gates);
    drop(starts);
    drop(third);
    drop(captured);
    drop(fixture);
    close_handles(handles);
    assert!(readers.iter().all(|reader| reader.upgrade().is_none()));
}

#[test]
fn aggregate_cells_fall_back_and_retain_every_original_terminal_owner() {
    let fixture = fixture_96(2);
    let readers = readers(&fixture);
    let fixture_handles = fixture.mixer.thread_handles_for_test();
    let (mut pool, worker) = with_workers(2, || PitchActionPreparation::new(4)).unwrap();
    let handles = worker.thread_handles_for_test();
    assert_eq!(pool.preparation_slots(), 4);
    // These are genuine source-bound captures. This component test does not admit
    // a scheduler effect; unplanned Work must return intact as terminal rejection.
    let originals: Vec<_> = (0..7).map(|_| capture(&fixture, 1.0)).collect();
    let gates: Vec<_> = originals.iter().map(WorkerGate::for_ordinary).collect();
    let mut work: Vec<_> = originals
        .iter()
        .map(|s| s.take_work(None).unwrap().map(PreparedActionWork::Ordinary))
        .collect();
    for index in 0..2 {
        assert_eq!(pool.publish_prepared(&mut work[index]), Ok(true));
        gates[index].wait_for(false);
    }
    for value in work.iter_mut().take(6).skip(2) {
        assert_eq!(pool.publish_prepared(value), Ok(true));
    }
    assert_eq!(pool.preparation_slots(), 0);
    assert!(pool.publish_prepared(&mut work[6]).is_err());
    assert!(work[6].is_some());
    gates[1].release();
    gates[3].wait_for(false);
    assert_eq!(pool.preparation_slots(), 1);
    // The next preferred shard is full; the other shard has the sole real cell.
    assert_eq!(pool.publish_prepared(&mut work[6]), Ok(true));
    assert!(work[6].is_none());
    assert_eq!(pool.preparation_slots(), 0);
    for gate in &gates {
        gate.release();
    }
    let deadline = Instant::now() + WORKER_WAIT;
    let mut returned = Vec::new();
    while returned.len() < originals.len() {
        if let Some(work) = pool.poll_prepared() {
            returned.push(work);
        } else {
            assert!(Instant::now() < deadline, "all original owners return");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let mut sequences: Vec<_> = returned.iter().map(PreparedActionWork::sequence).collect();
    sequences.sort_unstable();
    let mut expected: Vec<_> = originals.iter().map(CapturedPadStart::sequence).collect();
    expected.sort_unstable();
    assert_eq!(sequences, expected);
    assert!(originals.iter().all(|s| s.state() == "rejected"));
    assert_eq!(pool.preparation_slots(), 4);
    drop(returned);
    drop(pool);
    drop(worker);
    close_handles(handles);
    drop(work);
    drop(gates);
    drop(originals);
    drop(fixture);
    close_handles(fixture_handles);
    assert!(readers.iter().all(|reader| reader.upgrade().is_none()));
}
