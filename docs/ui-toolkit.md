# UI Toolkit: Dear ImGui Guidelines

Flitzis Looper uses Dear ImGui through `imgui-bundle`. ImGui is
immediate-mode: every frame describes the UI again from current application
state, and ImGui resolves interaction from current input plus stable widget IDs.

This document defines the project-specific UI architecture. It is not a generic
ImGui tutorial. The goal is a responsive performance UI that stays thin,
predictable, and testable while the domain logic, persistence, background work,
and realtime audio engine remain outside rendering code.

## Goals

- Keep the domain core deterministic, testable, and independent from ImGui.
- Keep render functions thin: layout, display, and input-to-action mapping.
- Make every user action explicit through `UiContext`.
- Keep per-frame work bounded and predictable.
- Keep the UI away from the realtime audio callback and live Rust audio state.

## Core/UI Separation Pattern (required)

ImGui works best when the UI is a per-frame adapter around application state and
explicit actions. In this project, the "core" is not one reducer function. It is
the combination of Pydantic state models, Python controllers, persistence,
background workers, and the Rust `AudioEngine` boundary.

```text
+------------------------------------------------------------------+
|                            Domain Core                           |
|                                                                  |
|  ProjectState       durable performer intent, saved to disk       |
|  SessionState       transient runtime/UI projection               |
|  Controllers        validation, orchestration, side effects        |
|  AudioEngine        Rust realtime engine behind bounded APIs       |
+-----------------------------+------------------------------------+
                              ^
                              | read-only selectors and snapshots
                              |
                              | explicit actions / intent
                              v
+-----------------------------+------------------------------------+
|                                  UI                              |
|                                                                  |
|  render/*.py        layout, widgets, visual state, input mapping  |
|  UiContext          read-only state facade + action facade        |
|  ImGui              immediate-mode frame renderer                 |
+------------------------------------------------------------------+
```

Render functions must read through `ctx.state` and emit intent through
`ctx.audio`, `ctx.ui`, or `ctx.input`. They must not reach around `UiContext` to
mutate project state, call low-level audio APIs, or perform slow work directly.

## Current UI Runtime Flow

The UI shell starts in `src/flitzis_looper/ui/run.py`:

1. Create `AppController`.
2. Create `UiContext(controller)`.
3. Configure Hello ImGui/ImmApp runner parameters.
4. Register `render_ui(context)` as the per-frame callback.
5. Gate close requests in `after_swap` through nonblocking
   `controller.prepare_shutdown`, retaining the frame loop and a visible pending
   message until Copy's actual worker/Native/config ownership has settled.
6. Register `controller.shut_down` for permitted exit; teardown never bypasses
   pending Copy ownership. See [pad content copies](../openspec/changes/archive/2026-10-30-add-rearrange-shared-pad-content/evidence/pad-content-copy.md).

`render_ui()` in `src/flitzis_looper/ui/render/render.py` is the canonical frame
entrypoint:

```text
render_ui(ctx)
-> ctx.on_frame_render()
   -> controller.on_frame_render()
      -> controller-owned per-frame callbacks
-> poll keyboard input for Learn/mapped actions
-> ctx.audio.poll.poll()
   -> loader events + audio telemetry messages
-> draw main layout, settings/performance view, bottom bar
-> draw waveform editor and independent file/performer-confirmation dialogs when applicable
-> ctx.persistence.maybe_flush()
```

This ordering matters:

- controller-owned frame updates happen before drawing,
- keyboard/MIDI mapping intent is captured before widgets are drawn,
- loader and audio telemetry are folded into `SessionState` before visual
  meters/playheads are rendered,
- persistence flushing is outside individual widgets.

## Project File Map

