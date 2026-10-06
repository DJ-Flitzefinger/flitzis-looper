//! Conservative full-source PCM feature evidence, entirely outside realtime audio.
//!
//! Exact repeated isolated threshold attacks support a discrete feature/count
//! correspondence. They do not determine musical quarter units or a universal
//! beat onset. Independent source-bound quarter assertions are a separate input.

mod attacks;
mod counts;
mod types;

use sha2::{Digest, Sha256};

use crate::tempo_evidence::f32_pcm_sha256;
use crate::tempo_summary::{MAX_RAW_POSITIONS, SourceIdentity};

pub use types::*;

/// Refine complete detector evidence using independently scanned loaded PCM.
///
/// PCM must be the complete loaded-rate mono float32 stream identified by its
/// little-endian SHA-256. No expected period, fitted grid, crop or nearest-integer
/// BPM enters the scan or correspondence. The fixed half-frame bound applies
/// only to the repeated discrete threshold feature, not general musical onset.
pub fn refine_comparable_attacks(
    source: &SourceIdentity,
    pcm: &[f32],
    raw_beat_seconds: &[f64],
    independent_origin_seconds: f64,
    search_halfwidth_seconds: f64,
    independent_quarters: Option<&IndependentQuarterEvidence>,
) -> Result<TempoRefinement, TempoRefinementError> {
    validate_inputs(
        source,
        pcm,
        raw_beat_seconds,
        independent_origin_seconds,
        search_halfwidth_seconds,
    )?;
    let scan = attacks::scan(pcm, source.loaded_sample_rate_hz)?;
    if let Some(evidence) = independent_quarters {
        counts::validate_independent(source, &scan.attacks, evidence)?;
    }
    let (raw_associations, matching_reasons) =
        counts::associate(raw_beat_seconds, &scan.attacks, search_halfwidth_seconds);
    let mut rejection_reasons = scan.reasons;
    rejection_reasons.extend(matching_reasons);
    let mut matched = vec![false; scan.attacks.len()];
    for association in &raw_associations {
        if let Some(index) = association.attack_index {
            matched[index] = true;
        }
    }
    let unmatched_attack_indices = matched
        .iter()
        .enumerate()
        .filter_map(|(index, &matched)| (!matched).then_some(index))
        .collect();
    let proposals = if rejection_reasons.is_empty() {
        counts::proposals(
            source,
            &scan.attacks,
            &raw_associations,
            independent_quarters,
        )?
    } else {
        Vec::new()
    };
    let mut refined_source = source.clone();
    let feature_timing_error_halfwidth_seconds = rejection_reasons
        .is_empty()
        .then_some(0.5 / f64::from(source.loaded_sample_rate_hz));
    if let Some(halfwidth) = feature_timing_error_halfwidth_seconds {
        refined_source.timing_error_halfwidth_seconds = halfwidth;
        refined_source.raw_revision = feature_revision(
            source,
            raw_beat_seconds,
            search_halfwidth_seconds,
            &scan.attacks,
            scan.shape_sha256.as_deref().expect("comparable shape"),
        );
    }
    Ok(TempoRefinement {
        policy_version: POLICY_VERSION,
        status: if rejection_reasons.is_empty() {
            RefinementStatus::ComparableAttacks
        } else {
            RefinementStatus::Unsupported
        },
        rejection_reasons,
        original_source: source.clone(),
        refined_source,
        feature_timing_error_halfwidth_seconds,
        independent_origin_seconds,
        raw_beat_seconds: raw_beat_seconds.to_vec(),
        search_halfwidth_seconds,
        attacks: scan.attacks,
        raw_associations,
        unmatched_attack_indices,
        attack_shape_sha256: scan.shape_sha256,
        proposals,
    })
}

fn feature_revision(
    source: &SourceIdentity,
    raw: &[f64],
    halfwidth: f64,
    attacks: &[ComparableAttack],
    shape_digest: &str,
) -> String {
    let mut revision = Sha256::new();
    for text in [
        POLICY_VERSION,
        &source.source_sha256,
        &source.pcm_sha256,
        &source.backend_revision,
        &source.configuration_revision,
        &source.raw_revision,
        shape_digest,
    ] {
        revision.update((text.len() as u64).to_le_bytes());
        revision.update(text.as_bytes());
    }
    revision.update(source.loaded_sample_rate_hz.to_le_bytes());
    revision.update(source.loaded_frame_count.to_le_bytes());
    revision.update(halfwidth.to_le_bytes());
    revision.update((raw.len() as u64).to_le_bytes());
    for seconds in raw {
        revision.update(seconds.to_le_bytes());
    }
    revision.update((attacks.len() as u64).to_le_bytes());
    for attack in attacks {
        revision.update(attack.frame.to_le_bytes());
        revision.update(attack.end_frame_exclusive.to_le_bytes());
    }
    format!("{POLICY_VERSION}:{:x}", revision.finalize())
}

fn validate_inputs(
    source: &SourceIdentity,
    pcm: &[f32],
    raw: &[f64],
    origin: f64,
    halfwidth: f64,
) -> Result<(), TempoRefinementError> {
    let digest = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    let text = |value: &str| !value.trim().is_empty() && value.len() <= 4096;
    if !digest(&source.source_sha256)
        || !digest(&source.pcm_sha256)
        || !text(&source.backend_revision)
        || !text(&source.configuration_revision)
        || !text(&source.raw_revision)
        || !(8_000..=768_000).contains(&source.loaded_sample_rate_hz)
        || source.loaded_frame_count == 0
        || source.loaded_frame_count != pcm.len() as u64
        || std::mem::size_of_val(pcm) > MAX_PCM_BYTES
        || raw.len() > MAX_RAW_POSITIONS
        || !origin.is_finite()
        || !halfwidth.is_finite()
        || !(0.0..=MAX_SEARCH_HALFWIDTH_SECONDS).contains(&halfwidth)
        || !source.timing_error_halfwidth_seconds.is_finite()
        || source.timing_error_halfwidth_seconds < 0.0
    {
        return Err(TempoRefinementError(
            "invalid PCM source identity or refinement bounds",
        ));
    }
    let duration = pcm.len() as f64 / f64::from(source.loaded_sample_rate_hz);
    if source.timing_error_halfwidth_seconds > duration {
        return Err(TempoRefinementError(
            "raw timing bound exceeds source duration",
        ));
    }
    let mut previous = -1.0;
    for &seconds in raw {
        if !seconds.is_finite() || seconds < 0.0 || seconds >= duration || seconds <= previous {
            return Err(TempoRefinementError(
                "invalid complete raw source coordinates",
            ));
        }
        previous = seconds;
    }
    if pcm.iter().any(|sample| !sample.is_finite()) {
        return Err(TempoRefinementError("nonfinite loaded PCM"));
    }
    if f32_pcm_sha256(pcm) != source.pcm_sha256 {
        return Err(TempoRefinementError(
            "loaded PCM digest does not match source identity",
        ));
    }
    Ok(())
}
