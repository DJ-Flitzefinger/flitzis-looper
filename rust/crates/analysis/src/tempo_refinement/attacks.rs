//! Complete bounded PCM scan, independent of any expected period or beat grid.

use crate::tempo_evidence::f32_pcm_sha256;

use super::{ComparableAttack, MAX_ATTACKS, RefinementRejection, TempoRefinementError};

const THRESHOLD_FULL_SCALE: f32 = 0.01;
const MIN_SILENCE_SECONDS: f64 = 0.005;
const MAX_ACTIVE_SECONDS: f64 = 0.1;

pub(super) struct Scan {
    pub attacks: Vec<ComparableAttack>,
    pub reasons: Vec<RefinementRejection>,
    pub shape_sha256: Option<String>,
}

pub(super) fn scan(pcm: &[f32], rate: u32) -> Result<Scan, TempoRefinementError> {
    let silence_frames = (f64::from(rate) * MIN_SILENCE_SECONDS).ceil() as usize;
    let max_active_frames = (f64::from(rate) * MAX_ACTIVE_SECONDS).floor() as usize;
    let mut attacks = Vec::new();
    let mut start = None;
    let mut last_active = 0;
    for (frame, &sample) in pcm.iter().enumerate() {
        if sample.abs() > THRESHOLD_FULL_SCALE {
            start.get_or_insert(frame);
            last_active = frame;
        } else if let Some(begin) = start
            && frame - last_active >= silence_frames
        {
            push_attack(&mut attacks, begin, last_active + 1, rate)?;
            start = None;
        }
    }
    let mut reasons = Vec::new();
    if let Some(begin) = start {
        push_attack(&mut attacks, begin, last_active + 1, rate)?;
        reasons.push(RefinementRejection::IncompleteTrailingSilence);
    }
    if attacks.len() < 2 {
        reasons.push(RefinementRejection::InsufficientAttacks);
    }
    if attacks
        .iter()
        .any(|attack| attack.end_frame_exclusive - attack.frame > max_active_frames as u64)
    {
        reasons.push(RefinementRejection::ActiveEventTooLong);
    }
    if attacks
        .first()
        .is_some_and(|attack| attack.frame != 0 && attack.frame < silence_frames as u64)
    {
        reasons.push(RefinementRejection::IncompleteLeadingSilence);
    }
    let mut shape_sha256 = None;
    if let Some(first) = attacks.first() {
        let template = &pcm[first.frame as usize..first.end_frame_exclusive as usize];
        if attacks.iter().skip(1).any(|attack| {
            let shape = &pcm[attack.frame as usize..attack.end_frame_exclusive as usize];
            shape.len() != template.len()
                || shape
                    .iter()
                    .zip(template)
                    .any(|(left, right)| left.to_bits() != right.to_bits())
        }) {
            reasons.push(RefinementRejection::DifferentAttackShapes);
        } else {
            shape_sha256 = Some(f32_pcm_sha256(template));
        }
    }
    Ok(Scan {
        attacks,
        reasons,
        shape_sha256,
    })
}

fn push_attack(
    attacks: &mut Vec<ComparableAttack>,
    start: usize,
    end: usize,
    rate: u32,
) -> Result<(), TempoRefinementError> {
    if attacks.len() == MAX_ATTACKS {
        return Err(TempoRefinementError("PCM attack limit exceeded"));
    }
    attacks.push(ComparableAttack {
        frame: start as u64,
        seconds: start as f64 / f64::from(rate),
        end_frame_exclusive: end as u64,
        source_boundary: start == 0,
    });
    Ok(())
}
