//! One owned spool worker. It never owns a callback/mixer/control join owner.

use super::spool::{Header, SPOOL_HEADER_BYTES, SpoolState};
use super::{Block, CaptureFailure, Shared};
use rtrb::{Consumer, Producer};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

#[derive(Default)]
pub(super) struct Hooks {
    // Deterministic fault seams for this module's actual File/ring worker only.
    #[cfg(test)]
    pub(super) gate: Option<Arc<std::sync::atomic::AtomicBool>>,
    #[cfg(test)]
    pub(super) fail_after_payload_bytes: Option<usize>,
    #[cfg(test)]
    pub(super) unpublished_sequence_gate: Option<Arc<WorkerPhaseGate>>,
    #[cfg(test)]
    pub(super) before_terminal_decision: Option<Arc<WorkerPhaseGate>>,
    #[cfg(test)]
    pub(super) after_terminal_decision: Option<Arc<WorkerPhaseGate>>,
    #[cfg(test)]
    pub(super) final_checkpoint_tail_failure: bool,
}

/// Worker-only deterministic interleaving seam. No hook or wait is present in
/// a production build or the callback's original block/publication path.
#[cfg(test)]
#[derive(Default)]
pub(super) struct WorkerPhaseGate {
    pub(super) reached: std::sync::atomic::AtomicBool,
    pub(super) released: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl WorkerPhaseGate {
    fn wait(&self) {
        self.reached.store(true, Ordering::Release);
        while !self.released.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn detail(shared: &Shared, reason: CaptureFailure, error: impl std::fmt::Display) {
    shared.worker_fail(reason);
    let mut saved = shared
        .detail
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if saved.is_none() {
        *saved = Some(error.to_string())
    }
}

struct Writer {
    file: File,
    header: Header,
    scratch: Vec<u8>,
    payload_hasher: Sha256,
    drained_blocks: u64,
    drained_end: Option<u64>,
    broken: bool,
    #[cfg(test)]
    payload_bytes_attempted: usize,
}

impl Writer {
    fn write_payload(&mut self, bytes: usize, hooks: &Hooks) -> io::Result<()> {
        #[cfg(not(test))]
        let _ = hooks;
        #[cfg(test)]
        if let Some(limit) = hooks.fail_after_payload_bytes {
            let remaining = limit.saturating_sub(self.payload_bytes_attempted);
            if remaining < bytes {
                self.file.write_all(&self.scratch[..remaining])?;
                self.payload_bytes_attempted += remaining;
                return Err(io::Error::other(
                    "injected partial capture payload write failure",
                ));
            }
            self.payload_bytes_attempted += bytes;
        }
        self.file.write_all(&self.scratch[..bytes])
    }

    fn block(&mut self, block: &Block, shared: &Shared, hooks: &Hooks) {
        if block.session_id != shared.session_id
            || self.drained_blocks.checked_add(1) != Some(block.sequence)
        {
            detail(
                shared,
                CaptureFailure::WriterProtocol,
                "foreign/out-of-order capture block",
            );
            self.broken = true;
            return;
        }
        // The producer's final scalar publication follows its queue push. Wait
        // off realtime for that exact sequence before exposing written progress.
        while shared.submitted_blocks.load(Ordering::Acquire) < block.sequence {
            #[cfg(test)]
            if let Some(gate) = &hooks.unpublished_sequence_gate {
                gate.wait();
            }
            if shared.sealed.load(Ordering::Acquire)
                && shared.submitted_blocks.load(Ordering::Acquire) < block.sequence
            {
                detail(
                    shared,
                    CaptureFailure::WriterProtocol,
                    "unpublished capture block at seal",
                );
                self.broken = true;
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        let config = shared.config;
        let Some(samples) = block.frames.checked_mul(config.channels) else {
            detail(
                shared,
                CaptureFailure::WriterProtocol,
                "capture block sample extent overflow",
            );
            self.broken = true;
            return;
        };
        let first = shared.first_frame.load(Ordering::Acquire);
        let expected = self.drained_end.unwrap_or(first);
        let end = block.first_frame.checked_add(block.frames as u64);
        if !shared.started.load(Ordering::Acquire)
            || block.frames == 0
            || block.frames > config.block_frames
            || samples > block.pcm.len()
            || samples * 4 > self.scratch.len()
            || block.first_frame != expected
            || end.is_none()
            || block.pcm[..samples]
                .iter()
                .any(|sample| !sample.is_finite())
        {
            detail(
                shared,
                CaptureFailure::WriterProtocol,
                "capture block/session/geometry is not contiguous",
            );
            self.broken = true;
            return;
        }
        self.drained_blocks = block.sequence;
        self.drained_end = end;
        if self.broken {
            return;
        }
        if shared.written_blocks.load(Ordering::Relaxed) == 0
            && let Err(error) =
                self.header
                    .checkpoint(&mut self.file, shared, SpoolState::RecordingIncomplete)
        {
            detail(shared, CaptureFailure::WriterIo, error);
            self.broken = true;
            return;
        }
        for (sample, encoded) in block.pcm[..samples]
            .iter()
            .zip(self.scratch.chunks_exact_mut(4))
        {
            encoded.copy_from_slice(&sample.to_bits().to_le_bytes());
        }
        if let Err(error) = self.write_payload(samples * 4, hooks) {
            detail(shared, CaptureFailure::WriterIo, error);
            self.broken = true;
            return;
        }
        // Hash exactly the original little-endian bytes after a complete write.
        // A partially failed block is never part of the published full payload.
        self.payload_hasher.update(&self.scratch[..samples * 4]);
        let Some(written) = shared
            .written_frames
            .load(Ordering::Relaxed)
            .checked_add(block.frames as u64)
        else {
            detail(
                shared,
                CaptureFailure::WriterProtocol,
                "capture written frames overflow",
            );
            self.broken = true;
            return;
        };
        shared.written_end.store(
            end.expect("checked contiguous block end"),
            Ordering::Relaxed,
        );
        shared.written_frames.store(written, Ordering::Relaxed);
        shared
            .written_blocks
            .store(block.sequence, Ordering::Release);
    }

    fn finish(mut self, shared: &Shared, hooks: &Hooks) {
        #[cfg(not(test))]
        let _ = hooks;
        let written = shared.written_frames.load(Ordering::Acquire);
        let submitted = shared.submitted_frames.load(Ordering::Acquire);
        if !self.broken
            && (written != submitted
                || self.drained_blocks != shared.submitted_blocks.load(Ordering::Acquire)
                || (shared.started.load(Ordering::Acquire)
                    && shared.written_end.load(Ordering::Acquire)
                        != shared.end_frame.load(Ordering::Acquire)))
        {
            detail(
                shared,
                CaptureFailure::WriterProtocol,
                "sealed capture extent was not fully written",
            );
            self.broken = true;
        }
        let length = written
            .checked_mul(shared.config.channels as u64)
            .and_then(|samples| samples.checked_mul(4))
            .and_then(|bytes| bytes.checked_add(SPOOL_HEADER_BYTES));
        let result = length
            .ok_or_else(|| io::Error::other("capture written extent overflow"))
            .and_then(|length| {
                // Remove an incomplete trailing write, preserving only full
                // successfully written blocks. Both header slots remain intact.
                let actual = self.file.metadata()?.len();
                if actual < length {
                    return Err(io::Error::other(
                        "capture spool lost part of its written prefix",
                    ));
                }
                if actual > length {
                    self.file.set_len(length)?;
                }
                self.file.flush()?;
                self.file.sync_all()?;
                if self.file.metadata()?.len() != length {
                    return Err(io::Error::other(
                        "capture spool length differs from written extent",
                    ));
                }
                Ok(())
            });
        let payload_synced = result.is_ok();
        if let Err(error) = result {
            detail(shared, CaptureFailure::WriterIo, error);
        }
        #[cfg(test)]
        if let Some(gate) = &hooks.before_terminal_decision {
            gate.wait();
        }
        // One atomic winner freezes control cancellation, while subsequent
        // writer I/O/panic can still add its first low failure code.
        shared
            .failure
            .fetch_or(super::FINAL_DECISION, Ordering::AcqRel);
        #[cfg(test)]
        if let Some(gate) = &hooks.after_terminal_decision {
            gate.wait();
        }
        if !self.broken && payload_synced && written > 0 && shared.failure_code() == 0 {
            let digest: [u8; 32] = self.payload_hasher.finalize().into();
            if shared.payload_sha256.set(digest).is_err() {
                detail(
                    shared,
                    CaptureFailure::WriterProtocol,
                    "capture payload witness was already published",
                );
            }
        }
        // A later checkpoint I/O error still fails this live session even when
        // the complete synced payload witness has already been published.
        let state = if shared.failure_code() == 0 {
            SpoolState::SealedCandidate
        } else {
            SpoolState::FailedIncomplete
        };
        let checkpoint = self.header.checkpoint(&mut self.file, shared, state);
        #[cfg(test)]
        let checkpoint = checkpoint.and_then(|()| {
            if hooks.final_checkpoint_tail_failure {
                Err(io::Error::other(
                    "injected final checkpoint tail I/O failure",
                ))
            } else {
                Ok(())
            }
        });
        if let Err(error) = checkpoint {
            detail(shared, CaptureFailure::WriterIo, error);
        }
        drop(self.file);
        shared.ready.store(false, Ordering::Release);
        shared.closed.store(true, Ordering::Release);
    }
}

pub(super) fn run(
    file: File,
    mut data: Consumer<Box<Block>>,
    mut free: Producer<Box<Block>>,
    scratch: Vec<u8>,
    shared: Arc<Shared>,
    hooks: Hooks,
) {
    let mut writer = Writer {
        file,
        header: Header::prepared(shared.session_id, shared.config),
        scratch,
        payload_hasher: Sha256::new(),
        drained_blocks: 0,
        drained_end: None,
        broken: false,
        #[cfg(test)]
        payload_bytes_attempted: 0,
    };
    shared.ready.store(true, Ordering::Release);
    #[cfg(test)]
    if let Some(gate) = &hooks.gate {
        while !gate.load(Ordering::Acquire) && !shared.sealed.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
    }
    loop {
        while let Ok(block) = data.pop() {
            writer.block(&block, &shared, &hooks);
            // Final block/ring destruction, including a defensive full return
            // ring, occurs here off realtime. The callback never drops a block.
            if let Err(rtrb::PushError::Full(block)) = free.push(block) {
                drop(block)
            }
        }
        if shared.sealed.load(Ordering::Acquire) {
            // Acquire the final producer publication before the empty check.
            // No new block may be published after the RT owner's seal.
            if data.is_empty() {
                break;
            }
            continue;
        }
        thread::sleep(Duration::from_millis(1));
    }
    writer.finish(&shared, &hooks);
}
