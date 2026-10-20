//! Bounded native mix capture; scheduler/mixer integration remains the caller's authority.
//!
//! Preparation, file ownership and joins are control-side. The callback moves only
//! preallocated blocks and scalar status. A terminal `Box<CaptureRt>` MUST leave
//! realtime through the existing retirement/CallbackCloseOwner boundary; neither
//! a failed queue push nor stop destroys its last heavy owner. The internal raw
//! f32le spool is not a completed performer recording or an encoded file.

use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};

pub(crate) mod publication;
mod spool;
mod writer;
#[cfg(test)]
pub(crate) use spool::SPOOL_HEADER_BYTES;
pub(crate) use spool::{SpoolInfo, SpoolState, inspect_spool};
#[cfg(test)]
mod tests;

pub(crate) const MAX_POOL_PCM_BYTES: usize = 64 * 1024 * 1024;
const MAX_POOL_BLOCKS: usize = 4096;
const MAX_BLOCK_FRAMES: usize = 65_536;
const START_CLAIMED: u8 = 0x40;
const FINAL_DECISION: u8 = 0x80;
const FAILURE_MASK: u8 = 0x3f;
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CaptureConfig {
    pub(crate) sample_rate_hz: u32,
    pub(crate) channels: usize,
    pub(crate) block_frames: usize,
    pub(crate) block_count: usize,
    /// PCM payload admission only; fixed ring/header/scratch allocations are separate.
    pub(crate) max_pool_pcm_bytes: usize,
}