```text
src/flitzis_looper/ui/run.py
    UI runner setup, fonts, Hello ImGui/ImmApp integration.

src/flitzis_looper/ui/context.py
    Public UI boundary. Provides read-only state selectors and grouped action
    facades for audio, UI, waveform, settings, and input mapping.

src/flitzis_looper/ui/render/
    Immediate-mode render functions for the main surface, sidebars, bottom bar,
    settings, waveform editor, file dialog, and control gestures.

src/flitzis_looper/controller/
    Application controllers. Own validation, state mutation, persistence marks,
    background orchestration, audio API calls, and runtime event polling.

src/flitzis_looper/models.py
    Pydantic `ProjectState` and `SessionState`.

src/flitzis_looper_audio/
    Python import boundary for the Rust audio extension.
```

## State Ownership

The [archived pad-owned PCM program](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/pad-owned-pcm-program.md)
retains historical requirements and incomplete gates, not a pending execution
queue. Current [stems](architecture/stems.md) and [media/PCM](architecture/media-and-pcm.md)
separate saved intent, disk eligibility, residency, pending and effective audio.
Controllers request the required admitted components; disk existence alone is
not Native readiness. FULL MIX and actual own feedback remain truthful while
preparing. Rendering uses snapshots and does not load PCM or poll workers.

New-track loading initializes loop and an independent persisted scalar grid base
at the loaded frame immediately before the first crossing of the symmetric
fixed deadzone [-0.01, +0.01] full scale, without millisecond pre-roll or scaling
the threshold to track peak. This widened value follows the user's screenshot
test; tracks entirely inside the band use the existing no-candidate fallback.
The background native candidate keeps Auto 8 bars; missing activity retains loop
zero and legacy grid fallback. BPM and raw analysis remain independent. Restore,
same-path reload and Analyze preserve saved/manual grid and loop intent.

The editor starts initial/reset and loop-focused views one beat before the loop
reference 1. Line 0 is the invisible left view edge; line 1 is one regular beat
to its right for a newly aligned track. Negative space is a display range, not
padded audio. Earlier source audio remains reachable by panning/Jump to start;
PCM reads, seeks and physical markers stay inside source bounds. Coordinates use
beat distance from current loop start, with fractional labels for off-grid loops;
moving the loop does not move the physical grid. Labels are bounded and thinned
at distant zoom. These numbers do not drive native SYNC. Performers can correct
noise, pickups and fades; the initial seed does not certify a musical downbeat.

| Layer | Owns | Must not own |
| --- | --- | --- |
| Render functions | Layout, widget composition, stable IDs, local visual gestures, input-to-action mapping. | Business rules, persistence, file I/O, audio engine calls, background jobs. |
| `UiContext` | Read-only selectors and explicit action facades used by render code. | Hidden behavior that bypasses controllers for core audio/project rules. |
| Controllers | Validation, model mutation, persistence dirty marks, background jobs, audio API calls, runtime event polling. | ImGui layout or widget-specific rendering decisions. |
| `ProjectState` | Durable performer intent: samples, loop regions, BPM/key metadata, dB Gain/Trim and EQ intent, stem cache metadata, settings, mappings. | Live audio truth. |
| `SessionState` | Recoverable runtime/UI projection: active pads, progress, meters, playheads, open overlays, edit buffers. | Durable behavior contracts or realtime audio authority. |
| Rust `AudioEngine` | Live audio truth: loaded buffers, transport, scheduler, voices, stems, playback-rate, Key Lock, DSP, metering telemetry. | UI rendering, JSON persistence, file dialogs, Python object ownership in the callback. |

Some UI-specific state is intentionally stored in `ProjectState` or
`SessionState`, for example selected pad, selected bank, sidebar expansion,
settings visibility, and text-edit buffers. Render functions still should not
mutate those fields directly. Use `ctx.ui.*` actions so persistence marks,
validation, and future behavior changes remain centralized.

## `UiContext` Contract

`UiContext` is the only object render modules should need.

### Read path

Use `ctx.state`:

