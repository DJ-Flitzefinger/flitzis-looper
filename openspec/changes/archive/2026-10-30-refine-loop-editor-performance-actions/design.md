# Design

## Published seams and ownership

Published baseline `e9da7e1ac45b767bcf4dfda491d9a485474029ff` has one raw/envelope
waveform branch in `ui/render/waveform_editor.py`, always-set Y limits, number
labels projected through `plot_to_pixels(source_s, 1.0)`, and overlays on the plot
draw list. The previous hardware-free test
`src/tests/flitzis_looper/ui/test_waveform_render_stability.py` inspects real
ImGui/ImPlot frames, but renders `_render_plot` without the complete toolbar and
does not cover the reported font/DPI/number-row transition. Its historical pass
does not establish E11-04.

The pre-fix productive full-editor regression executed real hovered wheel input
in both context-isolation modes. X limits changed, while plot Y changed
73 -> 89 -> 73 and the actual upper glyph origin changed 77 -> 93 -> 77; the
plot bottom stayed 728. Each frame still had one visible number band, at most
one waveform branch and valid clipped draw commands. The demonstrated fault is
vertical layout movement, not a proven second renderer or GPU-clipping defect.
Readiness was read before `get_render_data` updated it later in the same frame;
adding/removing the status row above the remaining-height plot changed Y/height
one frame after a request/completion. The reported Human perceptual duplicate
and its exact binary remain unverified until final Human repetition.

The first-frame matrix also demonstrated an independent four-pixel plot-bottom
shrink. ImPlot's padded single-space playhead tag applied `OverrideSizeLate` at
EndPlot and changed the retained tick height for the following frame. Set this
plot's vertical annotation padding to zero while retaining horizontal padding
and the existing visible/invisible playhead tag. This removes that measured late
layout change; it is not evidence of a clipping defect or another waveform.

Reuse `ctx.ui.waveform` cache/source fences, current scalar coordinates and the
shared editor close/release authority. Reserve two above-plot status/control
slots at current text/Retry/Cancel height, including idle/ready frames. Preserve
truthful preparation/errors and actionable Retry/Cancel, and scope the zero
vertical annotation padding to this plot. Preserve source-coordinate labels and
balanced existing clipping absent a measured need. A genuine zoom or resize
invalidates the view cache; stable frames and source replacements retain the
existing appropriate cache behavior. Source coordinates, grid origin, loop/seek
markers, analysis backend, transport and DSP do not change.

Baseline `sidebar_left._render_loaded_actions` directly called unload/analyze and
placed Adjust below them. Move only Adjust into the section after `_render_pad_header`
and before `_render_bpm`. Reuse the authoritative open/toggle action and close
path. The new close label follows Grid Offset in the toolbar; at narrow widths
the layout may wrap the control group while retaining label and hit target.
It must not reintroduce a far-right icon-only close requirement.

There is no existing shared warning/modal adapter in the baseline. A focused
transient confirmation intent serves both actions, using the independent central
dialog-render and target/cleanup ownership pattern already used by file dialogs.
It remains visible independently of sidebar selection/content. It captures action,
pad ID, `pad_content.instance_id`, path and `waveform_source_identity` native
generation/digest/frames/rate; path/digest alone cannot fence equal-byte ABA reload.
Acceptance revalidates the same target/current eligibility and consumes the
intent once before invoking the existing `LoaderController.unload_sample`
or `analyze_sample_async`. Native unload admission remains before Python cleanup,
so failed admission preserves the editor and pending/claimed mode transaction.

Cancel/Escape/dismissal own only the warning intent, never `cancel_residency` or
`cancel_requested`. Same-assignment KEYLOCK request/status/window readiness is
not content identity and introduces no new manual-analysis veto. Unload/rebind,
replacement/rearrangement/content-instance or lost native identity rejects old
acceptance; selection changes cannot retarget it. Sidebar and mapped keyboard/MIDI
performer paths request this same warning; Learn remains capture-only. Automatic
load analysis, saved-result restore and internal lifecycle cleanup remain direct.
The existing manual/grid/loop analysis-preservation policy is unchanged.

Sidebar and mapped Adjust previously maintained different editor-close behavior.
All open/retarget/close inputs now use one bounded editor authority with view
release; toolbar close uses it too. No second flag or general framework is added.
Keep the existing `key_lock_status` selectors and shared effective-color versus
pending/error/mixed-text button renderer, including their actual sidebar rows.
Its muted status text wraps at narrow/scaled sidebar widths so pending/error
feedback remains visible without altering effective button color or cancellation.

## Bounded delivery and real gates

`X11-UI` can use the accepted UI/source snapshot without waiting for whole P4b.
It precedes integrated V0/H-LIVE/H-FINAL/C-FINAL; the central program records the
exact DAG. Implementation must produce:

- Actual native headless draw frames for the complete editor including toolbar,
  zoom buttons/wheel, labels, pending/ready/error states, raw/envelope crossover,
  source replacement and playback overlays. Test repeated zoom in/out at 100%,
  125%, 150% and 200% display/font scaling, ordinary and narrow window sizes,
  initial/loop/full/tail views, and both existing context-isolation modes.
- Recorded plot rectangle, label baseline, draw-command clip rectangles, cache
  query identity and real vertices/indices. Check at most one waveform branch and
  visible number band per frame, exactly one only when matching valid waveform
  data and visible major/Loop-1 label candidates respectively exist. Pending/error
  data=None or a view without label candidates must not invent geometry. Check
  visible clipped geometry rather than raw outside vertices; require no shifted
  duplicate, no vertical plot/row alternation during horizontal zoom, no emergent scrollbar,
  and stable settled geometry. Compare normalized geometry at scaled sizes;
  whole-frame equality alone cannot identify a duplicate or its location.
- Controller/action/UI regressions proving warnings cannot act before accept,
  Cancel/Escape are no-ops, stale content fails safely, selection changes cannot
  unload/analyze the new selection, equal-byte ABA/native identity is fenced,
  loading races remain ineligible, and eligible confirmed requests execute once.
  Opening/cancelling warnings during pending/claimed KEYLOCK and failed native
  unload must preserve transaction/readers/feedback/playback and edited-pad state.
  Real keyboard/MIDI/Learn inputs must exercise the shared warning and close paths.
- Actual layout/order and gesture assertions for Adjust and labeled close,
  including minimum hit targets, shared toggle/close dispatch and unchanged
  playback/loop/grid state. Existing sidebar/context tests are the extension seam.
- Independent final source/diff review, official strict validation and later
  final Human visual repetition of the reported zoom case. Headless software
  geometry does not establish Human visual acceptance or an exact Human binary.

Software task completion and independent final review are recorded separately
from the still-open H-FINAL Human visual gate. Whole P4b/P5a, resource/music/history,
later extension, device/hearing and Goal completion do not follow from X11-UI.
