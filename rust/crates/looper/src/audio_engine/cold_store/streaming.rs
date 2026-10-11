//! Bounded complete PCM writing. Total file extent is never a RAM reservation.
use super::*;

pub(in crate::audio_engine) struct CompletedPcmArtifact {
    pub descriptor: Value,
    pub reader: File,
}

pub(in crate::audio_engine) struct PcmArtifactWriter {
    path: PathBuf,
    file: File,
    rate_hz: u32,
    channels: usize,
    frames: u64,
    interleaved: Sha256,
    mono: Sha256,
}

impl PcmArtifactWriter {
    pub(in crate::audio_engine) fn create(
        path: &Path,
        rate_hz: u32,
        channels: usize,
    ) -> io::Result<Self> {
        if rate_hz == 0 || !(1..=32).contains(&channels) {
            return Err(invalid("streamed PCM geometry"));
        }
        Ok(Self {
            path: path.to_owned(),
            file: OpenOptions::new().write(true).create_new(true).open(path)?,
            rate_hz,
            channels,
            frames: 0,
            interleaved: Sha256::new(),
            mono: Sha256::new(),
        })
    }

    pub(in crate::audio_engine) fn append(
        &mut self,
        samples: &[f32],
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<()> {
        if !samples.len().is_multiple_of(self.channels) {
            return Err(invalid("streamed PCM incomplete frame"));
        }
        let mut bytes = [0_u8; CHUNK_BYTES];
        let chunk_samples = CHUNK_BYTES / 4 / self.channels * self.channels;
        for chunk in samples.chunks(chunk_samples) {
            check_cancelled(cancelled)?;
            for (sample, target) in chunk.iter().zip(bytes.chunks_exact_mut(4)) {
                if !sample.is_finite() {
                    return Err(invalid("nonfinite streamed PCM"));
                }
                target.copy_from_slice(&sample.to_le_bytes());
            }
            let encoded = &bytes[..chunk.len() * 4];
            self.file.write_all(encoded)?;
            self.interleaved.update(encoded);
            for frame in chunk.chunks_exact(self.channels) {
                let sum: f64 = frame.iter().copied().map(f64::from).sum();
                self.mono
                    .update(((sum / self.channels as f64) as f32).to_le_bytes());
            }
            self.frames = self
                .frames
                .checked_add((chunk.len() / self.channels) as u64)
                .ok_or_else(|| invalid("streamed PCM frame extent overflow"))?;
        }
        check_cancelled(cancelled)
    }

    pub(in crate::audio_engine) fn finish(
        self,
        provenance: Value,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<CompletedPcmArtifact> {
        if self.frames == 0 || !provenance.is_object() {
            return Err(invalid("streamed PCM empty extent or provenance"));
        }
        check_cancelled(cancelled)?;
        self.file.sync_all()?;
        drop(self.file);
        let length = self
            .frames
            .checked_mul(self.channels as u64)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| invalid("streamed PCM byte extent overflow"))?;
        let descriptor = json!({"format":"f32-le-interleaved-v1","channel_layout":"interleaved-channel-index-order-v1",
            "rate_hz":self.rate_hz,"channels":self.channels,"full_frames":self.frames,"full_bytes":length,
            "interleaved_sha256":format!("{:x}",self.interleaved.finalize()),"mono_sha256":format!("{:x}",self.mono.finalize()),
            "mono_revision":MONO_RULE,"source_zero_bits":"0000000000000000","provenance":provenance});
        let mut reader = sealed_reader(&self.path)?;
        // Re-read every frame, signed-f64 mono, hash, finite value and exact EOF
        // from the immutable handle; writer counters alone grant no publication.
        warm::verify_pcm(&mut reader, &descriptor, CHUNK_BYTES, cancelled, &mut 0)?;
        Ok(CompletedPcmArtifact { descriptor, reader })
    }
}

impl ColdTransaction {
    pub(in crate::audio_engine) fn record_streaming_scratch(&mut self, bytes: usize) {
        self.integrity.stream_scratch_reserved_bytes = self
            .integrity
            .stream_scratch_reserved_bytes
            .max(bytes as u64);
    }
    pub(in crate::audio_engine) fn streamed_artifact_paths(
        &self,
    ) -> io::Result<(PathBuf, PathBuf)> {
        if self.manifest.is_some() {
            return Err(invalid("cold artifacts already prepared"));
        }
        let path = self
            .staging
            .as_ref()
            .ok_or_else(|| invalid("cold transaction already committed"))?;
        Ok((path.join("decoder.f32le"), path.join("playback.f32le")))
    }