impl CaptureConfig {
    fn sizes(self) -> io::Result<(usize, usize)> {
        let invalid =
            || io::Error::new(io::ErrorKind::InvalidInput, "invalid capture geometry/pool");
        if self.sample_rate_hz == 0
            || self.channels == 0
            || self.channels > u16::MAX as usize
            || self.block_frames == 0
            || self.block_frames > MAX_BLOCK_FRAMES
            || self.block_count == 0
            || self.block_count > MAX_POOL_BLOCKS
            || self.max_pool_pcm_bytes == 0
            || self.max_pool_pcm_bytes > MAX_POOL_PCM_BYTES
        {
            return Err(invalid());
        }
        let samples = self
            .block_frames
            .checked_mul(self.channels)
            .ok_or_else(invalid)?;
        let bytes = samples.checked_mul(size_of::<f32>()).ok_or_else(invalid)?;
        let pool_bytes = bytes.checked_mul(self.block_count).ok_or_else(invalid)?;
        if pool_bytes > self.max_pool_pcm_bytes {
            return Err(invalid());
        }
        Ok((samples, pool_bytes))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum CaptureFailure {
    Cancelled = 1,
    Abandoned = 2,
    Overflow = 3,
    NonFinite = 4,
    Discontinuous = 5,
    FrameOverflow = 6,
    Geometry = 7,
    NotReady = 8,
    WriterIo = 9,
    WriterProtocol = 10,
    Stream = 11,
    Rejected = 12,
    WorkerPanicked = 13,
}

impl CaptureFailure {
    pub(crate) fn code(self) -> u8 {
        self as u8
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Abandoned => "abandoned",
            Self::Overflow => "overflow",
            Self::NonFinite => "non_finite",
            Self::Discontinuous => "discontinuous",
            Self::FrameOverflow => "frame_overflow",
            Self::Geometry => "geometry",
            Self::NotReady => "not_ready",
            Self::WriterIo => "writer_io",
            Self::WriterProtocol => "writer_protocol",
            Self::Stream => "stream",
            Self::Rejected => "rejected",
            Self::WorkerPanicked => "worker_panicked",
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code & FAILURE_MASK {
            0 => None,
            1 => Some(Self::Cancelled),
            2 => Some(Self::Abandoned),
            3 => Some(Self::Overflow),
            4 => Some(Self::NonFinite),
            5 => Some(Self::Discontinuous),
            6 => Some(Self::FrameOverflow),
            7 => Some(Self::Geometry),
            8 => Some(Self::NotReady),
            9 => Some(Self::WriterIo),
            10 => Some(Self::WriterProtocol),
            11 => Some(Self::Stream),
            12 => Some(Self::Rejected),
            _ => Some(Self::WorkerPanicked),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CaptureStatus {
    pub(crate) session_id: u64,
    pub(crate) config: CaptureConfig,
    pub(crate) writer_ready: bool,
    pub(crate) started: bool,
    pub(crate) rt_sealed: bool,
    pub(crate) rt_released: bool,
    pub(crate) worker_closed: bool,
    pub(crate) worker_joined: bool,
    pub(crate) terminal_decided: bool,
    pub(crate) failure: Option<CaptureFailure>,
    /// Original worker's synced payload bytes, never a recovery-header permit.
    pub(crate) payload_sha256: Option<[u8; 32]>,
    pub(crate) first_frame: Option<u64>,
    pub(crate) captured_end: Option<u64>,
    pub(crate) buffered_frames: u64,
    pub(crate) submitted_end: Option<u64>,
    pub(crate) written_end: Option<u64>,
    pub(crate) end_frame: Option<u64>,
    pub(crate) submitted_frames: u64,
    pub(crate) written_frames: u64,
    pub(crate) submitted_blocks: u64,
    pub(crate) written_blocks: u64,
    pub(crate) allocated_blocks: usize,
    pub(crate) pool_pcm_bytes: usize,
    pub(crate) writer_scratch_bytes: usize,
}

impl CaptureStatus {
    pub(crate) fn empty(&self) -> bool {
        self.rt_sealed && self.submitted_frames == 0
    }

    /// Closed contiguous float spool only, never selected-codec/public-file success.
    pub(crate) fn successful_spool(&self) -> bool {
        self.started
            && self.rt_sealed
            && self.worker_closed
            && self.failure.is_none()
            && self.payload_sha256.is_some()
            && !self.empty()
            && self.submitted_frames == self.written_frames
            && self.end_frame == self.written_end
    }
}

struct Shared {
    session_id: u64,
    config: CaptureConfig,
    pool_pcm_bytes: usize,
    writer_scratch_bytes: usize,
    ready: AtomicBool,
    started: AtomicBool,
    sealed: AtomicBool,
    rt_released: AtomicBool,
    closed: AtomicBool,
    joined: AtomicBool,
    failure: AtomicU8,
    // Published once by the original worker, before final closure. RT never
    // computes or sets this witness; observation copies fixed bytes only.
    payload_sha256: OnceLock<[u8; 32]>,
    first_frame: AtomicU64,
    captured_end: AtomicU64,
    buffered_frames: AtomicU64,
    submitted_end: AtomicU64,
    written_end: AtomicU64,
    end_frame: AtomicU64,
    submitted_frames: AtomicU64,
    written_frames: AtomicU64,
    submitted_blocks: AtomicU64,
    written_blocks: AtomicU64,
    // Only the worker/control observer accesses this mutex, never CaptureRt methods.
    detail: Mutex<Option<String>>,
}

impl Shared {
    fn fail(&self, reason: CaptureFailure) -> bool {
        match self
            .failure
            .compare_exchange(0, reason as u8, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => true,
            Err(START_CLAIMED) => {
                // The only eligible transition missed by the first strong CAS
                // is the one start permission claim. Retry once preserving it.
                // Every other writer adds failure/final bits permanently, so
                // no retry loop or callback wait is necessary.
                match self.failure.compare_exchange(
                    START_CLAIMED,
                    START_CLAIMED | reason as u8,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => true,
                    Err(previous) => previous & FINAL_DECISION == 0,
                }
            }
            Err(previous) => previous & FINAL_DECISION == 0,
        }
    }

    /// Only writer I/O/panic may add the first failure after its decision.
    fn worker_fail(&self, reason: CaptureFailure) {
        let _ = self
            .failure
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |word| {
                (word & FAILURE_MASK == 0).then_some(word | reason as u8)
            });
    }

    fn failure_code(&self) -> u8 {
        self.failure.load(Ordering::Acquire) & FAILURE_MASK
    }

    fn snapshot(&self) -> CaptureStatus {
        // Final closure is published after file drop; RT sealing is published after
        // its final extent. Active progress fields are monotone observations, not
        // one fabricated atomic image of both independently advancing threads.
        let worker_closed = self.closed.load(Ordering::Acquire);
        let rt_sealed = self.sealed.load(Ordering::Acquire);
        let started = self.started.load(Ordering::Acquire);
        let written_blocks = self.written_blocks.load(Ordering::Acquire);
        let written_frames = self.written_frames.load(Ordering::Acquire);
        let written_end = self.written_end.load(Ordering::Acquire);
        let submitted_blocks = self.submitted_blocks.load(Ordering::Acquire);
        let failure_word = self.failure.load(Ordering::Acquire);
        let failure = CaptureFailure::from_code(failure_word);
        let terminal_decided = failure_word & FINAL_DECISION != 0;
        CaptureStatus {
            session_id: self.session_id,
            config: self.config,
            writer_ready: self.ready.load(Ordering::Acquire)
                && !worker_closed
                && !terminal_decided
                && failure.is_none(),
            started,
            rt_sealed,
            rt_released: self.rt_released.load(Ordering::Acquire),
            worker_closed,
            worker_joined: self.joined.load(Ordering::Acquire),
            terminal_decided,
            failure,
            payload_sha256: self.payload_sha256.get().copied(),
            first_frame: started.then(|| self.first_frame.load(Ordering::Acquire)),
            captured_end: started.then(|| self.captured_end.load(Ordering::Acquire)),
            buffered_frames: self.buffered_frames.load(Ordering::Acquire),
            submitted_end: started.then(|| self.submitted_end.load(Ordering::Acquire)),
            written_end: started.then_some(written_end),
            end_frame: (started && rt_sealed).then(|| self.end_frame.load(Ordering::Acquire)),
            submitted_frames: self.submitted_frames.load(Ordering::Acquire),
            written_frames,
            submitted_blocks,
            written_blocks,
            allocated_blocks: self.config.block_count,
            pool_pcm_bytes: self.pool_pcm_bytes,
            writer_scratch_bytes: self.writer_scratch_bytes,
        }
    }
}

/// Observation only. Dropping a UI ticket cannot revoke a live engine-owned take.
#[derive(Clone)]
pub(crate) struct CaptureObserver {
    shared: Arc<Shared>,
}

impl CaptureObserver {
    pub(crate) fn status(&self) -> CaptureStatus {
        self.shared.snapshot()
    }

    pub(crate) fn error_detail(&self) -> Option<String> {
        self.shared
            .detail
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

/// Engine/control-side join owner; retain until the RT image is sealed/retired.
pub(crate) struct CaptureControl {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    spool_path: PathBuf,
}

impl CaptureControl {
    pub(crate) fn observer(&self) -> CaptureObserver {
        CaptureObserver {
            shared: self.shared.clone(),
        }
    }

    pub(crate) fn status(&self) -> CaptureStatus {
        self.shared.snapshot()
    }

    pub(crate) fn spool_path(&self) -> &Path {
        &self.spool_path
    }

    pub(crate) fn request_failure(&self, session_id: u64, reason: CaptureFailure) -> bool {
        if session_id != self.shared.session_id || self.shared.closed.load(Ordering::Acquire) {
            return false;
        }
        self.shared.fail(reason)
    }

    /// Never waits for an active callback or an in-progress filesystem operation.
    /// A caller-owned bounded shutdown poll decides when to retain incomplete data.
    pub(crate) fn join_if_finished(&mut self) -> io::Result<bool> {
        let Some(worker) = self.worker.as_ref() else {
            return Ok(true);
        };
        if !worker.is_finished() {
            return Ok(false);
        }
        let worker = self.worker.take().expect("finished worker retained above");
        if worker.join().is_err() {
            self.shared.worker_fail(CaptureFailure::WorkerPanicked);
            self.shared.ready.store(false, Ordering::Release);
            self.shared.closed.store(true, Ordering::Release);
            self.shared.joined.store(true, Ordering::Release);
            return Err(io::Error::other(
                "mix capture writer panicked; spool is incomplete",
            ));
        }
        self.shared.joined.store(true, Ordering::Release);
        Ok(true)
    }
}

impl Drop for CaptureControl {
    fn drop(&mut self) {
        if !self.shared.closed.load(Ordering::Acquire) {
            self.shared.fail(CaptureFailure::Abandoned);
        }
        // A live RT owner must first acknowledge/seal its extent. Waiting for it
        // here would deadlock callback/stream teardown. Engine keeps this owner
        // through normal settlement; exceptional abandonment detaches the thread.
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            let _ = self.join_if_finished();
        }
    }
}

struct Block {
    session_id: u64,
    sequence: u64,
    first_frame: u64,
    frames: usize,
    pcm: Box<[f32]>,
}

struct RtReleased(Arc<Shared>);

impl Drop for RtReleased {
    fn drop(&mut self) {
        // Last declared field: all RT-side queue endpoints/retained PCM have
        // actually dropped before admission may regard this owner as released.
        self.0.rt_released.store(true, Ordering::Release);
    }
}

/// One original RT owner's start permission, never an effective-frame ACK.
/// Its exclusive borrow prevents foreign publication, intervening RT mutation
/// or duplicate publication. A combined effect may only perform infallible
/// stores in disjoint state before consuming this capability.
#[must_use]
pub(crate) struct CaptureStartClaim<'a> {
    owner: &'a mut CaptureRt,
    frame: u64,
}

impl CaptureStartClaim<'_> {
    /// Publish infallible stores to the exact retained owner, consuming permission.
    /// A later shared failure cannot revoke the accepted effect. The Release ACK
    /// follows every preceding effect store; no fallible check follows the claim.
    pub(crate) fn publish(self) {
        let owner = self.owner;
        let frame = self.frame;
        owner.next_frame = frame;
        owner.submitted_end = frame;
        owner.started = true;
        owner.shared.first_frame.store(frame, Ordering::Relaxed);
        owner.shared.captured_end.store(frame, Ordering::Relaxed);
        owner.shared.submitted_end.store(frame, Ordering::Relaxed);
        owner.shared.written_end.store(frame, Ordering::Relaxed);
        owner.shared.started.store(true, Ordering::Release);
    }
}

/// Unique callback work. Caller MUST retire this complete Box off realtime.
pub(crate) struct CaptureRt {
    shared: Arc<Shared>,
    data: Producer<Box<Block>>,
    free: Consumer<Box<Block>>,
    // Adjacent short segments share this allocation. A defensive failed push
    // retains the same original block here until the existing off-RT retirement.
    partial: Option<Box<Block>>,
    started: bool,
    sealed: bool,
    next_frame: u64,
    submitted_end: u64,
    submitted_frames: u64,
    submitted_blocks: u64,
    _released: RtReleased,
}

impl CaptureRt {
    pub(crate) fn session_id(&self) -> u64 {
        self.shared.session_id
    }
    pub(crate) fn config(&self) -> CaptureConfig {
        self.shared.config
    }
    pub(crate) fn terminal(&self) -> bool {
        self.sealed
    }

    pub(crate) fn start(
        &mut self,
        session_id: u64,
        frame: u64,
        actual_rate_hz: u32,
        actual_channels: usize,
    ) -> bool {
        let Some(claim) = self.try_claim_start(session_id, frame, actual_rate_hz, actual_channels)
        else {
            return false;
        };
        claim.publish();
        true
    }

    /// All fallible geometry/readiness checks precede the single permission CAS.
    /// Control/worker failure winning first refuses the claim. Later failure
    /// remains authoritative but cannot undo this accepted effect permission.
    pub(crate) fn try_claim_start(
        &mut self,
        session_id: u64,
        frame: u64,
        actual_rate_hz: u32,
        actual_channels: usize,
    ) -> Option<CaptureStartClaim<'_>> {
        if session_id != self.session_id() || self.started || self.sealed {
            return None;
        }
        if self.observe_failure() {
            return None;
        }
        if (actual_rate_hz, actual_channels)
            != (
                self.shared.config.sample_rate_hz,
                self.shared.config.channels,
            )
        {
            self.fail(session_id, CaptureFailure::Geometry);
            return None;
        }
        if !self.shared.ready.load(Ordering::Acquire) {
            self.fail(session_id, CaptureFailure::NotReady);
            return None;
        }
        if self
            .shared
            .failure
            .compare_exchange(0, START_CLAIMED, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            self.observe_failure();
            return None;
        }
        Some(CaptureStartClaim { owner: self, frame })
    }

    /// Copy only the already rendered final mix. No source, clock or mixer mutation.
    pub(crate) fn append_segment(&mut self, first_frame: u64, pcm: &[f32]) -> bool {
        if self.sealed || self.observe_failure() {
            return false;
        }
        if !self.started {
            return false;
        }
        let config = self.shared.config;
        if !pcm.len().is_multiple_of(config.channels) {
            self.fail(self.session_id(), CaptureFailure::Geometry);
            return false;
        }
        if first_frame != self.next_frame {
            self.fail(self.session_id(), CaptureFailure::Discontinuous);
            return false;
        }
        let frames = pcm.len() / config.channels;
        if first_frame.checked_add(frames as u64).is_none() {
            self.fail(self.session_id(), CaptureFailure::FrameOverflow);
            return false;
        }
        let mut offset = 0;
        while offset < pcm.len() {
            if self.observe_failure() {
                return false;
            }
            let held_frames = self.partial.as_ref().map_or(0, |block| block.frames);
            let capacity = (config.block_frames - held_frames) * config.channels;
            let count = (pcm.len() - offset).min(capacity);
            let chunk = &pcm[offset..offset + count];
            if chunk.iter().any(|sample| !sample.is_finite()) {
                self.fail(self.session_id(), CaptureFailure::NonFinite);
                return false;
            }
            if self.partial.is_none() {
                let Ok(mut block) = self.free.pop() else {
                    self.fail(self.session_id(), CaptureFailure::Overflow);
                    return false;
                };
                block.session_id = self.session_id();
                block.first_frame = self.next_frame;
                block.frames = 0;
                self.partial = Some(block);
            }
            let chunk_frames = chunk.len() / config.channels;
            let Some(end) = self.next_frame.checked_add(chunk_frames as u64) else {
                self.fail(self.session_id(), CaptureFailure::FrameOverflow);
                return false;
            };
            let block = self.partial.as_mut().expect("owned partial capture block");
            let start = block.frames * config.channels;
            block.pcm[start..start + chunk.len()].copy_from_slice(chunk);
            block.frames += chunk_frames;
            self.next_frame = end;
            self.shared.captured_end.store(end, Ordering::Release);
            self.shared
                .buffered_frames
                .store(block.frames as u64, Ordering::Release);
            offset += chunk.len();
            if block.frames == config.block_frames && !self.flush_partial() {
                self.seal();
                return false;
            }
        }
        true
    }

    fn flush_partial(&mut self) -> bool {
        let Some(mut block) = self.partial.take() else {
            return true;
        };
        let Some(sequence) = self.submitted_blocks.checked_add(1) else {
            self.partial = Some(block);
            self.shared.fail(CaptureFailure::FrameOverflow);
            return false;
        };
        let Some(total_frames) = self.submitted_frames.checked_add(block.frames as u64) else {
            self.partial = Some(block);
            self.shared.fail(CaptureFailure::FrameOverflow);
            return false;
        };
        let Some(end) = block.first_frame.checked_add(block.frames as u64) else {
            self.partial = Some(block);
            self.shared.fail(CaptureFailure::FrameOverflow);
            return false;
        };
        block.sequence = sequence;
        if let Err(PushError::Full(block)) = self.data.push(block) {
            self.partial = Some(block);
            self.shared.fail(CaptureFailure::Overflow);
            return false;
        }
        self.submitted_end = end;
        self.submitted_frames = total_frames;
        self.submitted_blocks = sequence;
        self.shared.submitted_end.store(end, Ordering::Relaxed);
        self.shared
            .submitted_frames
            .store(total_frames, Ordering::Relaxed);
        self.shared.buffered_frames.store(0, Ordering::Relaxed);
        // Final scalar publication gates this block's worker processing.
        self.shared
            .submitted_blocks
            .store(sequence, Ordering::Release);
        true
    }

    pub(crate) fn stop(&mut self, session_id: u64, end_frame: u64) -> bool {
        if session_id != self.session_id() || !self.started || self.sealed {
            return false;
        }
        if end_frame != self.next_frame {
            self.fail(session_id, CaptureFailure::Discontinuous);
            return false;
        }
        self.seal()
    }

    pub(crate) fn fail(&mut self, session_id: u64, reason: CaptureFailure) -> bool {
        if session_id != self.session_id() || self.sealed {
            return false;
        }
        self.shared.fail(reason);
        self.seal();
        true
    }

    /// Call at callback service/tap even when no PCM is rendered, to seal a
    /// control/worker/stream failure without requiring a future nonempty segment.
    pub(crate) fn observe_failure(&mut self) -> bool {
        if self.shared.failure_code() != 0 {
            self.seal();
            true
        } else {
            false
        }
    }

    fn seal(&mut self) -> bool {
        if !self.sealed {
            let flushed = self.flush_partial();
            self.sealed = true;
            self.shared
                .end_frame
                .store(self.submitted_end, Ordering::Relaxed);
            self.shared.sealed.store(true, Ordering::Release);
            return flushed;
        }
        true
    }
}

impl Drop for CaptureRt {
    fn drop(&mut self) {
        // This destructor belongs to the existing non-RT retirement boundary.
        // It wakes the writer's terminal observation before rings/blocks drop.
        if !self.sealed {
            self.shared.fail(CaptureFailure::Abandoned);
            self.seal();
        }
    }
}

pub(crate) fn prepare(
    spool_path: &Path,
    config: CaptureConfig,
) -> io::Result<(CaptureControl, Box<CaptureRt>)> {
    prepare_inner(spool_path, config, writer::Hooks::default())
}

fn prepare_inner(
    spool_path: &Path,
    config: CaptureConfig,
    hooks: writer::Hooks,
) -> io::Result<(CaptureControl, Box<CaptureRt>)> {
    let (samples, pool_pcm_bytes) = config.sizes()?;
    if !spool_path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "capture spool path must be absolute",
        ));
    }
    let session_id = NEXT_SESSION
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_add(1)
        })
        .map_err(|_| io::Error::other("mix capture session identity exhausted"))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1); // Allow reads, deny foreign writes/deletion while open.
    }
    let mut file = options.open(spool_path)?;
    spool::Header::prepared(session_id, config).initialize(&mut file)?;
    let (data, data_consumer) = RingBuffer::new(config.block_count);
    let (mut free_producer, free) = RingBuffer::new(config.block_count);
    for _ in 0..config.block_count {
        let mut pcm = Vec::new();
        pcm.try_reserve_exact(samples).map_err(io::Error::other)?;
        pcm.resize(samples, 0.0);
        let block = Box::new(Block {
            session_id,
            sequence: 0,
            first_frame: 0,
            frames: 0,
            pcm: pcm.into_boxed_slice(),
        });
        if free_producer.push(block).is_err() {
            return Err(io::Error::other(
                "mix capture free pool initialization failed",
            ));
        }
    }
    let scratch_bytes = samples
        .checked_mul(size_of::<f32>())
        .ok_or_else(|| io::Error::other("mix capture scratch size overflow"))?;
    let mut scratch = Vec::new();
    scratch
        .try_reserve_exact(scratch_bytes)
        .map_err(io::Error::other)?;
    scratch.resize(scratch_bytes, 0_u8);
    let shared = Arc::new(Shared {
        session_id,
        config,
        pool_pcm_bytes,
        writer_scratch_bytes: scratch_bytes,
        ready: AtomicBool::new(false),
        started: AtomicBool::new(false),
        sealed: AtomicBool::new(false),
        rt_released: AtomicBool::new(false),
        closed: AtomicBool::new(false),
        joined: AtomicBool::new(false),
        failure: AtomicU8::new(0),
        payload_sha256: OnceLock::new(),
        first_frame: AtomicU64::new(0),
        captured_end: AtomicU64::new(0),
        buffered_frames: AtomicU64::new(0),
        submitted_end: AtomicU64::new(0),
        written_end: AtomicU64::new(0),
        end_frame: AtomicU64::new(0),
        submitted_frames: AtomicU64::new(0),
        written_frames: AtomicU64::new(0),
        submitted_blocks: AtomicU64::new(0),
        written_blocks: AtomicU64::new(0),
        detail: Mutex::new(None),
    });
    let worker_shared = shared.clone();
    let worker = thread::Builder::new()
        .name("flitzis-mix-capture-writer".into())
        .spawn(move || {
            writer::run(
                file,
                data_consumer,
                free_producer,
                scratch,
                worker_shared,
                hooks,
            );
        })?;
    let rt = Box::new(CaptureRt {
        shared: shared.clone(),
        data,
        free,
        partial: None,
        started: false,
        sealed: false,
        next_frame: 0,
        submitted_end: 0,
        submitted_frames: 0,
        submitted_blocks: 0,
        _released: RtReleased(shared.clone()),
    });
    Ok((
        CaptureControl {
            shared,
            worker: Some(worker),
            spool_path: spool_path.to_path_buf(),
        },
        rt,
    ))
}
