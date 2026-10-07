use pyo3::pymodule;

mod audio_engine;
mod messages;

/// The Python module implemented in Rust.
#[pymodule]
mod flitzis_looper_audio {
    #[pymodule_export]
    use super::audio_engine::AudioEngine;

    #[pymodule_export]
    use super::audio_engine::OfflineAnalysisJob;

    #[pymodule_export]
    use super::audio_engine::PreparedSourceTicket;

    #[pymodule_export]
    use super::audio_engine::ConstantTimingTicket;

    #[pymodule_export]
    use super::audio_engine::CapturedConstantTiming;

    #[pymodule_export]
    use super::audio_engine::AcceptedTimingRefreshTicket;

    #[pymodule_export]
    use super::audio_engine::SavedConstantTimingTicket;

    #[pymodule_export]
    use super::audio_engine::InputRuntimePadBinding;

    #[pymodule_export]
    use super::audio_engine::GlobalPlaybackBatchTicket;

    #[pymodule_export]
    use super::audio_engine::ScalarSourceGrid;

    #[pymodule_export]
    use super::messages::AudioMessage;
}
