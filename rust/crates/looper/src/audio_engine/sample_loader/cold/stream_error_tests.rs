//! Corrupt demux termination through the real PCM decoder and cold publication.
#![cfg(windows)]

use super::*;
use crate::audio_engine::cold_store::ColdTransaction;
use std::collections::VecDeque;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use symphonia::core::formats::{Cue, Packet, SeekMode, SeekTo, SeekedTo, Track};
use symphonia::core::meta::Metadata;

const RATE: u32 = 8_000;
const CHANNELS: usize = 2;
const MAXIMUM: usize = 64 * 1024 * 1024;

#[derive(Default)]
struct Reads {
    packets: AtomicUsize,
    injected_errors: AtomicUsize,
    actual_eof: AtomicUsize,
}

/// Only the demux result is injected. Valid packets, codec parameters, PCM
/// conversion, disk verification and source-bound manifest are all productive.
struct CorruptingReader {
    inner: Box<dyn FormatReader>,
    bursts: VecDeque<(usize, usize)>,
    reads: Arc<Reads>,
}

impl FormatReader for CorruptingReader {
    fn try_new(_: MediaSourceStream, _: &FormatOptions) -> symphonia::core::errors::Result<Self> {
        Err(DecoderError::Unsupported(
            "test reader wraps the actual probe",
        ))
    }

    fn cues(&self) -> &[Cue] {
        self.inner.cues()
    }

