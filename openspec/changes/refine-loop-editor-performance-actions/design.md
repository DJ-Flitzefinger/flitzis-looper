# Design

## Published seams and ownership

Baseline `694cd61e50a3fc81bc6a41d88269f6458c4ae269` has one raw/envelope
waveform branch in `ui/render/waveform_editor.py`, always-set Y limits, number
labels projected through `plot_to_pixels(source_s, 1.0)`, and overlays on the plot
draw list. The current hardware-free test
`src/tests/flitzis_looper/ui/test_waveform_render_stability.py` inspects real
ImGui/ImPlot frames, but renders `_render_plot` without the complete toolbar and
does not cover the reported font/DPI/number-row transition. Its historical pass
does not establish E11-04.

Reuse `ctx.ui.waveform` cache/source fences, current scalar coordinates and
`ctx.ui.waveform.close()`. Correct the reproduced geometry/cache/clip fault,
including label placement relative to the actual plot rectangle, without
speculating that a second waveform exists. Readiness rows must remain above the
remaining-height plot to avoid scrollbar/resolution feedback. A genuine zoom or
resize invalidates the view cache; stable frames and source replacements retain
the existing appropriate cache behavior.

`sidebar_left._render_loaded_actions` directly calls unload/analyze and places
Adjust below them. Move only Adjust into the section after `_render_pad_header`
and before `_render_bpm`. Reuse the authoritative open/toggle action and close
path. The new close label follows Grid Offset in the toolbar; at narrow widths
the layout may wrap the control group while retaining label and hit target.
It must not reintroduce a far-right icon-only close requirement.

Warnings capture action kind, pad ID and current source/content revision when
opened. Controller acceptance rechecks the captured binding against current
loaded content and eligibility. Selection changes cannot retarget an accepted
dialog; unload/replacement/rearrangement invalidates it. Cancel/Escape and stale
acceptance perform no unload or analysis. Automatic load analysis and internal
lifecycle cleanup retain their existing paths; the warning belongs to explicit
performer actions. A shared small confirmation adapter can serve both actions.

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
  query identity and real vertices/indices. Check one waveform branch and one
  number-row baseline per frame, no out-of-clip duplicated label/waveform draw,
  no vertical plot/row alternation during horizontal zoom, no emergent scrollbar,
  and stable settled geometry. Compare normalized geometry at scaled sizes;
  whole-frame equality alone cannot identify a duplicate or its location.
- Controller/action/UI regressions proving warnings cannot act before accept,
  Cancel/Escape are no-ops, stale content fails safely, selection changes cannot
  unload/analyze the new selection, and eligible confirmed requests execute once.
- Actual layout/order and gesture assertions for Adjust and labeled close,
  including minimum hit targets, shared toggle/close dispatch and unchanged
  playback/loop/grid state. Existing sidebar/context tests are the extension seam.
- Independent final source/diff review, official strict validation and later
  final Human visual repetition of the reported zoom case. Headless software
  geometry does not establish Human visual acceptance or an exact Human binary.

No production runtime/test/visual PASS is claimed by this planning change.
