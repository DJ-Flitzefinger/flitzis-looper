//! Weak range locators on the existing verified cache, never playback authority.
//! Opening/preparation gates serialize equal-file misses. Asset admission runs
//! after this mutex is released and charges each fresh scoped owner normally.

use super::*;
use crate::messages::{CompleteSourceIdentity, ResidentSourceView};

// Matches the existing 1024 physical reader records with 128 retained views each.
// Locators never own PCM, a view, a file or an assignment.
const MAX_WINDOWS: usize = 1024 * 128;

struct Window {
    pcm: Weak<[f32]>,
    view: Weak<ResidentSourceView>,
}

#[derive(Default)]
pub(super) struct SharedResidentWindows {
    entries: Vec<Window>,
}

impl SharedResidentWindows {
    pub(super) fn prune(&mut self) {
        self.entries
            .retain(|window| window.pcm.strong_count() > 0 && window.view.strong_count() > 0);
    }

    fn available(&self) -> io::Result<()> {
        if self.entries.len() >= MAX_WINDOWS {
            return Err(
                crate::audio_engine::resident_accounting::ResidentAdmissionError::Capacity(
                    "verified cache window locators",
                )
                .into_io_error(),
            );
        }
        Ok(())
    }

    fn find(
        &mut self,
        source: &CompleteSourceIdentity,
        start: usize,
        end: usize,
    ) -> io::Result<Option<SampleBuffer>> {
        self.prune();
        for entry in &self.entries {
            let Some(view) = entry.view.upgrade() else {
                continue;
            };
            if *view.source != *source || view.start_frame != start {
                continue;
            }
            let Some(samples) = entry.pcm.upgrade() else {
                continue;
            };
            let sample = SampleBuffer {
                channels: source.channels,
                samples,
                residency: Some(view),
            };
            if sample.resident_end() == end
                && sample.valid_residency(source.sample_rate_hz, source.channels)
            {
                // A fresh view will be registered even on a hit; reserve locator space
                // before any caller can allocate a new PCM range on a miss.
                self.available()?;
                return Ok(Some(sample));
            }
        }
        self.available()?;
        Ok(None)
    }

    fn remember(&mut self, sample: &SampleBuffer) -> io::Result<()> {
        let view = sample
            .residency
            .as_ref()
            .ok_or_else(|| invalid("cached range view missing"))?;
        if !sample.valid_residency(view.source.sample_rate_hz, view.source.channels) {
            return Err(invalid(
                "cached range geometry differs from verified source",
            ));
        }
        self.prune();
        let pcm = Arc::downgrade(&sample.samples);
        let view = Arc::downgrade(view);
        if self
            .entries
            .iter()
            .any(|entry| entry.pcm.ptr_eq(&pcm) && entry.view.ptr_eq(&view))
        {
            return Ok(());
        }
        self.available()?;
        self.entries.push(Window { pcm, view });
        Ok(())
    }
}

impl CacheReaders {
    /// Call only after this cache's manifest, complete PCM and held FileIDs have
    /// passed warm verification under OpeningGuard. The returned view is a locator;
    /// its revision/context and current Native permissions are never adopted.
    pub(super) fn verified_window(
        &self,
        source: &CompleteSourceIdentity,
        start: usize,
        end: usize,
    ) -> io::Result<Option<SampleBuffer>> {
        self.windows
            .lock()
            .map_err(|_| invalid("warm range ownership poisoned"))?
            .find(source, start, end)
    }

    pub(super) fn remember_verified_window(&self, sample: &SampleBuffer) -> io::Result<()> {
        self.windows
            .lock()
            .map_err(|_| invalid("warm range ownership poisoned"))?
            .remember(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::ResidentContext;

    #[test]
    fn equal_verified_ranges_share_only_live_immutable_backing_and_identity() {
        let complete = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.5; 32]),
            residency: None,
        }
        .with_complete_source(8_000);
        let first = complete
            .window(4, 20, 7, ResidentContext::FiniteLoop)
            .unwrap();
        let mut windows = SharedResidentWindows::default();
        windows.remember(&first).unwrap();
        let source = first.residency.as_ref().unwrap().source.clone();
        let hit = windows.find(&source, 4, 20).unwrap().unwrap();
        assert!(Arc::ptr_eq(&first.samples, &hit.samples));
        assert!(Arc::ptr_eq(
            &source,
            &hit.residency.as_ref().unwrap().source
        ));
        let second = hit
            .window(4, 20, 23, ResidentContext::KeyLockFiniteLoop)
            .unwrap();
        windows.remember(&second).unwrap();
        assert!(!Arc::ptr_eq(
            first.residency.as_ref().unwrap(),
            second.residency.as_ref().unwrap()
        ));
        assert_eq!(first.window_revision(), 7);
        assert_eq!(second.window_revision(), 23);
        assert!(windows.find(&source, 5, 20).unwrap().is_none());
        assert!(windows.find(&source, 4, 19).unwrap().is_none());
        let foreign = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.25; 32]),
            residency: None,
        }
        .with_complete_source(8_001);
        assert!(
            windows
                .find(&foreign.residency.as_ref().unwrap().source, 4, 20)
                .unwrap()
                .is_none()
        );
        drop(first);
        drop(hit);
        assert!(windows.find(&source, 4, 20).unwrap().is_some());
        drop(second);
        assert!(windows.find(&source, 4, 20).unwrap().is_none());
        assert!(windows.entries.is_empty());
    }
}
