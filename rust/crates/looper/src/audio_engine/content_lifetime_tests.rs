use super::*;
use crate::audio_engine::audio_stream::process_control_message;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::input_runtime_binding::{self, InputRuntimePadBinding};
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::audio_engine::{AudioEngine, LoadedSourcePublication, publish_loaded_sample_with_arm};
use crate::messages::{ControlMessage, SampleBuffer, TriggerQuantization};
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use pyo3::Python;
use pyo3::types::{PyAnyMethods, PyDict, PyDictMethods};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicBool, AtomicU8};
use std::sync::{Arc, Mutex};

const RATE: u32 = 48_000;

struct Fixture {
    engine: AudioEngine,
    mixer: Box<RtMixer>,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    consumer: Consumer<ControlMessage>,
    sample: SampleBuffer,
    digest: String,
    next_generation: u64,
}

impl Fixture {
    fn new() -> Self {
        Python::initialize();
        let engine = AudioEngine::new().unwrap();
        let mut mixer = Box::new(RtMixer::new(1, RATE as f32));
        mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
        let (producer, consumer) = RingBuffer::new(4);
        Self {
            engine,
            mixer,
            producer: Arc::new(Mutex::new(producer)),
            consumer,
            sample: SampleBuffer {
                channels: 1,
                samples: Arc::from(vec![0.25; 128]),
                residency: None,
            }
            .with_complete_source(RATE),
            digest: "a".repeat(64),
            next_generation: 1,
        }
    }

    fn publish(
        &mut self,
        replacement: bool,
        arm: impl FnOnce() -> Result<(), String>,
    ) -> Result<Arc<AtomicU8>, String> {
        let generation = self.next_generation;
        self.next_generation += 1;
        let adoption = Arc::new(AtomicU8::new(0));
        let mut generations = self.engine.loaded_source_generations.lock().unwrap();
        let mut digests = self.engine.loaded_source_digests.lock().unwrap();
        publish_loaded_sample_with_arm(
            &self.producer,
            &self.engine.sample_cache,
            0,
            self.sample.clone(),
            LoadedSourcePublication {
                initial_mix: None,
                placement: self
                    .engine
                    .input_runtime_ownership
                    .slot_snapshot(0)
                    .unwrap(),
                ownership: &self.engine.input_runtime_ownership,
                generation,
                rate: RATE,
                generation_slot: &mut generations[0],
                digest_slot: &mut digests[0],
                digest: self.digest.clone(),
                cold: true,
                cold_epoch: None,
                cold_adoption: Some(adoption.clone()),
                replace_assignment: replacement,
                loop_region: None,
                resident_cancelled: None,
                incoming_defaults: None,
                initial_pitch: crate::audio_engine::SourcePitchInitialization::FreshNeutral,
                intent: None,
            },
            arm,
        )?;
        Ok(adoption)
    }

    fn drain(&mut self) {
        process_control_message(
            self.consumer.pop().unwrap(),
            &mut FixedCapacityScheduler::<8>::new(),
            0,
            &mut TriggerQuantization::Immediate,
            &mut TransportTimeline::new(RATE),
            &mut self.mixer,
            &mut Vec::new(),
            &mut ImmediateAudioBufferRetirement,
        );
    }

    fn binding(&self) -> InputRuntimePadBinding {
        input_runtime_binding::capture(&self.engine, 0)
            .unwrap()
            .unwrap()
    }

    fn lifetime(&self) -> u64 {
        self.engine.input_runtime_ownership.content_lifetime(0)
    }
}

#[test]
fn checked_allocator_never_wraps_or_recycles_exhausted_values() {
    let counter = AtomicU64::new(u64::MAX - 1);
    assert_eq!(reserve_from(&counter).unwrap(), u64::MAX - 1);
    assert_eq!(reserve_from(&counter), Err("content lifetime exhausted"));
    assert_eq!(counter.load(Ordering::Acquire), u64::MAX);
    assert_eq!(reserve_from(&counter), Err("content lifetime exhausted"));
    assert_eq!(counter.load(Ordering::Acquire), u64::MAX);
}

#[test]
fn assignment_reservation_is_fresh_but_same_content_maintenance_retains_lifetime() {
    let first = for_assignment(0, false).unwrap();
    let maintenance = for_assignment(first, false).unwrap();
    let replacement = for_assignment(first, true).unwrap();
    let copy = reserve().unwrap();
    assert_ne!(first, 0);
    assert_eq!(maintenance, first);
    assert_ne!(replacement, first);
    assert_ne!(copy, first);
    assert_ne!(copy, replacement);
}

