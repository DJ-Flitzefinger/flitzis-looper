//! Complete warm admission: bounded manifest, actual decoder selector, full
//! interleaved+mono verification and exact executed playback transform.
use super::lifecycle::{OpeningGuard, store};
use super::*;
use crate::audio_engine::cold_residency::{self, ResidentLoadHint};
use crate::audio_engine::sample_loader::{decoder_cache_selector, playback_cache_transform};
use crate::messages::{ResidentContext, ResidentSourceView};

const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_CANDIDATES: usize = 4096;

fn number(value: &Value, key: &str) -> io::Result<u64> {
    value[key]
        .as_u64()
        .ok_or_else(|| invalid("missing warm numeric dimension"))
}

pub(super) fn dimensions(value: &Value, maximum: usize) -> io::Result<(usize, usize, u32, u64)> {
    let channels = usize::try_from(number(value, "channels")?)
        .map_err(|_| invalid("warm channels overflow"))?;
    let frames = usize::try_from(number(value, "full_frames")?)
        .map_err(|_| invalid("warm frames overflow"))?;
    let rate =
        u32::try_from(number(value, "rate_hz")?).map_err(|_| invalid("warm rate overflow"))?;
    let length = frames
        .checked_mul(channels)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| invalid("warm extent overflow"))?;
    if !(1..=32).contains(&channels)
        || frames == 0
        || rate == 0
        || length > maximum
        || number(value, "full_bytes")? != length as u64
        || value["format"] != "f32-le-interleaved-v1"
        || value["channel_layout"] != "interleaved-channel-index-order-v1"
        || value["mono_revision"] != MONO_RULE
        || value["source_zero_bits"] != "0000000000000000"
    {
        return Err(invalid("incompatible warm PCM dimensions or policy"));
    }
    Ok((channels, frames, rate, length as u64))
}