- `ctx.state.project`: read-only proxy for durable project state.
- `ctx.state.session`: read-only proxy for transient session state.
- `ctx.state.pads`: derived pad selectors such as label, loading state, BPM,
  key, loop region, active/selected state, peak, and playhead projection.
- `ctx.state.stems`: stem availability, mix mode, masks, progress, and errors.
- `ctx.state.banks`: bank selection.
- `ctx.state.global_`: global derived values such as effective BPM.

`ReadOnlyStateProxy` intentionally raises on assignment. If render code needs a
mutation, that is a signal to add or use an action facade.

### Write path

Use explicit action groups:

- `ctx.audio.pads.*`: trigger/stop pads, load/unload, loop editing, analysis,
  BPM/key overrides, pad Gain/Trim in dB, and pad EQ.
- `ctx.audio.stems.*`: generate/delete stems, set stem mix mode, and set stem
  enabled mask.
- `ctx.audio.global_.*`: volume, speed, BPM display, Multi Loop, BPM Lock, Key
  Lock, and trigger quantization.
- `ctx.ui.*`: UI-only actions such as sidebars, file dialog, waveform editor,
  selected pad/bank, global BPM edit state, and pressed-pad projection.
- `ctx.ui.settings.*`: settings overlay actions.
- `ctx.input.*`: Learn mode and keyboard capture.

These actions route through controllers when behavior affects project intent,
audio state, persistence, input mappings, or background work.

Explicit performer Unload Audio and manual Analyze Audio use one transient
`PerformanceConfirmationController` intent through sidebar and mapped keyboard/MIDI
actions. `ctx.ui.confirmation` exposes it to an independent central modal renderer;
changing selection, hiding a sidebar or showing Settings does not retarget or hide
the captured warning. Learn captures the mapping without opening a warning.
The dialog identifies pad/source and explains the action before its accept/Cancel
controls. It is not persisted or an audio-engine transaction.

The controller captures content-instance ID, path and validated native source
generation/digest/frames/rate, rechecks the same assignment/current eligibility and
consumes acceptance once before the existing LoaderController action. Equal-byte
reload/rebind/rearrangement and absent/stale native identity cannot reuse old
authority. Cancel/Escape/dismiss own only that warning, with no loader, project,
playback or resource changes; they never cancel Residency/KEYLOCK. Mode/window
readiness on unchanged content is separate from assignment identity. Loading and
already analyzing content retain the manual-analysis eligibility policy. Automatic
load analysis, saved-result restore and internal lifecycle unload remain direct.
Native unload admission still precedes Python cleanup, so failed admission leaves
the editor and mode transaction intact; admitted unload closes only its edited pad.

## Immediate-Mode Rules

Immediate-mode is not "stateless UI". It means the application owns state and
the UI redraws from that state every frame.

Render code should be readable as:

```text
read state -> draw widget -> if interacted, emit action
```

Rules:

- Assume the draw function runs every frame.
- Keep per-frame work bounded and predictable.
- Derive widget IDs from stable domain identifiers such as pad ID, bank ID,
  stem kind, EQ band, setting key, or action key.
- Do not use shifting display labels as the only ImGui ID when a stable domain
  ID exists.
- Precompute expensive derived values in controllers or helpers.
- Keep validation and clamping in controller/model logic.
- Put meaningful behavior behind testable controller/helper APIs.

## What Belongs In The Core

Core/controller/model code owns:

- state transitions that affect project or audio behavior,
- validation and clamping,
- persistence dirty tracking and flushing,
- sample loading and unloading,
- waveform render-data generation,
- BPM/key/beat-grid analysis,
- loop-region semantics,
- keyboard/MIDI mapping state,
- stem generation and cache validation,
- Rust `AudioEngine` API calls,
- telemetry polling and projection into `SessionState`.

Core code must not call ImGui APIs or depend on widget frame timing for
correctness.

## What Belongs In The UI

UI/render code owns:

