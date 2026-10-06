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
    use super::audio_engine::ScalarSourceGrid;

    #[pymodule_export]
    use super::messages::AudioMessage;
}
