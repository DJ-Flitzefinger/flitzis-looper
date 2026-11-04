//! Qualify driver timing without changing the engine's transport/frame authority.

#[derive(Clone, Copy)]
pub(super) struct DriverClockObservation {
    pub system_ns: u64,
    pub sample_position: u64,
    pub sample_rate: f64,
    pub flags: u32,
}

#[derive(Default)]
pub(super) struct DriverClockProgress {
    previous: Option<(u64, u64)>,
    callbacks: u64,
}

impl DriverClockProgress {
    pub fn observe(&mut self, time: DriverClockObservation, rate: u32, frames: usize) -> bool {
        self.callbacks = self.callbacks.saturating_add(1);
        let valid = rate != 0
            && frames != 0
            && self.callbacks >= 3
            && time.flags & 3 == 3
            && time.flags & 32 == 0
            && (time.flags & 16 == 0 || time.flags & 4 != 0 && time.sample_rate == f64::from(rate))
            && (time.flags & 4 == 0 || time.sample_rate == f64::from(rate))
            && self.previous.is_some_and(|(system_ns, position)| {
                time.system_ns > system_ns
                    && time.sample_position.checked_sub(position) == Some(frames as u64)
            });
        self.previous = (time.flags & 3 == 3).then_some((time.system_ns, time.sample_position));
        valid
    }
}

pub(super) fn latency_publication(before: u32, after: u32, frames: i32) -> Option<u64> {
    (before == after && frames >= 0).then(|| (u64::from(after) << 32) | frames as u64)
}

pub(super) fn latency_delay_ns(packed: u64, observed_revision: u32, rate: u32) -> Option<u64> {
    (rate != 0 && packed >> 32 == u64::from(observed_revision))
        .then(|| u64::from(packed as u32) * 1_000_000_000 / u64::from(rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(system_ns: u64, position: u64) -> DriverClockObservation {
        DriverClockObservation {
            system_ns,
            sample_position: position,
            sample_rate: 96_000.0,
            flags: 7,
        }
    }

    #[test]
    fn synchronous_priming_is_not_a_valid_audible_clock() {
        let mut progress = DriverClockProgress::default();
        assert!(!progress.observe(time(10, 0), 96_000, 96));
        assert!(!progress.observe(time(10, 96), 96_000, 96));
        assert!(progress.observe(time(1_000_010, 192), 96_000, 96));
        assert!(progress.observe(time(2_000_010, 288), 96_000, 96));
    }

    #[test]
    fn missing_flags_counter_discontinuity_and_changed_rate_cannot_qualify() {
        let mut progress = DriverClockProgress::default();
        for index in 0..3 {
            progress.observe(time(index + 1, index * 96), 96_000, 96);
        }
        assert!(!progress.observe(time(4, 480), 96_000, 96));
        assert!(!progress.observe(time(4, 576), 96_000, 96));
        let mut observation = time(5, 672);
        observation.flags = 1;
        assert!(!progress.observe(observation, 96_000, 96));
        assert!(!progress.observe(time(6, 768), 96_000, 96));
        observation = time(7, 864);
        observation.sample_rate = 48_000.0;
        assert!(!progress.observe(observation, 96_000, 96));
        observation = time(8, 960);
        observation.flags |= 32;
        assert!(!progress.observe(observation, 96_000, 96));
        assert!(progress.observe(time(9, 1056), 96_000, 96));
    }

    #[test]
    fn latency_requery_race_and_revision_mismatch_remain_unknown() {
        assert_eq!(latency_publication(4, 5, 192), None);
        assert_eq!(latency_publication(5, 5, -1), None);
        let packed = latency_publication(5, 5, 192).unwrap();
        assert_eq!(latency_delay_ns(packed, 6, 96_000), None);
        assert_eq!(latency_delay_ns(packed, 5, 0), None);
        assert_eq!(latency_delay_ns(packed, 5, 96_000), Some(2_000_000));
    }
}