- windows, child regions, panels, overlays, and widget layout,
- visual grouping, colors, spacing, and label presentation,
- translating user gestures into `UiContext` actions,
- short-lived visual gesture state when it has no business meaning,
- opening/closing ImGui popups and file-dialog surfaces through `ctx.ui`.

UI code must not:

- perform file I/O,
- scan cache directories,
- decode audio,
- run Demucs,
- call blocking operations,
- own MIDI ports,
- mutate durable project state directly,
- call low-level Rust APIs for work that belongs in a controller.

## Slow Work And Background Tasks

Any interaction that can take noticeable time must be modeled as request,
progress, completion, and error state:

```text
UI gesture
-> UiContext action
-> controller validates request
-> background/audio worker starts
-> controller polls events
-> SessionState records progress/completion/error
-> UI redraws from SessionState
```

Current examples:

- sample loading through `LoaderController`,
- audio analysis through loader/analysis flow,
- offline Demucs stem generation through `StemController`,
- offline BS-RoFormer MUSDB18HQ through the same request/publication controller,
- Rust audio telemetry through `AppController.poll_runtime_events()`.

Render functions should never hide slow work behind a button click.

Settings exposes a persistent Stem Separator combo: Demucs htdemucs or
BS-RoFormer MUSDB18HQ. Choosing it is a small controller/persistence action;
the admitted job retains its selection and future jobs use the new choice.
Demucs quality sliders remain stored and appear when Demucs is selected,
including after a return from BS-RoFormer. Existing caches/playback remain
eligible under their current source/timing contracts. The combo performs no
model loading, download, cache scan or inference, and is independent of the
automatic beat-analyzer acceptance gate.

## Audio Boundary

The UI never touches realtime audio state directly. It sends intent to
controllers, and controllers call bounded `AudioEngine` methods.

```text
UI gesture
-> UiContext action
-> controller validation / state mutation
-> PyO3 AudioEngine method
-> bounded Rust command or parameter ring
-> CPAL audio callback
-> telemetry message
-> controller projection into SessionState
-> UI redraw
```

The CPAL callback must remain isolated from:

- ImGui objects,
- Python/GIL access,
- file I/O and JSON persistence,
- plugin loading/scanning,
- Demucs or neural inference,
- blocking locks or waits,
- logging/printing,
- heavy allocation or unbounded work.

For more detail, read [architecture.md](architecture.md).

## Waveform Editor Pattern

The waveform editor is the main UI surface where heavier derived display data is
needed. Its pattern is:

- render code owns layout and visible plot interaction,
- `ctx.ui.waveform` stores editor view state and short-lived cached render data
  keyed by pad, visible range, resolution, and current sample identity,
- `controller.transport.waveform.get_render_data(...)` supplies waveform render
  data,
- loop edits still go through transport loop actions,
- playback controls in the editor go through playback actions.

Adjust Loop occupies Pad, separator, Adjust Loop, separator, BPM in the left
sidebar and remains available during eligible analysis. Sidebar and mapped Adjust
use `WaveformController`'s shared open/retarget/close methods, which release the
previous view and advance its view revision. The UI projection observes that
revision before cache reuse. `CLOSE LOOP EDITOR` follows Grid Offset through the
same close authority; its full text and toolbar hit target wrap as needed at
narrow/scaled widths. Closing changes no playback, loop or grid state.

Waveform and Residency readiness rows reserve two fixed status/control
slots before ImPlot fills the remaining editor height. Idle/ready frames retain
the same slot height as pending/unavailable frames, including Retry/Cancel hit height.
Technical error details go to the command window; they do not occupy these rows.
The pre-fix full-editor wheel regression demonstrated plot Y and upper glyph
origins moving together when a late readiness change inserted/removed text above
the plot. Keeping the rows above the plot and reserving their height stabilizes
plot Y/height; existing source-coordinate labels and clipping remain authoritative.
The initial-frame matrix separately found a four-pixel plot-bottom shrink from
ImPlot's padded single-space playhead tag late tick-size override. This plot uses
zero vertical annotation padding, retaining horizontal padding and the existing
visible/invisible marker, to prevent the following frame's geometry from changing.
No clipping defect was established by either measured layout negative.
Appending rows below that full-height plot
creates a scrollbar/width feedback cycle: pending and ready frames alternate
between different resolutions and repeatedly supersede the asynchronous read.
Waveform resolution uses the actual ImPlot interior width after axis setup,
excluding axis labels and padding. A stationary view therefore retains its
cache key across readiness and playback updates. Genuine zoom, resize and source
replacement still request the matching view through the existing cache.

