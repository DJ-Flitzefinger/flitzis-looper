# State, input and UI

Python owns validated performer intent and application coordination; Rust owns
current audio effects and their identity proof. See the [overview](../architecture.md),
[runtime](runtime-and-transport.md) and [media](media-and-pcm.md) for the other
boundaries.

## Entry points and state owners

[__main__.py](../../src/flitzis_looper/__main__.py) calls
[run_ui](../../src/flitzis_looper/ui/run.py). The runner creates an
[AppController](../../src/flitzis_looper/controller/app.py), constructs the
[UiContext](../../src/flitzis_looper/ui/context.py) action facade and renders
through [ui/render](../../src/flitzis_looper/ui/render/). Widgets present state
and call actions; they do not write audio buffers or become transport time.

[models.py](../../src/flitzis_looper/models.py) separates:

| State | Meaning |
| --- | --- |
| `ProjectState` | Durable assignments/content, loop/BPM/Key/pitch/gain/EQ/stem intent, global settings, selected PAD/bank and input-mapping enablement. Validation protects fixed 216-slot tables and typed values. |
| `SessionState` | Recreated UI/runtime projections: selection admission/revisions, active/paused observations, pending work, progress/errors, meters/playheads, dialogs and edit buffers. |
| Native current owners | Source generation, content lifetime/placement, accepted timing, prepared readers, voices, scheduler and operation-specific outcomes. Copying saved fields cannot recreate these. |

Fresh defaults include quantization step 1/16 initially disabled, FLAC24 recording
with compression 8 and BS-RoFormer MUSDB18HQ. Saved valid settings remain intent.
Global Key Lock is inherited only where a PAD's nullable override is `None`;
an explicit choice survives same-value global edits. Returning to GLOBAL clears
that override. [key_intent.py](../../src/flitzis_looper/key_intent.py) separates
source Key/correction from requested transposition and retrigger.

Controllers resolve current bindings before live publication. UI selection is
display intent, not playback master or the MultiLoop OFF effective target.
[pad_selection.py](../../src/flitzis_looper/controller/pad_selection.py) records
admitted selection with a Session revision and displayed content, including a
repeated slot choice. Pair/Bank projections preserve captured intent across
placement changes.

## Input intent and native dispatch

[actions.py](../../src/flitzis_looper/input_mapping/actions.py) defines serializable
semantic actions; [bindings.py](../../src/flitzis_looper/input_mapping/bindings.py)
defines bindings; [controller.py](../../src/flitzis_looper/input_mapping/controller.py)
owns Learn, keyboard handling, snapshot publication and controller actions.
Gesture/render conventions remain in [ui-toolkit.md](../ui-toolkit.md).

[storage.py](../../src/flitzis_looper/input_mapping/storage.py) persists the
actual bindings separately in `config/input/keyboard.json` and
`config/input/midi.json`; these files are not the ProjectState mapping table.

Native [input_mapping.rs](../../rust/crates/looper/src/audio_engine/input_mapping.rs)
captures MIDI outside the audio callback. Direct PAD/pitch actions use published
source-bound Native state; controller-owned actions return as small events.
Learn takes precedence over mapped playback: input is captured for learning
instead of directly triggering the mapping.

GUI/keyboard capture timestamps from the same native monotonic epoch as MIDI.
Original input survives preparation and direct-dispatch fallback. Rust direct
dispatch is all-or-none: undispatched events are reported as such, and Python
retries through the guarded current API rather than an unqualified launch.
Successfully dispatched effects are not replayed by Python. Captured pitch
variants retain SET versus SET_RETRIGGER and their original target.

[current_binding.py](../../src/flitzis_looper/controller/current_binding.py) and
[current_timing.py](../../src/flitzis_looper/controller/current_timing.py) resolve
source/authority and exact accepted timing. Input runtime snapshots refresh on
relevant source, loop, timing or hierarchy changes, not every unchanged frame.
Saved automatic analysis without current own acceptance cannot stand in for
runtime proof.

## Polling, feedback and errors

`AppController.poll_runtime_events` polls recording, holds, arrangements, loaders,
audio feedback, timing publication, residency, pitch and global-rate/BPM Lock
outcomes. The UI runner keeps runtime polling reachable during dialogs/settings
and close handling. Observer fences protect Copy/Pair/Bank participants so
intermediate placement cannot update other controllers as final state.

Background loads/analysis retain request IDs, content instance and source
identity. Stale progress/success/failure is ignored for replaced owners; Native
also checks before source publication. Timing and Key edits have distinct epochs,
so timing-stale results cannot erase a newer Key correction.
[accepted_publication.py](../../src/flitzis_looper/controller/accepted_publication.py)
separates accepted timing adoption from its derived loop/master refresh ACK.
Polling completion is observation, not audio scheduling.

Peaks/playheads and generic lifecycle messages are best-effort. Ordinary Pause
also has current content/source-bound observations; tickets settle their own work.
[control_diagnostics.py](../../src/flitzis_looper/control_diagnostics.py) reports
control errors outside realtime. Requested/preparing/applied/rejected states
remain visible where exposed; a queue refusal must not look like successful
playback or saved selection.

