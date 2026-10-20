## Stable control geometry

Rate feedback is an asynchronous projection, so it must not participate in
the fader's layout. The BPM display owns the existing hover tooltip and can
include requested and confirmed rate information without reserving dynamic rows.
Key Lock retains an intent-colored button and a fixed single-line audio state;
additional non-error state information can use hover details.

The fader height is computed from the sidebar viewport, the current font metrics
and a fixed count of control rows. Receipt states, error strings, pointer state
and scrollbar appearance are not inputs. Normal supported window heights keep
the step, Reset and lock buttons visible. Small windows may scroll, while the
geometry remains the same for the same viewport and scale.

## Language

The two policy values and persistence keys are unchanged. Only the shared
display labels and Settings explanations become English.

## Validation

Actual ImGui frames cycle idle, queued, preparing, applied and rejected feedback,
including long diagnostic strings. Tests record native item rectangles and
sidebar geometry, hold a real mouse drag stationary, and check that feedback
changes cannot move the fader or alter the speed. No AudioEngine is constructed.