This restores the existing `waveform-editor` cached mono waveform and single-grid
contracts; it introduces no new timing, label, marker or playback semantics.
Binary64 source seconds and the existing loop-relative musical labels remain
unchanged (`preserve-scalar-source-precision` and
`auto-place-new-track-loop-start`).

The editor grid uses effective pad BPM (manual override before analysis) and
the persisted activity base (or legacy analysis onset when absent) rounded at
the loaded-buffer rate plus the persisted signed sample offset. Loop control publishes that same signed origin as precise native
timing metadata, including origins before source zero. Global playback modes and
other-pad actions do not move the source grid or persisted markers.
Native loop-region seconds remain `f64` through ordinary and direct-MIDI paths
until integer source-frame conversion, keeping long-position editor markers
sample-accurate. Native scalar BPM, period and rate parameters retain binary64
precision; rounded display values do not replace that authority.

Visible lines, musical snap and automatic endpoints share the pure native scalar
evaluator through a focused Python wrapper. Waveform X/query, seek and playhead
interfaces retain `f64`; ImPlot receives promoted visible Y arrays without narrowing
X. BPM edit buffers initialize from full precision and publish deliberate text
changes only. Rounded summaries expose effective precision in tooltips. See
[scalar source coordinates](scalar-source-coordinates.md) for units and limits.

BPM Lock is independent of MultiLoop and retains a source-qualified master
reference; editor selection alone does not replace it. Rust owns requested versus
effective application and the qualified one-time bootstrap when its reference is
active and valid. Repeated control publications do not reanchor an established
clock. Normal launches retain the future-grid/effective-loop-start policy. See
[runtime and transport](architecture/runtime-and-transport.md) for current clock,
rate, input ordering and acknowledgement ownership.

Do not move waveform decoding, sample scanning, or source-buffer access into
render functions.

## Input Mapping Pattern

Keyboard and MIDI Learn are action-based. UI code should map gestures to stable
`LooperAction` objects, not directly to ad hoc side effects.

Rules:

- Learnable actions should use existing action factories from
  `flitzis_looper.input_mapping`.
- Text-input focus must be respected before capturing keyboard shortcuts.
- MIDI/keyboard mapping behavior belongs in `InputMappingController`, not in
  render modules.
- `InputMappingController` publishes Learn state to Rust so MIDI Learn capture
  suppresses direct Rust dispatch before any mapped playback command is queued.
- Direct Rust MIDI events with `dispatched=True` must not be executed again in
  Python; direct events with `dispatched=False` use the same controller fallback
  path as non-direct MIDI actions.
- Capture launch time through `ctx.input.capture_timestamp_ns()` immediately
  after an accepted keyboard/mouse input condition. Pass `received_at_ns` through
  action facades/controllers before loop preparation; global restart batches
  reuse one captured value. Waveform restart uses the same timestamp path.
- Performance pads trigger on the left-button down edge (`is_mouse_clicked`),
  not a held-button/per-pad latch. Holding or dragging into a pad does not create
  another trigger, and releasing outside the pad cannot suppress the next click.
  The pressed projection clears when the rendered pad is no longer hovered.
  Right-button hold retains the existing per-frame stop behavior.
- Identical pending starts share the existing residency transaction and replace
  only its waiting launch with the latest original input timestamp. Unchanged
  resident PCM still needs native ACK and guarded launch, but no cold-worker or
  stem-file preparation; readiness belongs to the shared controller/native path.