## Persistence and restore

[ProjectPersistence](../../src/flitzis_looper/controller/persistence.py) owns config
writer, dirty revision and save error. It verifies current accepted timing against
Native before serialization, checks the captured revision, writes a flushed/
fsynced temporary file beside config and replaces atomically. A newer edit stays
dirty. Timing rejection, writer fences and ordinary I/O failure preserve prior
config and expose failure; serialization is not audio ACK. Atomic replacement
is not a blanket power-loss guarantee.

The App constructor follows these dependency constraints:

1. Load/validate `ProjectState`, create fresh `SessionState`, configure resident
   admission and Key Lock start behavior, then start `AudioEngine`.
2. Construct asset/controllers and inspect bounded arrangement/recovery journal
   histories. Retain custody and fence participants before ordinary restore or
   cleanup.
3. Restore usable assignments through the loader. Recovery-owned missing
   assignments remain visible/fenced; ordinary invalid assignments are cleared
   with corresponding neutral Native state.
4. Verify saved stem selections after matching source load; obtain fresh
   registration and, where requested, component/mask or preload acknowledgement.
   FullMix with preload disabled does not imply component residency.
5. Publish global and loaded-PAD intent plus input runtime state against current
   sources. Empty PADs do not receive speculative loaded-state settings; explicit
   unload/reset still clears actual stale Native state.

[accepted_restore.py](../../src/flitzis_looper/controller/accepted_restore.py) and
the loader obtain fresh source-verified timing adoption. Old accepted JSON,
historical journal tokens and an empty selection epoch cannot create current
ownership or Native acceptance. Existing field-recovery paths preserve valid
intent without restoring runtime tickets; they do not authorize a new migration/
compatibility program.

## Copy, Pair, Bank and shared custody

[copy_snapshot.py](../../src/flitzis_looper/copy_snapshot.py),
[pair_snapshot.py](../../src/flitzis_looper/pair_snapshot.py) and
[bank_snapshot.py](../../src/flitzis_looper/bank_snapshot.py) capture immutable
musical intent. Mutable content/voice state and fresh acceptance remain independent
even when material is shared.

[pad_arrangement.py](../../src/flitzis_looper/controller/pad_arrangement.py),
[pair_arrangement.py](../../src/flitzis_looper/controller/pair_arrangement.py) and
[bank_arrangement.py](../../src/flitzis_looper/controller/bank_arrangement.py)
coordinate journals, config revisions, Native mutation and observer settlement.
Occupied Copy retains source A, predecessor B and one fixed successor C through
preparation, actual own ACK and terminal journal append. Config receipts refer
to actual written revisions, preserving newer unrelated edits. Bank is one
guarded 36-slot operation, not 36 unrelated optimistic copies.

Startup recovery checks complete bounded histories, original custody and successor
lineage. A recognized written successor receives fresh target-only acceptance;
it does not copy a potentially changed source again. Ambiguous/unknown histories,
missing custody and conflicting unfinished owners keep visible fences. Readers
survive interrupted config/Native settlement. See [media identity](media-and-pcm.md#identity-and-custody)
and the [Copy report](../pad-content-copy.md) for software cases and open gates.

## Recording and shutdown

[recording.py](../../src/flitzis_looper/controller/recording.py) coordinates bounded
Native mix capture and background spool/finalization/codec work. Direct mix
recording starts capture without a global-start group. Group-coupled recording
retains the original global-start owner through preparation; an unavailable/stale
group cannot be replaced by a newly captured group to finish preparation.
[recording_recovery.py](../../src/flitzis_looper/recording_recovery.py) retains the
separate recoverable spool/finalization lifecycle. Callback capture performs no
file I/O or encoding.

Close calls `prepare_shutdown` while callback/controllers are alive: recording
settles under its actual deadline, holds release, input dispatch is disabled,
batch generation is cancelled and deletion/Copy/Pair/Bank settle. UI polling
continues. Focus loss/hidden editor requests matching Hold release; release/
focus loss/close clears momentary output mute. File-picker cancellation settles
separately. Only then does `shut_down` release dependent workers, attempt config
flush, stop audio, release saved assignments and close the engine. A failed save
or uncertain stream disposal does not prove durable settlement or last-reader
release. Runner exceptions cannot bypass the arrangement gate.

## Verification boundaries

Relevant regressions cover selection versus transport target, Learn/direct
fallback, stale loader/analysis feedback, nullable Key Lock inheritance, ordinary
pause plus Hold, observer fences, revision-safe saves, recovery-owned missing
media and close settlement. Current [performance UI](../../openspec/specs/performance-ui/spec.md)
and [background tasks](../../openspec/specs/background-tasks/spec.md) specify
contracts. Historical reports retain wider resource/device/Human gates. Earlier
OPEN boxes do not prove missing code; software passes do not constitute
whole-216, device, musical or hearing acceptance.
