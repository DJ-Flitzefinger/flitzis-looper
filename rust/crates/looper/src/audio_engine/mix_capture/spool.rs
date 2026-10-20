//! Fixed, self-describing internal spool metadata. All functions are off realtime.

use super::{CaptureConfig, CaptureFailure, Shared};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::Ordering;

const MAGIC: &[u8; 8] = b"FLTZCAP1";
const SLOT_BYTES: usize = 128;
pub(crate) const SPOOL_HEADER_BYTES: u64 = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum SpoolState {
    PreparedIncomplete = 0,
    RecordingIncomplete = 1,
    /// Declared sealed candidate; a later sync/seek can still have failed.
    /// Recovery must independently verify it and never grants publication here.
    SealedCandidate = 2,
    FailedIncomplete = 3,
}

#[derive(Debug)]
pub(crate) struct SpoolInfo {
    pub(crate) session_id: u64,
    pub(crate) revision: u64,
    pub(crate) sample_rate_hz: u32,
    pub(crate) channels: usize,
    pub(crate) state: SpoolState,
    pub(crate) failure: Option<CaptureFailure>,
    pub(crate) first_frame: Option<u64>,
    pub(crate) declared_submitted_frames: u64,
    pub(crate) declared_written_frames: u64,
    pub(crate) declared_end_frame: Option<u64>,
    /// Actual file length supplies a recoverable whole-frame prefix even when
    /// process loss preceded the next bounded header checkpoint.
    pub(crate) physical_whole_frames: u64,
    pub(crate) partial_frame_bytes: u64,
    pub(crate) requires_verification: bool,
    pub(crate) publication_ready: bool,
}

pub(super) struct Header {
    pub(super) revision: u64,
    session_id: u64,
    sample_rate_hz: u32,
    channels: u32,
    state: SpoolState,
    failure: u32,
    first: Option<u64>,
    submitted: u64,
    written: u64,
    end: Option<u64>,
}

impl Header {
    pub(super) fn prepared(session_id: u64, config: CaptureConfig) -> Self {
        Self {
            revision: 0,
            session_id,
            sample_rate_hz: config.sample_rate_hz,
            channels: config.channels as u32,
            state: SpoolState::PreparedIncomplete,
            failure: 0,
            first: None,
            submitted: 0,
            written: 0,
            end: None,
        }
    }

    fn bytes(&self) -> [u8; SLOT_BYTES] {
        let mut bytes = [0_u8; SLOT_BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        put32(&mut bytes, 8, 1); // Schema version.
        put32(&mut bytes, 12, SLOT_BYTES as u32);
        put64(&mut bytes, 16, self.revision);
        put64(&mut bytes, 24, self.session_id);
        put32(&mut bytes, 32, self.sample_rate_hz);
        put32(&mut bytes, 36, self.channels);
        put32(&mut bytes, 40, self.state as u32);
        put32(&mut bytes, 44, self.failure);
        put32(&mut bytes, 48, u32::from(self.first.is_some()));
        put32(&mut bytes, 52, u32::from(self.end.is_some()));
        put64(&mut bytes, 56, self.first.unwrap_or(0));
        put64(&mut bytes, 64, self.submitted);
        put64(&mut bytes, 72, self.written);
        put64(&mut bytes, 80, self.end.unwrap_or(0));
        put32(&mut bytes, 88, 1); // Interleaved IEEE754 float32 little endian.
        let digest = Sha256::digest(&bytes[..96]);
        bytes[96..].copy_from_slice(&digest);
        bytes
    }

    pub(super) fn initialize(&self, file: &mut File) -> io::Result<()> {
        let bytes = self.bytes();
        file.write_all(&bytes)?;
        file.write_all(&bytes)?;
        file.sync_all()
    }

    pub(super) fn checkpoint(
        &mut self,
        file: &mut File,
        shared: &Shared,
        state: SpoolState,
    ) -> io::Result<()> {
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| io::Error::other("capture header revision exhausted"))?;
        // Acquire sealing before its associated progress fields. A concurrent
        // stop must not pair an earlier submitted extent with its newer end.
        let sealed = shared.sealed.load(Ordering::Acquire);
        let started = shared.started.load(Ordering::Acquire);
        let candidate = Self {
            revision,
            session_id: self.session_id,
            sample_rate_hz: self.sample_rate_hz,
            channels: self.channels,
            state,
            first: started.then(|| shared.first_frame.load(Ordering::Acquire)),
            submitted: shared.submitted_frames.load(Ordering::Acquire),
            written: shared.written_frames.load(Ordering::Acquire),
            end: (started && sealed).then(|| shared.end_frame.load(Ordering::Acquire)),
            failure: u32::from(shared.failure_code()),
        };
        let offset = (revision % 2) * SLOT_BYTES as u64;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&candidate.bytes())?;
        file.sync_data()?;
        let frame_bytes = u64::from(self.channels) * 4;
        let payload_end = candidate
            .written
            .checked_mul(frame_bytes)
            .and_then(|bytes| bytes.checked_add(SPOOL_HEADER_BYTES))
            .ok_or_else(|| io::Error::other("capture spool extent overflow"))?;
        file.seek(SeekFrom::Start(payload_end))?;
        // A failed update must retry the SAME slot, preserving the previously
        // valid slot instead of alternating into and corrupting its fallback.
        *self = candidate;
        Ok(())
    }
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
fn get32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("fixed header field"),
    )
}
fn get64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("fixed header field"),
    )
}