- MIDI fallback preserves the event's original Rust timestamp. Never recapture
  a delayed event or mix Python monotonic time with the Rust engine epoch.
  Timestamp metadata stays transient and current launch behavior is unchanged.
- High-rate continuous parameters should be routed through controller/action
  paths that can choose safe Rust parameter-ring updates and smoothing.

## Testing Implications

ImGui is poor at deep black-box pixel testing. The architecture should make most
behavior testable without clicking pixels.

Preferred tests:

- controller tests for validation, state transitions, persistence marks, and
  audio API calls,
- helper tests for derived labels, display values, gesture math, and formatting,
- targeted render-helper tests where a calculation is pure enough to exercise
  without a live UI backend.

`test_waveform_render_stability.py` exercises the complete productive editor,
center child, toolbar, actual runtime regular/bold fonts and UI/controller cache
in both isolated ImGui/ImPlot context modes. A delayed worker projection is the
controlled boundary; real IO wheel/button events drive plot hover and toolbar
view changes before each native frame. Record every request/pending/ready/error/
settling plot rectangle, limits, cache/source query, draw-command clips/indices/
vertices and actual visible font-glyph geometry, including the first transition
frames. Cover raw/envelope crossover, initial/loop/full/tail views, replacement,
playback/Residency overlays, font/display scale 100/125/150/200 percent and ordinary/
narrow layouts. Compare meaningful scaled geometry instead of complete frames
while the waveform view changes.

The shared KEYLOCK renderer preserves effective button color separately from
pending/mixed status. Global status occupies a compact constant row with ordinary
details in a tooltip. Technical failures go to the command window. Keep actual
status and Residency controls in complete layout tests.

At most one waveform representation and one visible number band is permitted.
Require exactly one only for matching valid data and visible major/Loop-1 label
candidates respectively; pending/error data=None and empty candidate sets remain
truthful. A raw vertex outside a valid draw-command clip is not itself a visible
clipping defect. Actual warning/button/sidebar gestures and keyboard/MIDI/Learn
routes complement controller identity/admission tests. These draw/action checks
remain separate from final Human visual, listening and device acceptance.

If a behavior can only be verified by exact pixel interaction, too much logic is
probably in rendering code.

## Change Checklist

Before changing UI code, check:

- Does the render function only draw and emit explicit actions?
- Is the relevant state read through `ctx.state`?
- Does mutation go through `ctx.audio`, `ctx.ui`, or `ctx.input`?
- Are validation, clamping, persistence marks, and audio calls owned by
  controllers?
- Are widget IDs stable across labels, banks, and pad contents?
- Is per-frame work bounded?
- Is slow work represented by progress/completion/error state?
- Can the behavior be tested without pixel automation?
- Does the change preserve the audio callback boundary described above?

## Implemented Key + Transposition engineering surface

The sidebar's Piano toggle opens one central three-octave keyboard above pads
1–6. Its open state survives pad changes; chosen note, octave, offset and the two
independent Retrigger preferences belong to the selected content. Real white and
black keys retain their distinct octave identities. The original source key has
its own marker; a chosen note outside the displayed range receives an exact text
description instead of a folded highlight. Unknown source keys retain numerical
note and offset controls. NoteOff preserves the choice and highlight.

The compact Source button opens metadata correction with twelve Major and twelve
Minor choices. Use detected source removes only that correction. Both actions
preserve the numeric piano shift and octave, offset, Retrigger preferences and
playback; Original Key remains the separate reset of all pitch modifications. Raw unknown/legacy
labels remain readable, a manual marker identifies corrections and the tooltip
shows detected metadata. The popup retains its original content/path/Native source
identity and closes after a different selection or replacement. Metadata analysis
completion alone does not invalidate a conscious new correction. Existing edit
reservations and exhausted metadata epochs report their refusal in the command window.

