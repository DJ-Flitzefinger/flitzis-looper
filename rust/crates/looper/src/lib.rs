use pyo3::{pyfunction, pymodule};

mod audio_engine;
mod messages;
mod selected_bpm;

/// Report the profile of this loaded native module without opening an engine.
#[pyfunction]
fn native_build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// The Python module implemented in Rust.
#[pymodule]
mod flitzis_looper_audio {
    #[pymodule_export]
    use super::audio_engine::resolve_project_asset;
    #[pymodule_export]
    use super::native_build_profile;
    #[pymodule_export]
    use super::selected_bpm::summarize_selected_bpm_json;

    #[pymodule_export]
    use super::audio_engine::AudioEngine;

    #[pymodule_export]
    use super::audio_engine::ProjectAssetLease;

    #[pymodule_export]
    use super::audio_engine::MaterialMigrationHold;
    #[pymodule_export]
    use super::audio_engine::MaterialMigrationJournalStore;
    #[pymodule_export]
    use super::audio_engine::MaterialMigrationPreparation;
    #[pymodule_export]
    use super::audio_engine::MaterialMigrationSourceTicket;
    #[pymodule_export]
    use super::audio_engine::MaterialMigrationStemPreparation;

    #[pymodule_export]
    use super::audio_engine::MigrationArtifactLease;
    #[pymodule_export]
    use super::audio_engine::MigrationInventoryLease;
    #[pymodule_export]
    use super::audio_engine::MigrationProjectGuard;

    #[pymodule_export]
    use super::audio_engine::OfflineAnalysisJob;

    #[pymodule_export]
    use super::audio_engine::InstrumentalStemReader;
    #[pymodule_export]
    use super::audio_engine::PreparedSourceTicket;
    #[pymodule_export]
    use super::audio_engine::PreparedStemPair;

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
