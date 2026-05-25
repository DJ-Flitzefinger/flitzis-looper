use crate::messages::{PreparedStemSet, SampleBuffer};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub(crate) const RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY: usize = 1024;
const RETIRED_AUDIO_BUFFER_BACKLOG_CAPACITY: usize = 128;

#[allow(dead_code)]
pub(crate) enum RetiredAudioBuffer {
    Sample(SampleBuffer),
    PreparedStems(PreparedStemSet),
}

pub(crate) trait AudioBufferRetirement {
    fn retire_sample(&mut self, sample: SampleBuffer);
    fn retire_prepared_stems(&mut self, stems: PreparedStemSet);
    fn available_retirement_slots(&mut self) -> usize;
}

#[cfg(test)]
pub(crate) struct ImmediateAudioBufferRetirement;

#[cfg(test)]
impl AudioBufferRetirement for ImmediateAudioBufferRetirement {
    fn retire_sample(&mut self, _sample: SampleBuffer) {}

    fn retire_prepared_stems(&mut self, _stems: PreparedStemSet) {}

    fn available_retirement_slots(&mut self) -> usize {
        usize::MAX
    }
}

pub(crate) struct RtAudioBufferRetirement {
    producer: Producer<RetiredAudioBuffer>,
    backlog: Box<[Option<RetiredAudioBuffer>; RETIRED_AUDIO_BUFFER_BACKLOG_CAPACITY]>,
    backlog_len: usize,
}

impl RtAudioBufferRetirement {
    fn new(producer: Producer<RetiredAudioBuffer>) -> Self {
        Self {
            producer,
            backlog: Box::new(std::array::from_fn(|_| None)),
            backlog_len: 0,
        }
    }

    fn retire_buffer(&mut self, buffer: RetiredAudioBuffer) {
        self.flush_backlog();

        match self.producer.push(buffer) {
            Ok(()) => {}
            Err(PushError::Full(buffer)) => {
                self.store_or_forget(buffer);
            }
        }
    }

    fn flush_backlog(&mut self) {
        if self.backlog_len == 0 {
            return;
        }

        for slot in self.backlog.iter_mut() {
            let Some(buffer) = slot.take() else {
                continue;
            };

            match self.producer.push(buffer) {
                Ok(()) => {
                    self.backlog_len -= 1;
                }
                Err(PushError::Full(buffer)) => {
                    *slot = Some(buffer);
                    return;
                }
            }
        }
    }

    fn store_or_forget(&mut self, buffer: RetiredAudioBuffer) {
        for slot in self.backlog.iter_mut() {
            if slot.is_none() {
                *slot = Some(buffer);
                self.backlog_len += 1;
                return;
            }
        }

        // Avoid a large final Arc drop on the callback thread even if the retirement path is
        // saturated. The queue/backlog are sized to make this a last-resort failure mode.
        std::mem::forget(buffer);
    }
}

impl AudioBufferRetirement for RtAudioBufferRetirement {
    fn retire_sample(&mut self, sample: SampleBuffer) {
        self.retire_buffer(RetiredAudioBuffer::Sample(sample));
    }

    fn retire_prepared_stems(&mut self, stems: PreparedStemSet) {
        self.retire_buffer(RetiredAudioBuffer::PreparedStems(stems));
    }

    fn available_retirement_slots(&mut self) -> usize {
        self.flush_backlog();
        self.producer
            .slots()
            .saturating_add(RETIRED_AUDIO_BUFFER_BACKLOG_CAPACITY - self.backlog_len)
    }
}

pub(crate) struct AudioBufferRetirementWorker {
    running: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl AudioBufferRetirementWorker {
    fn spawn(mut consumer: Consumer<RetiredAudioBuffer>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let join_handle = thread::Builder::new()
            .name("flitzis-audio-buffer-retirement".to_string())
            .spawn(move || {
                while thread_running.load(Ordering::Acquire) || !consumer.is_empty() {
                    let mut drained = false;
                    while let Ok(buffer) = consumer.pop() {
                        drop(buffer);
                        drained = true;
                    }

                    if !drained {
                        thread::sleep(Duration::from_millis(5));
                    }
                }
            })
            .ok();

        Self {
            running,
            join_handle,
        }
    }
}

impl Drop for AudioBufferRetirementWorker {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

pub(crate) fn create_audio_buffer_retirement()
-> (RtAudioBufferRetirement, AudioBufferRetirementWorker) {
    let (producer, consumer) = RingBuffer::new(RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY);
    (
        RtAudioBufferRetirement::new(producer),
        AudioBufferRetirementWorker::spawn(consumer),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn sample_with_weak() -> (SampleBuffer, std::sync::Weak<[f32]>) {
        let samples: Arc<[f32]> = Arc::from(vec![0.0_f32; 16].into_boxed_slice());
        let weak = Arc::downgrade(&samples);
        (
            SampleBuffer {
                channels: 1,
                samples,
            },
            weak,
        )
    }

    #[test]
    fn retirement_queue_holds_sample_until_consumer_drops_it() {
        let (producer, mut consumer) = RingBuffer::new(1);
        let mut retirement = RtAudioBufferRetirement::new(producer);
        let (sample, weak) = sample_with_weak();

        retirement.retire_sample(sample);

        assert!(weak.upgrade().is_some());

        let retired = consumer.pop().unwrap();
        drop(retired);

        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn full_retirement_queue_uses_preallocated_backlog() {
        let (producer, _consumer) = RingBuffer::new(0);
        let mut retirement = RtAudioBufferRetirement::new(producer);
        let (sample, weak) = sample_with_weak();

        retirement.retire_sample(sample);

        assert!(weak.upgrade().is_some());
        assert_eq!(retirement.backlog_len, 1);
    }
}