Transposition's +1/Reset/-1 row supports the signed extra range -18..+18 with its
own Retrigger. Reset clears only that offset, preserving the piano note/octave and
source correction. Original Key clears the chosen note, offset and manual source
correction together. The piano's highlight stays on the chosen note when the
offset changes. Its nominal result does not claim to identify the sample's first
note; with Key Lock OFF, tempo still contributes to audible pitch.

The compact MIDI / octave steps foldout retains piano and offset ±1/±12 actions
and encoder mode selection. Its Fixed offset button displays the current signed
offset and uses the existing absolute extra-shift action with the offset lane's
own Retrigger preference. Set the desired offset before starting Learn, then
choose this button: Learn stores that exact target without playing or changing
the note/offset. Encoder mode does not turn it into a relative step. Reopened
bindings retain their fixed target and SET or SET_RETRIGGER variant even when
later GUI preferences, offsets or pad selection differ.
Fixed extra CC bindings keep their established per-value behavior: repeated
values and zero each apply the stored action. Piano/Original CC buttons retain
their separate high-edge behavior.

`UiContext` routes choices and resets through `PitchController` and the original
Native `CapturedPitchAction`, shared with MIDI's SET/SET_RETRIGGER semantics.
Each checkbox affects its own GUI lane and new Learn bindings; learned variants
remain fixed. SET preserves the cursor and stopped state. Each admitted SET_RETRIGGER,
including a repeated identical choice, owns a distinct attack whose tuple and
launch share the existing scheduler execution frame. Input time, content lifetime
and original dueframe survive preparation; delayed fallback never resolves a new
selection. The delivered stem-start coalescer's replacement semantics do not
apply to distinct admitted pitch attacks; preparation work may share.