#[test]
fn real_cold_publication_exposes_lifetime_before_own_ack_and_renews_only_assignment() {
    let mut fixture = Fixture::new();
    let first = fixture.publish(false, || Ok(())).unwrap();
    let lifetime = fixture.lifetime();
    let binding = fixture.binding();
    assert_ne!(lifetime, 0);
    assert!(binding.is_current());
    assert_eq!(first.load(Ordering::Acquire), 0);
    Python::attach(|py| {
        let value = binding.metadata(py).unwrap();
        let metadata = value.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            metadata
                .get_item("content_lifetime")
                .unwrap()
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            lifetime
        );
    });
    fixture.drain();
    assert_eq!(first.load(Ordering::Acquire), 2);
    assert!(fixture.mixer.bank_for_measurement()[0].is_some());

    let maintenance = fixture.publish(false, || Ok(())).unwrap();
    assert_eq!(fixture.lifetime(), lifetime);
    fixture.drain();
    assert_eq!(maintenance.load(Ordering::Acquire), 2);
    let replacement = fixture.publish(true, || Ok(())).unwrap();
    assert_ne!(fixture.lifetime(), lifetime);
    fixture.drain();
    assert_eq!(replacement.load(Ordering::Acquire), 2);
    assert!(Arc::ptr_eq(
        &fixture.sample.samples,
        &fixture.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
}

#[test]
fn prearm_rejection_leaves_the_existing_lifetime_source_and_binding_current() {
    let mut fixture = Fixture::new();
    fixture.publish(false, || Ok(())).unwrap();
    fixture.drain();
    let binding = fixture.binding();
    let lifetime = fixture.lifetime();
    let generation = fixture.engine.loaded_source_generations.lock().unwrap()[0];
    let armed = AtomicBool::new(false);
    let result = fixture.publish(true, || {
        armed.store(true, Ordering::Release);
        Err("source proposal arm rejected".into())
    });
    assert_eq!(result.unwrap_err(), "source proposal arm rejected");
    assert!(armed.load(Ordering::Acquire));
    assert_eq!(fixture.lifetime(), lifetime);
    assert_eq!(
        fixture.engine.loaded_source_generations.lock().unwrap()[0],
        generation
    );
    assert!(binding.is_current());
    assert!(fixture.consumer.is_empty());

    for _ in 0..4 {
        fixture
            .producer
            .lock()
            .unwrap()
            .push(ControlMessage::Ping())
            .unwrap();
    }
    armed.store(false, Ordering::Release);
    assert!(
        fixture
            .publish(true, || {
                armed.store(true, Ordering::Release);
                Ok(())
            })
            .is_err()
    );
    assert!(!armed.load(Ordering::Acquire));
    assert_eq!(fixture.lifetime(), lifetime);
    assert!(binding.is_current());
}

#[test]
fn different_complete_source_digest_renews_lifetime_without_stop_replacement() {
    let mut fixture = Fixture::new();
    fixture.publish(false, || Ok(())).unwrap();
    fixture.drain();
    let lifetime = fixture.lifetime();
    fixture.digest = "b".repeat(64);
    let adoption = fixture.publish(false, || Ok(())).unwrap();
    let renewed = fixture.lifetime();
    assert_ne!(renewed, lifetime);
    assert!(matches!(
        fixture.consumer.peek().unwrap(),
        ControlMessage::LoadColdSample {
            content_lifetime,
            replace_assignment: false,
            ..
        } if *content_lifetime == renewed
    ));
    fixture.drain();
    assert_eq!(adoption.load(Ordering::Acquire), 2);
    assert!(fixture.binding().is_current());
}

#[test]
fn fresh_native_owners_do_not_reuse_lifetime_for_equal_source_generation_and_digest() {
    let mut first = Fixture::new();
    let mut reopened = Fixture::new();
    first.publish(false, || Ok(())).unwrap();
    reopened.publish(false, || Ok(())).unwrap();
    assert_eq!(
        first.engine.loaded_source_generations.lock().unwrap()[0],
        (1, RATE)
    );
    assert_eq!(
        reopened.engine.loaded_source_generations.lock().unwrap()[0],
        (1, RATE)
    );
    assert_eq!(first.digest, reopened.digest);
    assert_ne!(first.lifetime(), reopened.lifetime());
    assert_ne!(first.lifetime(), 0);
    assert_ne!(reopened.lifetime(), 0);
    assert!(!Arc::ptr_eq(
        &first.engine.input_runtime_ownership,
        &reopened.engine.input_runtime_ownership
    ));
}

#[test]
fn changed_lifetime_rejects_queued_cold_work_even_with_unchanged_source_generation() {
    let mut fixture = Fixture::new();
    let adoption = fixture.publish(true, || Ok(())).unwrap();
    let binding = fixture.binding();
    let generation = fixture.engine.loaded_source_generations.lock().unwrap()[0];
    fixture
        .engine
        .input_runtime_ownership
        .publish_content_lifetime(0, reserve().unwrap());
    assert!(!binding.is_current());
    fixture.drain();
    assert_eq!(adoption.load(Ordering::Acquire), 3);
    assert!(fixture.mixer.bank_for_measurement()[0].is_none());
    assert_eq!(
        fixture.engine.loaded_source_generations.lock().unwrap()[0],
        generation
    );
}

#[test]
fn window_and_timing_authority_refreshes_do_not_change_content_lifetime() {
    let mut fixture = Fixture::new();
    fixture.publish(false, || Ok(())).unwrap();
    fixture.drain();
    let lifetime = fixture.lifetime();
    let ownership = &fixture.engine.input_runtime_ownership;
    ownership.publish_window(0, &fixture.sample);
    ownership.set_timing_intent(0, TimingIntent::Manual);
    ownership.revoke(0, ownership.next_authority(0).unwrap());
    assert_eq!(fixture.lifetime(), lifetime);
}
