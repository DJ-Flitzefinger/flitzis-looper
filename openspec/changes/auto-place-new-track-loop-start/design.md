# Initial signal boundary and editable grid alignment

The native decode/resample/channel-map worker already owns immutable loaded PCM.
One borrowed scan with constant extra memory finds the first frame where any
finite channel exceeds the fixed symmetric deadzone [-0.01, +0.01] full scale. Equality
remains inside this symmetric band. Return the preceding frame, saturating at
zero, divided by loaded rate as f64 seconds. This replaces fixed 5-ms pre-roll.
Stereo never cancels through mono averaging. Nonfinite samples do not establish
activity; a nonfinite predecessor is not evidence of silence. Noise, pickups
and soft fades remain heuristic limits. The user's zoomed screenshot shows small
precursor ripples around +/-0.005 FS; +/-0.01 is the next visually informed test
value, not an exact pixel-derived sample boundary or an accepted calibration.
It replaces the earlier peak-relative max(1e-5 FS, peak*0.001) tolerance, which
triggered too early. Later loud peaks cannot change the chosen threshold. Tracks
entirely inside the band have no candidate and retain ordinary loop-zero/legacy
grid fallback; do not silently lower the deadzone for quiet tracks.

Python consumes the optional candidate under existing source/request guards only
for new assignments. It initializes loop start and pad_grid_anchor_s together,
with zero manual offset and Auto 8 bars. The optional source-time base is separate
from pad_grid_offset_samples: encoding the base as an offset would lose long
leading silence under the one-bar clamp and later BPM edits. Canonical scalar
origin is the rounded persisted base plus signed offset; absent base retains
analysis downbeat/beat/zero fallback. No near-start analysis snap touches the base.

Restore publishes the base even without analysis/BPM. Same-path reload, reanalysis
and loop edits preserve it; unload/new assignment clears it. Legacy JSON without
the field remains compatible. Raw analyzer data and BPM are unchanged. After
initialization, grid and loop remain independently editable. Native timing uses
the same signed origin through the existing control-side command.

An attack can seed an editable grid without certifying a downbeat or enabling SYNC.
At normal BPM, file start and an attack only milliseconds later cannot represent
two successive beats. The user clarified that line 0 is the invisible left edge;
line 1 is one regular beat to its right at the initial attack/loop. Initial/reset
and loop-focused editor views therefore begin one beat before the selected loop.
Negative source time is virtual display space, not padded audio; a long physical
intro can be panned back into view. PCM reads, seeks and physical markers stay
inside source bounds. Line 0 itself is not drawn.

Numbering is 1+(line_time-loop_start)/beat_seconds, not an adaptive minor-line
index or fixed track-start number. Fractional/off-grid starts keep fractional
labels without reanchoring physical lines. Label rendering stays bounded and
readable, with loop-start reference 1 identifiable at coarse zoom.

Future B3/B7 use q(s)=B(s)-B(loop_start) and continuous native master/map phase;
UI integers are not timing identities. For accepted map start b0 and loop length
L, loop-relative SYNC maps via S(b0+mod(M+phi_loop,L)). This clarifies future intent
only; existing live launch/phase behavior and acceptance gates remain unchanged.

No callback work, extra inference, source/stem-origin change, dependency/model
change or default-analysis activation. B2b1 lossless publication remains next.