Requested intent and Native effective feedback remain separate. Normal preparation
and confirmed audio state remain available without moving the Pitch fader during
a drag. Technical rejection reasons go through the Python control-thread console
sink, including operation, pad and available request identity. Unchanged polling
does not repeat a line; another deliberate failed attempt does. The sink performs
no audio-thread work. This is an implemented engineering surface; variable-rate
SET past/history reconstruction, performer deadlines, full B5 quality/transition/
latency, B6 and Human device/listening/visual acceptance remain open. It grants no
whole-arrangement or full-216 acceptance. See [Key Lock backend](../openspec/changes/archive/2026-10-30-add-key-transposition-performance/evidence/key-lock-backend.md#durable-key-intent-and-engineering-pitch-controls).

## Re-Arrange surface

Re-Arrange uses a warning-colored bottom-left toggle/help, OFF on restart. Its gesture
owner branches before pad trigger/stop; MIDI stays alive. LeftMove/Swap, rightCopy/
overwrite with clear preview/no single-pad dialog; cancel/self/invalid/empty no-op.
Other-bank pulse/leftcopyall36incl empties keeps source selected/no navigation;
occupied target exact OVERWRITE/CANCEL, right exact clicked CLEAR BANK/CANCEL
includingcurrent. Draghover/end cannot leak bank actions. Rendering holds snapshots,
not disk/native truth. Selection/editor follows living content; waveform hold release
is independent, delivered by existing input/control tick outside rendered controls.
Current musical stem selections become independent durable CopySnapshot choices;
physical presses/holds/voices/cursors/meters/progress/job tokens remain transient.
Bank warnings share the existing performer confirmation owner. They retain the exact
original BankRequest and opaque participants through acceptance, cancellation, Escape
and close; later selection cannot redirect the action. Bank widgets are disabled
while a PAD gesture owns the pointer, including staggered two-button releases.
Ordinary MIDI refresh preserves the last successful reserved rows and Native's exact
committed rows until actual settlement, while outside rows remain publishable. An
explicit Multi Loop change retains its normal semantic supersession. Unsolicited
Native rejection reports through the existing deduplicated console diagnostics.
Bounded UI/controller/Native proofs do not establish full-216, resource, quality,
musical, device or Human acceptance.

## All-loaded stem generation

Settings provides **Generate Stems of all loaded PADs** through the same audio
confirmation owner used by performer and Bank actions. The warning retains the
original loaded PADs across all six banks (216 slots), the requested separator /
model and quality. Changed or unavailable originals are skipped; newly loaded
contents are excluded. Cancel, Escape and close dismiss the warning without
starting work. One confirmed operation continues outside Settings and keeps the
current PAD and Bank selected.

The bounded scrolling list shows original PAD numbers and exact Queued, Running,
Done, Skipped, Failed, Cancelling and Cancelled counts. Done means stems saved;
playback readiness is reported separately for each PAD. **Cancel batch** withdraws
only this operation's interests, preserves completed items and waits for its own
actual work to settle. Other callers sharing the work continue. Partial results
remain visible until another batch starts or **Dismiss batch results** closes the
terminal result. The surface owns no jobs, audio timing or persistence.

Bounded controller, Native-crossing and actual desktop checks cover this surface.
Full216 resource/process/adoption and Human musical/device/hearing acceptance
remain separate.

## All-loaded stem deletion

Settings provides **Delete Stems of all loaded PADs** through the same central
confirmation owner. Its immutable inventory retains all 216 original slot
positions across six banks, exact loaded assignments, current and known retained
set versions, admitted request identities and existing unadmitted caller tokens.
The warning reports occupied original PADs and unique exact set versions; shared
versions count once. Opening it acquires no source, reader or cleanup permission.
Cancel, Escape and close dismiss only that warning. Changed originals are
Skipped and newly loaded content is excluded.

One accepted `StemDeleteController` operation withdraws only still-unadmitted
captured batch tokens through their existing owner. Its returned withdrawal
receipt distinguishes a pending-only Removed result from an already absent
Noop. Admitted work still uses the existing guarded per-PAD deletion path and
safe FullMix handover. `AppController` offers at most eight original items per
desktop frame independently of Settings, deferring arrangement-fenced items
before reading their intermediate image. Shutdown preparation also advances the
logical operation before dependent owners are torn down. A closing frame reuses
its normal render poll; if no GUI frame ran, shutdown preparation performs the
bounded poll, so minimized frames still progress. Selection does not change.

`ctx.ui.stem_delete` exposes detached logical feedback and deliberate result
dismissal. The six-line scrolling list retains original PAD numbers and exact
Removed, Noop, Skipped, Failed and Queued states. Removed means withdrawal of
stem selection or pending work; it confirms neither effective audio nor physical
file removal. Partial results remain until **Dismiss deletion results** or a new
confirmed deletion replaces them. Generate and Delete are disabled while the
deletion operation is active.

The separate **Global file cleanup** row observes the existing Native cleanup
status and `ProjectAssetLifecycle` retirement errors. Its pending/error counts
explicitly cover global cleanup, including other operations. It does not
attribute Native's cumulative deletion count to this action. A failed status
query shows Pending unknown and its error. Global cleanup feedback remains
visible after logical results are dismissed; actual readers retain files until
their final release. Rendering performs no physical deletion or operation poll.
Complete B2 reader/resource and Human all-loaded UI/device/hearing acceptance
remain open.

## Direct extension2026-10-11 and remaining acceptance

The [E11 UI/control plan](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/pre-rust-extension-20261011.md) retains final Human
visual repetition of zoom/layout/confirmation usability after the bounded X11-UI
software gates. The measured readiness-layout fault does not establish the exact
Human perceptual duplicate or reported binary. Whole P4b/P5a and later resource,
music/history, device/hearing and V0/H-FINAL gates remain as recorded in historical
evidence. Old Goal/RECORD delivery programs are inactive; current recording capture
and settlement are described in [state/input](architecture/state-input-and-ui.md)
and [runtime](architecture/runtime-and-transport.md), without promoting those gates.
