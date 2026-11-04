//! Off-RT terminalization after the sole callback image irreversibly ends.

use super::RtMixer;

impl RtMixer {
    pub(in crate::audio_engine) fn retire_output_obligations(&self, frame: u64) {
        for work in self.hold_actions.iter().flatten() {
            work.action.settle_after_callback_retirement(frame);
        }
        if let Some(message) = self.deferred_control() {
            crate::audio_engine::audio_stream::retire_control_after_callback_end(message, frame);
        }
        if let Some(runtime) = &self.pitch_runtime {
            runtime.retire_after_callback_close();
        }
        for receipt in self.committed_stem_receipts.iter().flatten() {
            receipt.status.retire_unproved_output_mode();
        }
    }
}
