use crate::messages::LoaderEvent;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoadProgressStage {
    Decoding,
    Resampling,
    ChannelMapping,
    Analyzing,
    Publishing,
}

impl LoadProgressStage {
    pub fn stage_label(self) -> &'static str {
        match self {
            Self::Decoding => "Decoding…",
            Self::Resampling => "Resampling…",
            Self::ChannelMapping => "Mapping channels…",
            Self::Analyzing => "Analyzing…",
            Self::Publishing => "Publishing…",
        }
    }

    fn range(self, resampling_required: bool) -> (f32, f32) {
        if !resampling_required {
            return match self {
                Self::Decoding => (0.0, 0.1),
                Self::Resampling => (0.1, 0.1),
                Self::ChannelMapping => (0.1, 0.15),
                Self::Analyzing => (0.15, 0.95),
                Self::Publishing => (0.95, 1.0),
            };
        }

        match self {
            Self::Decoding => (0.0, 0.1),
            Self::Resampling => (0.1, 0.2),
            Self::ChannelMapping => (0.2, 0.25),
            Self::Analyzing => (0.25, 0.95),
            Self::Publishing => (0.95, 1.0),
        }
    }
}

pub struct ProgressReporter {
    id: usize,
    tx: Sender<LoaderEvent>,
    last_emit: Instant,
    min_interval: Duration,
    pub resampling_required: Option<bool>,
}

impl ProgressReporter {
    pub fn new(id: usize, tx: Sender<LoaderEvent>) -> Self {
        let min_interval = Duration::from_millis(100);
        Self {
            id,
            tx,
            last_emit: Instant::now()
                .checked_sub(min_interval)
                .unwrap_or_else(Instant::now),
            min_interval,
            resampling_required: None,
        }
    }

    pub fn emit(
        &mut self,
        stage: LoadProgressStage,
        local_percent: f32,
        resampling_required: bool,
        force: bool,
    ) {
        let local_percent = if local_percent.is_finite() {
            local_percent.clamp(0.0, 1.0)
        } else {
            0.0
        };

        let now = Instant::now();
        if !force && now.duration_since(self.last_emit) < self.min_interval {
            return;
        }
        self.last_emit = now;

        self.resampling_required.get_or_insert(resampling_required);
        let resampling_required = self.resampling_required.unwrap_or(resampling_required);

        let (start, end) = stage.range(resampling_required);
        let percent = (start + (end - start) * local_percent).clamp(0.0, 1.0);
        let stage = match stage {
            LoadProgressStage::Analyzing => stage.stage_label().to_string(),
            _ => format!("Loading ({})", stage.stage_label()),
        };
        let _ = self.tx.send(LoaderEvent::Progress {
            id: self.id,
            percent,
            stage,
        });
    }
}
