//! Shared parameter-free normalized decoder fixture, never learned execution.
use flitzis_stem_dsp::demucs::{BINS, Error, hybrid::Geometry};
pub fn predict(
    g: Geometry,
    spectral: &[f32],
    time: &[f32],
    decoded_spectral: &mut [f32],
    decoded_time: &mut [f32],
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Error> {
    let steps = g.spectral_steps();
    let spectrum_len = 2 * BINS * steps;
    let frames = g.training_frames();
    for batch in 0..g.batches() {
        for source in 0..g.sources() {
            for channel in 0..g.channels() {
                let input_lane = batch * g.channels() + channel;
                let output_lane = (batch * g.sources() + source) * g.channels() + channel;
                let gain = (source + 1) as f32;
                let label = (1 + batch * 16 + source * 3 + channel) as f32;
                for part in 0..2 {
                    for bin in 0..BINS {
                        let offset = (part * BINS + bin) * steps;
                        let mark = match (part, bin) {
                            (0, 3) => 8.0,
                            (1, 7) => -6.0,
                            (0, bin) if bin == BINS - 1 => 3.0,
                            _ => 0.0,
                        };
                        if cancelled() {
                            return Err(Error::Cancelled);
                        }
                        for step in 0..steps {
                            let i = offset + step;
                            let patterned = (step % 5) as f32 * 0.125 + label * 0.0625;
                            decoded_spectral[output_lane * spectrum_len + i] =
                                spectral[input_lane * spectrum_len + i] * (gain * 0.25)
                                    + mark * patterned;
                        }
                    }
                }
                for i in 0..frames {
                    if i.is_multiple_of(4096) && cancelled() {
                        return Err(Error::Cancelled);
                    }
                    decoded_time[output_lane * frames + i] = time[input_lane * frames + i]
                        * (gain * -0.125)
                        + ((i % 17) as f32 * 0.03125 + label * 0.0625);
                }
            }
        }
    }
    Ok(())
}