    pub(in crate::audio_engine) fn finish_streamed_artifacts(
        &mut self,
        decoder: CompletedPcmArtifact,
        playback: CompletedPcmArtifact,
        playback_transform: Value,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<ColdManifest> {
        check_cancelled(cancelled)?;
        if self.manifest.is_some() || !playback_transform.is_object() {
            return Err(invalid(
                "cold artifacts already prepared or missing transform",
            ));
        }
        let staging = self
            .staging
            .as_ref()
            .ok_or_else(|| invalid("cold transaction already committed"))?;
        self.integrity.decoder_verify_bytes = self.integrity.decoder_verify_bytes.saturating_add(
            decoder.descriptor["full_bytes"]
                .as_u64()
                .ok_or_else(|| invalid("decoder full bytes"))?,
        );
        self.integrity.playback_verify_bytes = self.integrity.playback_verify_bytes.saturating_add(
            playback.descriptor["full_bytes"]
                .as_u64()
                .ok_or_else(|| invalid("playback full bytes"))?,
        );
        let decoder_descriptor = json!({"schema_version":SCHEMA_VERSION,
            "original":{"sha256":self.source_digest,"bytes":self.source_bytes},"pcm":decoder.descriptor});
        let decoder_identity = descriptor_digest(&decoder_descriptor)?;
        let descriptor = json!({"schema_version":SCHEMA_VERSION,"decoder":decoder_descriptor,
            "playback":{"parent_identity":decoder_identity,"pcm":playback.descriptor,"transform":playback_transform}});
        let manifest = ColdManifest {
            identity: descriptor_digest(&descriptor)?,
            decoder_identity,
            descriptor,
        };
        let bytes =
            serde_json::to_vec(&canonical(&manifest.encoded())).map_err(io::Error::other)?;
        let path = staging.join("manifest.json");
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        writer.write_all(&bytes)?;
        check_cancelled(cancelled)?;
        writer.sync_all()?;
        drop(writer);
        let mut reader = sealed_reader(&path)?;
        verify_file(
            &mut reader,
            &format!("{:x}", Sha256::digest(&bytes)),
            bytes.len() as u64,
            cancelled,
        )?;
        self.artifact_readers
            .extend([decoder.reader, playback.reader, reader]);
        self.integrity.manifest_verify_bytes = self
            .integrity
            .manifest_verify_bytes
            .saturating_add(bytes.len() as u64);
        self.manifest = Some(manifest.clone());
        Ok(manifest)
    }

    /// Select actual playback demand only after complete disk integrity is known.
    pub(in crate::audio_engine) fn read_prepared_selection(
        &mut self,
        hint: Option<crate::audio_engine::cold_residency::ResidentLoadHint>,
        maximum: usize,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<SampleBuffer> {
        use crate::audio_engine::cold_residency;
        use crate::messages::{ResidentContext, ResidentSourceView};
        let source = cold_residency::identity(self.manifest()?).map_err(io::Error::other)?;
        let (start, end, context) = if let Some(hint) = hint {
            let range = hint
                .region(source.sample_rate_hz, source.frame_count)
                .map_err(io::Error::other)?;
            if hint.key_lock {
                (0, source.frame_count, ResidentContext::KeyLockFullTrack)
            } else {
                (range.start, range.end, ResidentContext::FiniteLoop)
            }
        } else {
            (0, source.frame_count, ResidentContext::FullTrack)
        };
        let mut reader = if let Some(cache) = self.reused_cache.as_ref() {
            let reader = sealed_reader(&cache.path.join("playback.f32le"))?;
            if cache.file_identities.get(1) != Some(&file_identity(&reader)?) {
                return Err(invalid("prepared playback FileID changed"));
            }
            reader
        } else {
            let retained = self
                .artifact_readers
                .get(1)
                .ok_or_else(|| invalid("prepared playback reader missing"))?;
            let path = self
                .staging
                .as_ref()
                .or(self.committed_cache.as_ref())
                .ok_or_else(|| invalid("prepared playback path missing"))?;
            let reader = sealed_reader(&path.join("playback.f32le"))?;
            if file_identity(retained)? != file_identity(&reader)? {
                return Err(invalid("prepared playback FileID changed"));
            }
            reader
        };
        let samples = warm::read_playback_range(
            &mut reader,
            start,
            end - start,
            source.channels,
            maximum,
            cancelled,
            &mut self.integrity,
        )?;
        Ok(SampleBuffer {
            channels: source.channels,
            samples,
            residency: Some(Arc::new(ResidentSourceView {
                source,
                start_frame: start,
                window_revision: 1,
                context,
            })),
        })
    }
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn productive_mid_write_read_only_handle_fault_rolls_back_only_owned_staging() {
        let temp = tempfile::tempdir().unwrap();
        let samples = temp.path().join("samples");
        fs::create_dir(&samples).unwrap();
        let source = samples.join("source.wav");
        fs::write(&source, b"protected byte-exact original").unwrap();
        let transaction = ColdTransaction::capture(&samples, &source, false, &|| false).unwrap();
        let staging = transaction.staging.clone().unwrap();
        let (decoder, _) = transaction.streamed_artifact_paths().unwrap();
        let mut writer = PcmArtifactWriter::create(&decoder, 48_000, 2).unwrap();
        writer.append(&[0.5, -0.25, 0.75, -0.5], &|| false).unwrap();
        // Precise isolated injection: after a productive PCM write replace only
        // this attempt's writable handle by a read-only handle. This exercises
        // the actual OS write failure, without drive filling or host OOM.
        writer.file = File::open(&decoder).unwrap();
        let failure = writer.append(&[0.125, -0.125], &|| false).unwrap_err();
        assert!(matches!(
            failure.kind(),
            io::ErrorKind::PermissionDenied | io::ErrorKind::Other
        ));
        assert_eq!(fs::metadata(&decoder).unwrap().len(), 16);
        drop(writer);
        drop(transaction);
        assert!(!staging.exists());
        assert_eq!(fs::read(&source).unwrap(), b"protected byte-exact original");
    }

    #[test]
    fn mid_complete_verification_cancel_returns_reader_and_preserves_owned_rollback() {
        use std::cell::Cell;
        let temp = tempfile::tempdir().unwrap();
        let samples = temp.path().join("samples");
        fs::create_dir(&samples).unwrap();
        let source = samples.join("source.wav");
        fs::write(&source, b"unchanged original").unwrap();
        let transaction = ColdTransaction::capture(&samples, &source, false, &|| false).unwrap();
        let staging = transaction.staging.clone().unwrap();
        let (decoder, _) = transaction.streamed_artifact_paths().unwrap();
        let mut writer = PcmArtifactWriter::create(&decoder, 48_000, 2).unwrap();
        let chunk = [0.25; 8192];
        for _ in 0..8 {
            writer.append(&chunk, &|| false).unwrap();
        }
        let calls = Cell::new(0);
        let cancelled = || {
            calls.set(calls.get() + 1);
            calls.get() >= 3
        };
        let failure = writer
            .finish(json!({"verification-cancel-fixture":1}), &cancelled)
            .err()
            .unwrap();
        assert_eq!(failure.kind(), io::ErrorKind::Interrupted);
        assert_eq!(fs::metadata(&decoder).unwrap().len(), 262_144);
        drop(transaction);
        assert!(!staging.exists());
        assert_eq!(fs::read(source).unwrap(), b"unchanged original");
    }
}
