use crate::trial_lifetime::{Context, terminate_during};
use ort::{
    ep::ExecutionProviderDispatch,
    session::{
        Session,
        builder::{AutoDevicePolicy, SessionBuilder},
    },
};
use sha2::{Digest, Sha256};

#[cfg(feature = "demucs-inference-trial")]
pub(crate) fn session_builder(
    providers: &[ExecutionProviderDispatch],
) -> Result<SessionBuilder, String> {
    session_builder_observed(providers, |_, _| {})
}

pub(crate) fn session_builder_observed(
    providers: &[ExecutionProviderDispatch],
    observe_registration_failure: impl FnOnce(ort::ErrorCode, &str),
) -> Result<SessionBuilder, String> {
    Session::builder()
        .map_err(|e| e.to_string())?
        .with_auto_device(AutoDevicePolicy::Default)
        .map_err(|e| e.to_string())?
        .with_execution_providers(providers)
        .map_err(|error| {
            observe_registration_failure(error.code(), error.message());
            error.to_string()
        })?
        .with_intra_threads(1)
        .map_err(|e| e.to_string())?
        .with_inter_threads(1)
        .map_err(|e| e.to_string())?
        .with_parallel_execution(false)
        .map_err(|e| e.to_string())?
        .with_intra_op_spinning(false)
        .map_err(|e| e.to_string())?
        .with_inter_op_spinning(false)
        .map_err(|e| e.to_string())
}

#[cfg(feature = "demucs-inference-trial")]
pub(crate) fn commit(
    builder: SessionBuilder,
    graph: &[u8],
    context: Option<&Context>,
) -> Result<Session, String> {
    commit_observed(builder, graph, context, |_, _| {})
}

pub(crate) fn commit_observed(
    mut builder: SessionBuilder,
    graph: &[u8],
    context: Option<&Context>,
    observe_initialization_failure: impl FnOnce(ort::ErrorCode, &str),
) -> Result<Session, String> {
    let canceler = builder.canceler();
    let work = || {
        builder.commit_from_memory(graph).map_err(|error| {
            observe_initialization_failure(error.code(), error.message());
            error.to_string()
        })
    };
    if let Some(context) = context {
        // Cancel the exact native options. Join before releasing options or graph.
        terminate_during(
            context,
            None,
            || canceler.cancel().map_err(|e| e.to_string()),
            work,
        )
        .0
    } else {
        work()
    }
}

pub(crate) fn sha256(
    data: &[u8],
    checkpoint: impl Fn() -> Result<(), String>,
) -> Result<String, String> {
    let mut hash = Sha256::new();
    for block in data.chunks(65536) {
        checkpoint()?;
        hash.update(block);
    }
    Ok(format!("{:x}", hash.finalize()))
}

// Exact ORT 1.24.4 termination status. Preserve all other native errors.
pub(crate) fn is_run_termination(code: ort::ErrorCode, message: &str) -> bool {
    code == ort::ErrorCode::GenericFailure
        && message == "Exiting due to terminate flag being set to true."
}