fn verify_pcm(
    file: &mut File,
    descriptor: &Value,
    maximum: usize,
    cancelled: &impl Fn() -> bool,
    total: &mut u64,
) -> io::Result<()> {
    let (channels, _, _, length) = dimensions(descriptor, maximum)?;
    if file.metadata()?.len() != length {
        return Err(invalid("warm PCM length mismatch"));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut full = Sha256::new();
    let mut mono = Sha256::new();
    let mut bytes = [0_u8; CHUNK_BYTES];
    let chunk_bytes = (CHUNK_BYTES / 4 / channels) * channels * 4;
    let mut left = length;
    while left > 0 {
        check_cancelled(cancelled)?;
        let count = usize::try_from(left.min(chunk_bytes as u64))
            .map_err(|_| invalid("warm read extent"))?;
        file.read_exact(&mut bytes[..count])?;
        *total = total
            .checked_add(count as u64)
            .ok_or_else(|| invalid("warm byte accounting overflow"))?;
        full.update(&bytes[..count]);
        for frame in bytes[..count].chunks_exact(channels * 4) {
            let mut sum = 0_f64;
            for sample in frame.chunks_exact(4) {
                let sample = f32::from_le_bytes(sample.try_into().expect("f32 bytes"));
                if !sample.is_finite() {
                    return Err(invalid("nonfinite warm PCM"));
                }
                sum += f64::from(sample);
            }
            mono.update(((sum / channels as f64) as f32).to_le_bytes());
        }
        left -= count as u64;
    }
    if file.read(&mut bytes[..1])? != 0
        || descriptor["interleaved_sha256"].as_str()
            != Some(format!("{:x}", full.finalize()).as_str())
        || descriptor["mono_sha256"].as_str() != Some(format!("{:x}", mono.finalize()).as_str())
    {
        return Err(invalid("complete warm PCM digest mismatch"));
    }
    file.seek(SeekFrom::Start(0))?;
    check_cancelled(cancelled)
}

fn thread_cpu_nanos() -> io::Result<Option<u64>> {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct FileTime {
            low: u32,
            high: u32,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentThread() -> *mut std::ffi::c_void;
            fn GetThreadTimes(
                thread: *mut std::ffi::c_void,
                creation: *mut FileTime,
                exit: *mut FileTime,
                kernel: *mut FileTime,
                user: *mut FileTime,
            ) -> i32;
        }
        let mut creation = FileTime { low: 0, high: 0 };
        let mut exit = FileTime { low: 0, high: 0 };
        let mut kernel = FileTime { low: 0, high: 0 };
        let mut user = FileTime { low: 0, high: 0 };
        // SAFETY: four initialized FILETIME outputs and the current-thread pseudo
        // handle remain valid throughout this non-realtime synchronous call.
        if unsafe {
            GetThreadTimes(
                GetCurrentThread(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let ticks = |time: FileTime| (u64::from(time.high) << 32) | u64::from(time.low);
        Ok(Some(
            ticks(kernel)
                .saturating_add(ticks(user))
                .saturating_mul(100),
        ))
    }
    #[cfg(not(windows))]
    {
        Ok(None)
    }
}

impl ColdTransaction {
    pub(in crate::audio_engine) fn record_assignment_copy(&mut self, bytes: u64) {
        self.integrity.assignment_copy_bytes =
            self.integrity.assignment_copy_bytes.saturating_add(bytes);
    }
    pub(in crate::audio_engine) fn playback_transform(&self) -> io::Result<&Value> {
        self.manifest
            .as_ref()
            .map(|manifest| &manifest.descriptor["playback"]["transform"])
            .ok_or_else(|| invalid("no prepared playback transform"))
    }

    /// Invoke under preparation_gate through commit. The gate is digest/device
    /// scoped; a source-path/mtime index is never a content-authority shortcut.
    #[cfg(test)]
    pub(in crate::audio_engine) fn try_reuse(
        &mut self,
        output_rate: u32,
        output_channels: usize,
        maximum: usize,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<Option<SampleBuffer>> {
        self.try_reuse_selected(output_rate, output_channels, maximum, None, cancelled)
    }

    pub(in crate::audio_engine) fn try_reuse_selected(
        &mut self,
        output_rate: u32,
        output_channels: usize,
        maximum: usize,
        resident_hint: Option<ResidentLoadHint>,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<Option<SampleBuffer>> {
        check_cancelled(cancelled)?;
        let started = Instant::now();
        let cpu = thread_cpu_nanos()?;
        let selector = decoder_cache_selector(self.snapshot_file()?, &self.original_path)
            .map_err(io::Error::other)?;
        // Entries are locators only. Limit the read metadata, then validate each
        // candidate's real original/decoder/playback identity before reuse.
        let mut result = None;
        for entry in fs::read_dir(&self.cache_root)?.take(MAX_CANDIDATES) {
            check_cancelled(cancelled)?;
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.')
                || name
                    .split('-')
                    .next()
                    .is_none_or(|id| id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()))
            {
                continue;
            }
            let path = entry.path();
            match self.open_candidate(
                &path,
                &selector,
                (output_rate, output_channels),
                maximum,
                resident_hint,
                cancelled,
            ) {
                Ok(sample) => {
                    result = Some(sample);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
                // Partial/corrupt/old/device-incompatible/unsealable candidates
                // are untouched and cannot authorize source state. Regenerate.
                Err(_) => {}
            }
        }
        self.integrity.wall_nanos = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.integrity.cpu_nanos = cpu
            .zip(thread_cpu_nanos()?)
            .map(|(before, after)| after.saturating_sub(before));
        self.integrity.warm = result.is_some();
        Ok(result)
    }

    fn open_candidate(
        &mut self,
        path: &Path,
        selector: &Value,
        output: (u32, usize),
        maximum: usize,
        resident_hint: Option<ResidentLoadHint>,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<SampleBuffer> {
        let (output_rate, output_channels) = output;
        let _admission = OpeningGuard::acquire(path, cancelled)?;
        reject_links(path)?;
        if fs::canonicalize(path)?.parent() != Some(self.cache_root.as_path()) {
            return Err(invalid("warm entry escaped cache root"));
        }
        let directory = directory_guard(path)?;
        let manifest_path = path.join("manifest.json");
        reject_links(&manifest_path)?;
        let mut manifest_reader = sealed_reader(&manifest_path)?;
        let size = manifest_reader.metadata()?.len();
        if size == 0 || size > MAX_MANIFEST_BYTES {
            return Err(invalid("warm manifest exceeds bound"));
        }
        let mut bytes = vec![0; size as usize];
        manifest_reader.read_exact(&mut bytes)?;
        self.integrity.manifest_verify_bytes =
            self.integrity.manifest_verify_bytes.saturating_add(size);
        if manifest_reader.read(&mut [0_u8; 1])? != 0 {
            return Err(invalid("warm manifest EOF mismatch"));
        }
        let saved: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if saved["schema_version"] != SCHEMA_VERSION {
            return Err(invalid("warm schema mismatch"));
        }
        let descriptor = &saved["descriptor"];
        let decoder = &descriptor["decoder"];
        let playback = &descriptor["playback"];
        let decoder_identity = descriptor_digest(decoder)?;
        let identity = descriptor_digest(descriptor)?;
        if descriptor["schema_version"] != SCHEMA_VERSION
            || decoder["schema_version"] != SCHEMA_VERSION
            || saved["identity"] != identity
            || saved["decoder_identity"] != decoder_identity
            || playback["parent_identity"] != decoder_identity
            || path
                .file_name()
                .and_then(|v| v.to_str())
                .and_then(|v| v.split('-').next())
                != Some(identity.as_str())
            || decoder["original"] != json!({"sha256":self.source_digest,"bytes":self.source_bytes})
        {
            return Err(invalid("warm full identity mismatch"));
        }
        let manifest = ColdManifest {
            identity,
            decoder_identity,
            descriptor: descriptor.clone(),
        };
        if serde_json::to_vec(&canonical(&manifest.encoded())).map_err(io::Error::other)? != bytes {
            return Err(invalid("warm manifest is not canonical complete content"));
        }
        let mut provenance = decoder["pcm"]["provenance"].clone();
        let fields = provenance
            .as_object_mut()
            .ok_or_else(|| invalid("warm decoder provenance missing"))?;
        for key in [
            "packet_error_silence_frames",
            "skipped_unknown_duration_packets",
        ] {
            if fields.get(key).and_then(Value::as_u64).is_none() {
                return Err(invalid("warm decoder packet accounting missing"));
            }
            fields.insert(key.into(), json!(0));
        }
        if &provenance != selector
            || playback["pcm"]["provenance"] != json!({"processing":"full-buffer-playback-v1"})
        {
            return Err(invalid("warm actual decoder/version policy mismatch"));
        }
        let (source_channels, source_frames, source_rate, _) =
            dimensions(&decoder["pcm"], maximum)?;
        let (channels, frames, rate, _) = dimensions(&playback["pcm"], maximum)?;
        let transform = playback_cache_transform(
            source_rate,
            source_channels,
            source_frames,
            output_rate,
            output_channels,
            maximum,
        )
        .map_err(io::Error::other)?;
        if channels != output_channels
            || rate != output_rate
            || playback["transform"] != transform
            || transform["output_frames"] != frames
        {
            return Err(invalid("warm device/transform mismatch"));
        }
        let mut readers = Vec::with_capacity(3);
        for (file_name, pcm, total) in [
            (
                "decoder.f32le",
                &decoder["pcm"],
                &mut self.integrity.decoder_verify_bytes,
            ),
            (
                "playback.f32le",
                &playback["pcm"],
                &mut self.integrity.playback_verify_bytes,
            ),
        ] {
            let path = path.join(file_name);
            reject_links(&path)?;
            let mut reader = sealed_reader(&path)?;
            verify_pcm(&mut reader, pcm, maximum, cancelled, total)?;
            readers.push(reader);
        }
        manifest_reader.seek(SeekFrom::Start(0))?;
        readers.push(manifest_reader);
        let file_identities = readers
            .iter()
            .map(file_identity)
            .collect::<io::Result<Vec<_>>>()?;
        let directory_identity = file_identity(&directory)?;
        let existing = {
            let state = store()
                .state
                .lock()
                .map_err(|_| invalid("warm ownership poisoned"))?;
            state.caches.get(path).and_then(Weak::upgrade)
        };
        let source = cold_residency::identity(&manifest).map_err(io::Error::other)?;
        let selected = resident_hint
            .map(|hint| hint.region(rate, frames))
            .transpose()
            .map_err(io::Error::other)?;
        let (start, end, context) = match (resident_hint, selected) {
            (Some(hint), Some(region)) if !hint.key_lock => {
                (region.start, region.end, ResidentContext::FiniteLoop)
            }
            (Some(_), _) => (0, frames, ResidentContext::KeyLockFullTrack),
            _ => (0, frames, ResidentContext::FullTrack),
        };
        let finite = start != 0 || end != frames;
        let samples = if finite {
            read_playback_range(
                &mut readers[1],
                start,
                end - start,
                channels,
                maximum,
                cancelled,
                &mut self.integrity,
            )?
        } else if let Some(shared) = existing.as_ref() {
            let mut pcm = shared
                .pcm
                .lock()
                .map_err(|_| invalid("warm PCM ownership poisoned"))?;
            if let Some(samples) = pcm
                .as_ref()
                .and_then(Weak::upgrade)
                .filter(|pcm| pcm.len() == frames * channels)
            {
                samples
            } else {
                let samples = read_playback(
                    &mut readers[1],
                    frames,
                    channels,
                    maximum,
                    cancelled,
                    &mut self.integrity,
                )?;
                *pcm = Some(Arc::downgrade(&samples));
                samples
            }
        } else {
            read_playback(
                &mut readers[1],
                frames,
                channels,
                maximum,
                cancelled,
                &mut self.integrity,
            )?
        };
        // Transfer a cleanup slot only after all fallible PCM admission/read work.
        // A rejected candidate leaves this reservation available to the next one.
        let shared = if let Some(shared) = existing {
            shared
        } else {
            let mut state = store()
                .state
                .lock()
                .map_err(|_| invalid("warm ownership poisoned"))?;
            let retiring = state.deleting.iter().any(|task| task.path == path);
            let shared = Arc::new(CacheReaders {
                path: path.to_owned(),
                root: self.cache_root.clone(),
                readers,
                directories: vec![directory],
                pcm: Mutex::new((!finite).then(|| Arc::downgrade(&samples))),
                retired: AtomicBool::new(retiring),
                durable_assignments: Mutex::new(HashSet::new()),
                identity: directory_identity,
                file_identities,
                cleanup: Some(
                    self.cache_slot
                        .take()
                        .expect("fresh warm cache cleanup reserved"),
                ),
            });
            state
                .caches
                .insert(path.to_owned(), Arc::downgrade(&shared));
            shared
        };
        store()
            .state
            .lock()
            .map_err(|_| invalid("warm ownership poisoned"))?
            .deleting
            .retain(|task| task.path != path);
        self.manifest = Some(manifest);
        self.reused_cache = Some(shared);
        self.committed_cache = Some(path.to_owned());
        Ok(SampleBuffer {
            residency: Some(Arc::new(ResidentSourceView {
                source,
                start_frame: start,
                window_revision: 1,
                context,
            })),
            samples,
            channels,
        })
    }
}

pub(super) fn read_playback(
    reader: &mut File,
    frames: usize,
    channels: usize,
    maximum: usize,
    cancelled: &impl Fn() -> bool,
    metrics: &mut IntegrityMetrics,
) -> io::Result<Arc<[f32]>> {
    read_playback_range(reader, 0, frames, channels, maximum, cancelled, metrics)
}

fn read_playback_range(
    reader: &mut File,
    start: usize,
    frames: usize,
    channels: usize,
    maximum: usize,
    cancelled: &impl Fn() -> bool,
    metrics: &mut IntegrityMetrics,
) -> io::Result<Arc<[f32]>> {
    let count = frames
        .checked_mul(channels)
        .ok_or_else(|| invalid("warm sample extent"))?;
    if count.checked_mul(8).is_none_or(|n| n > maximum) {
        return Err(invalid("warm PCM conversion exceeds whole-job budget"));
    }
    let mut data = Vec::new();
    data.try_reserve_exact(count)
        .map_err(|_| io::Error::other("warm PCM allocation failed"))?;
    if data
        .capacity()
        .checked_mul(4)
        .and_then(|n| n.checked_add(count * 4))
        .is_none_or(|n| n > maximum)
    {
        return Err(invalid("warm PCM conversion exceeds whole-job budget"));
    }
    let offset = start
        .checked_mul(channels)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| invalid("resident offset overflow"))?;
    reader.seek(SeekFrom::Start(offset as u64))?;
    let mut bytes = [0_u8; CHUNK_BYTES];
    let mut left = count * 4;
    while left > 0 {
        check_cancelled(cancelled)?;
        let length = left.min(CHUNK_BYTES);
        reader.read_exact(&mut bytes[..length])?;
        metrics.playback_read_bytes = metrics.playback_read_bytes.saturating_add(length as u64);
        data.extend(
            bytes[..length]
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().expect("f32 bytes"))),
        );
        left -= length;
    }
    check_cancelled(cancelled)?;
    data.shrink_to_fit();
    #[cfg(test)]
    super::super::c3_observation::owned_pcm(data.capacity() * 4);
    Ok(Arc::from(data.into_boxed_slice()))
}
