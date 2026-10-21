use pyo3::{pyfunction, pymodule};

mod audio_engine;
mod messages;
mod selected_bpm;

#[cfg(test)]
mod test_memory;

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
    use super::audio_engine::SealedProcessingFiles;
    #[pymodule_export]
    use super::audio_engine::portable_relocate_data;
    #[pymodule_export]
    use super::audio_engine::resolve_migration_config_reference;
    #[pymodule_export]
    use super::audio_engine::resolve_project_asset;
    #[pymodule_export]
    use super::audio_engine::resolve_project_asset_details;
    #[pymodule_export]
    use super::native_build_profile;
    #[pymodule_export]
    use super::selected_bpm::summarize_selected_bpm_json;

    #[pymodule_export]
    use super::audio_engine::AudioEngine;

    #[pymodule_export]
    use super::audio_engine::SourceBeatmap;

    #[pymodule_export]
    use super::audio_engine::SourceBeatmapArtifact;

    #[pymodule_export]
    use super::audio_engine::MixCaptureTicket;

    #[pymodule_export]
    use super::audio_engine::inspect_mix_capture_spool;

    #[pymodule_export]
    use super::audio_engine::BankMutationAcknowledgement;
    #[pymodule_export]
    use super::audio_engine::BankMutationTicket;
    #[pymodule_export]
    use super::audio_engine::BankParticipants;
    #[pymodule_export]
    use super::audio_engine::CapturedBankMutation;
    #[pymodule_export]
    use super::audio_engine::CapturedPadCopy;
    #[pymodule_export]
    use super::audio_engine::CapturedPadPair;
    #[pymodule_export]
    use super::audio_engine::LoaderEventDelivery;
    #[pymodule_export]
    use super::audio_engine::PadCopyTicket;
    #[pymodule_export]
    use super::audio_engine::PadPairAcknowledgement;
    #[pymodule_export]
    use super::audio_engine::PadPairTicket;
    #[pymodule_export]
    use super::audio_engine::RecoveredBankAcceptance;
    #[pymodule_export]
    use super::audio_engine::RecoveredCopyAcceptance;
    #[pymodule_export]
    use super::audio_engine::RecoveredPairAcceptance;

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
    use super::audio_engine::OfflineStemSourceLease;

    #[pymodule_export]
    use super::audio_engine::InstrumentalStemReader;
    #[pymodule_export]
    use super::audio_engine::PreparedSourceTicket;
    #[pymodule_export]
    use super::audio_engine::PreparedStemPair;

    #[pymodule_export]
    use super::audio_engine::CommittedStemSetTicket;
    #[pymodule_export]
    use super::audio_engine::ResidentStemTicket;

    #[pymodule_export]
    use super::audio_engine::ConstantTimingTicket;

    #[pymodule_export]
    use super::audio_engine::CapturedConstantTiming;

    #[pymodule_export]
    use super::audio_engine::AcceptedTimingRefreshTicket;

    #[pymodule_export]
    use super::audio_engine::SavedConstantTimingTicket;

    #[pymodule_export]
    use super::audio_engine::CapturedGlobalStart;
    #[pymodule_export]
    use super::audio_engine::CapturedInputPadTrigger;
    #[pymodule_export]
    use super::audio_engine::CapturedPadStart;
    #[pymodule_export]
    use super::audio_engine::CapturedPitchAction;
    #[pymodule_export]
    use super::audio_engine::HoldActionTicket;
    #[pymodule_export]
    use super::audio_engine::InputRuntimePadBinding;

    #[pymodule_export]
    use super::audio_engine::GlobalPlaybackBatchTicket;

    #[pymodule_export]
    use super::audio_engine::ScalarSourceGrid;

    #[pymodule_export]
    use super::messages::AudioMessage;
}