    fn metadata(&mut self) -> Metadata<'_> {
        self.inner.metadata()
    }

    fn tracks(&self) -> &[Track] {
        self.inner.tracks()
    }

    fn seek(&mut self, mode: SeekMode, to: SeekTo) -> symphonia::core::errors::Result<SeekedTo> {
        self.inner.seek(mode, to)
    }

    fn next_packet(&mut self) -> symphonia::core::errors::Result<Packet> {
        if let Some((after_packets, remaining)) = self.bursts.front_mut() {
            if self.reads.packets.load(Ordering::Relaxed) >= *after_packets {
                *remaining -= 1;
                if *remaining == 0 {
                    self.bursts.pop_front();
                }
                self.reads.injected_errors.fetch_add(1, Ordering::Relaxed);
                return Err(DecoderError::DecodeError("injected demux corruption"));
            }
        }
        let result = self.inner.next_packet();
        match &result {
            Ok(_) => {
                self.reads.packets.fetch_add(1, Ordering::Relaxed);
            }
            Err(DecoderError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                self.reads.actual_eof.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        result
    }

    fn into_inner(self: Box<Self>) -> MediaSourceStream {
        self.inner.into_inner()
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    samples: std::path::PathBuf,
    source: std::path::PathBuf,
    original: Vec<u8>,
    expected: Vec<f32>,
    frames: usize,
}

impl Fixture {
    fn new(frames: usize) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let samples = temp.path().join("samples");
        fs::create_dir(&samples).unwrap();
        let source = samples.join("source.wav");
        let signed: Vec<i16> = (0..frames * CHANNELS)
            .map(|index| ((index * 37) % 32_767) as i16 - 16_383)
            .collect();
        crate::audio_engine::sample_loader::tests::write_pcm16_wav(
            &source,
            CHANNELS as u16,
            RATE,
            &signed,
        )
        .unwrap();
        let original = fs::read(&source).unwrap();
        let expected = signed
            .into_iter()
            .map(|sample| f32::from(sample) / 32_768.0)
            .collect();
        Self {
            _temp: temp,
            samples,
            source,
            original,
            expected,
            frames,
        }
    }

    fn capture(&self) -> ColdTransaction {
        ColdTransaction::capture(&self.samples, &self.source, false, &|| false).unwrap()
    }

    fn prepare(
        &self,
        transaction: &mut ColdTransaction,
        bursts: impl IntoIterator<Item = (usize, usize)>,
    ) -> (
        Result<(), SampleLoadError>,
        Arc<Reads>,
        Vec<SampleLoadProgress>,
    ) {
        let (format, params, descriptor) =
            probe_snapshot(transaction.snapshot_file().unwrap(), &self.source).unwrap();
        assert_eq!(descriptor.declared_frames, Some(self.frames as u64));
        let reads = Arc::new(Reads::default());
        let format = Box::new(CorruptingReader {
            inner: format,
            bursts: bursts.into_iter().collect(),
            reads: Arc::clone(&reads),
        });
        let (decoder_path, playback_path) = transaction.streamed_artifact_paths().unwrap();
        let mut progress = Vec::new();
        let result = (|| {
            let mut decoded = decode_probed_audio_streamed(
                (format, params, descriptor),
                &decoder_path,
                RATE,
                MAXIMUM,
                &|| false,
                |event| progress.push(event),
            )?;
            let (playback, transform) = prepare_playback_streamed(
                &mut decoded,
                &playback_path,
                CHANNELS,
                RATE,
                MAXIMUM,
                &|| false,
                |_| {},
            )?;
            transaction.record_streaming_scratch(decoded.scratch_reserved_bytes);
            transaction.finish_streamed_artifacts(
                decoded.artifact,
                playback,
                transform.to_json(),
                &|| false,
            )?;
            Ok(())
        })();
        (result, reads, progress)
    }

    fn assert_complete(&self, transaction: &ColdTransaction, skipped_packets: usize) {
        let manifest = &transaction.manifest().unwrap().descriptor;
        assert_eq!(
            manifest["decoder"]["original"]["sha256"],
            format!("{:x}", Sha256::digest(&self.original))
        );
        assert_eq!(
            manifest["decoder"]["original"]["bytes"],
            self.original.len()
        );
        assert_eq!(manifest["decoder"]["pcm"]["full_frames"], self.frames);
        assert_eq!(manifest["playback"]["pcm"]["full_frames"], self.frames);
        assert_eq!(
            manifest["decoder"]["pcm"]["provenance"]["skipped_unknown_duration_packets"],
            skipped_packets
        );
        assert!(transaction.streamed_artifact_paths().is_err());
    }
}

fn samples(path: &Path) -> Vec<f32> {
    fs::read(path)
        .unwrap()
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}

#[test]
fn streamed_demux_error_limit_after_real_pcm_cannot_publish_complete_source() {
    let fixture = Fixture::new(16_401);
    let mut transaction = fixture.capture();
    let (decoder, playback) = transaction.streamed_artifact_paths().unwrap();
    let staging = decoder.parent().unwrap().to_owned();
    let (result, reads, progress) = fixture.prepare(&mut transaction, [(1, 65)]);
    assert!(
        matches!(
            &result,
            Err(SampleLoadError::Decode(DecoderError::DecodeError(_)))
        ),
        "65 demux errors after valid PCM must fail; result={result:?}; published manifest={:?}",
        transaction.manifest().map(|manifest| &manifest.descriptor)
    );
    assert_eq!(reads.packets.load(Ordering::Relaxed), 1);
    assert_eq!(reads.injected_errors.load(Ordering::Relaxed), 65);
    assert_eq!(reads.actual_eof.load(Ordering::Relaxed), 0);
    assert!(
        decoder.metadata().unwrap().len() > 0,
        "real PCM prefix was written"
    );
    assert!(transaction.manifest().is_err());
    assert!(!staging.join("manifest.json").exists());
    assert!(!playback.exists());
    assert!(progress.iter().all(|event| event.percent < 1.0));
    assert_eq!(fs::read(&fixture.source).unwrap(), fixture.original);
}

#[test]
fn streamed_demux_true_eof_publishes_all_pcm_bound_to_complete_original() {
    let fixture = Fixture::new(16_401);
    let mut transaction = fixture.capture();
    let (decoder, playback) = transaction.streamed_artifact_paths().unwrap();
    let (result, reads, progress) = fixture.prepare(&mut transaction, []);
    result.unwrap();
    assert!(reads.packets.load(Ordering::Relaxed) > 1);
    assert_eq!(reads.injected_errors.load(Ordering::Relaxed), 0);
    assert_eq!(reads.actual_eof.load(Ordering::Relaxed), 1);
    fixture.assert_complete(&transaction, 0);
    assert_eq!(samples(&decoder), fixture.expected);
    assert_eq!(samples(&playback), fixture.expected);
    assert_eq!(progress.last().unwrap().percent, 1.0);
    assert_eq!(fs::read(&fixture.source).unwrap(), fixture.original);
}

#[test]
fn streamed_demux_sixty_four_errors_then_real_packet_preserves_complete_pcm() {
    let fixture = Fixture::new(16_401);
    let mut transaction = fixture.capture();
    let (decoder, playback) = transaction.streamed_artifact_paths().unwrap();
    let (result, reads, _) = fixture.prepare(&mut transaction, [(1, 64), (2, 64)]);
    result.unwrap();
    assert!(reads.packets.load(Ordering::Relaxed) > 2);
    assert_eq!(reads.injected_errors.load(Ordering::Relaxed), 128);
    assert_eq!(reads.actual_eof.load(Ordering::Relaxed), 1);
    fixture.assert_complete(&transaction, 128);
    assert_eq!(samples(&decoder), fixture.expected);
    assert_eq!(samples(&playback), fixture.expected);
    assert_eq!(fs::read(&fixture.source).unwrap(), fixture.original);
}

#[test]
fn streamed_demux_more_than_sixty_four_isolated_errors_do_not_truncate_pcm() {
    let fixture = Fixture::new(512_017);
    let mut transaction = fixture.capture();
    let (decoder, playback) = transaction.streamed_artifact_paths().unwrap();
    let (result, reads, _) = fixture.prepare(&mut transaction, (1..=65).map(|packet| (packet, 1)));
    result.unwrap();
    assert!(reads.packets.load(Ordering::Relaxed) > 65);
    assert_eq!(reads.injected_errors.load(Ordering::Relaxed), 65);
    assert_eq!(reads.actual_eof.load(Ordering::Relaxed), 1);
    fixture.assert_complete(&transaction, 65);
    assert_eq!(samples(&decoder), fixture.expected);
    assert_eq!(samples(&playback), fixture.expected);
    assert_eq!(fs::read(&fixture.source).unwrap(), fixture.original);
}