fn decode(bytes: &[u8]) -> Option<Header> {
    if bytes.len() != SLOT_BYTES
        || &bytes[..8] != MAGIC
        || get32(bytes, 8) != 1
        || get32(bytes, 12) != SLOT_BYTES as u32
        || get32(bytes, 88) != 1
        || get32(bytes, 92) != 0
        || Sha256::digest(&bytes[..96])[..] != bytes[96..]
        || get64(bytes, 24) == 0
        || get32(bytes, 32) == 0
        || get32(bytes, 36) == 0
        || get32(bytes, 36) > u16::MAX as u32
        || get32(bytes, 48) > 1
        || get32(bytes, 52) > 1
        || get32(bytes, 44) > CaptureFailure::WorkerPanicked as u32
    {
        return None;
    }
    let state = match get32(bytes, 40) {
        0 => SpoolState::PreparedIncomplete,
        1 => SpoolState::RecordingIncomplete,
        2 => SpoolState::SealedCandidate,
        3 => SpoolState::FailedIncomplete,
        _ => return None,
    };
    Some(Header {
        revision: get64(bytes, 16),
        session_id: get64(bytes, 24),
        sample_rate_hz: get32(bytes, 32),
        channels: get32(bytes, 36),
        state,
        failure: get32(bytes, 44),
        first: (get32(bytes, 48) == 1).then(|| get64(bytes, 56)),
        submitted: get64(bytes, 64),
        written: get64(bytes, 72),
        end: (get32(bytes, 52) == 1).then(|| get64(bytes, 80)),
    })
}

/// Read only the fixed header and file metadata; never load a full take into RAM.
/// Unknown/corrupt input is rejected, never modified, repaired or deleted here.
pub(crate) fn inspect_spool(path: &Path) -> io::Result<SpoolInfo> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "unknown/inconsistent capture spool",
        )
    };
    let mut file = File::open(path)?;
    let mut bytes = [0_u8; SPOOL_HEADER_BYTES as usize];
    file.read_exact(&mut bytes)?;
    let first = decode(&bytes[..SLOT_BYTES]);
    let second = decode(&bytes[SLOT_BYTES..]);
    let header = match (first, second) {
        (Some(first), Some(second)) => {
            if (first.session_id, first.sample_rate_hz, first.channels)
                != (second.session_id, second.sample_rate_hz, second.channels)
            {
                return Err(invalid());
            }
            if first.revision >= second.revision {
                first
            } else {
                second
            }
        }
        (Some(header), None) | (None, Some(header)) => header,
        (None, None) => return Err(invalid()),
    };
    let payload = file
        .metadata()?
        .len()
        .checked_sub(SPOOL_HEADER_BYTES)
        .ok_or_else(invalid)?;
    let frame_bytes = u64::from(header.channels) * 4;
    let physical_whole_frames = payload / frame_bytes;
    let partial_frame_bytes = payload % frame_bytes;
    if header.written > physical_whole_frames
        || header.written > header.submitted
        || (header.first.is_none() && (header.written != 0 || header.submitted != 0))
        || (header.first.is_none() && payload != 0)
        || header
            .first
            .is_some_and(|first| first.checked_add(physical_whole_frames).is_none())
        || header.end.is_some_and(|end| {
            header
                .first
                .is_none_or(|first| first.checked_add(header.submitted) != Some(end))
        })
        || (header.state == SpoolState::SealedCandidate
            && (header.failure != 0
                || header.written != header.submitted
                || header.written != physical_whole_frames
                || partial_frame_bytes != 0
                || (header.first.is_some() && header.end.is_none())))
    {
        return Err(invalid());
    }
    Ok(SpoolInfo {
        session_id: header.session_id,
        revision: header.revision,
        sample_rate_hz: header.sample_rate_hz,
        channels: header.channels as usize,
        state: header.state,
        failure: CaptureFailure::from_code(header.failure as u8),
        first_frame: header.first,
        declared_submitted_frames: header.submitted,
        declared_written_frames: header.written,
        declared_end_frame: header.end,
        physical_whole_frames,
        partial_frame_bytes,
        requires_verification: true,
        publication_ready: false,
    })
}
