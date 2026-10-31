# Flitzis Looper user guide

Flitzis Looper is a loop performance instrument for DJs and turntablists. Load
your own music, choose loop regions, build banks, and play from the mouse,
keyboard or MIDI. You can combine prepared stems, change tempo and pitch, and
record the output mix.

This guide describes the current constant-BPM application. Start with
[installation](../README.md#installation) or the detailed
[Windows Portable guide](portable-windows.md). Contributors should use the
[development guide](development.md); technical ownership is explained in the
[architecture overview](architecture.md).

## Contents

- [First session](#first-session)
- [Screen and fresh defaults](#screen-and-fresh-defaults)
- [Audio output and startup](#audio-output-and-startup)
- [Banks, loading and PAD gestures](#banks-loading-and-pad-gestures)
- [Transport, MultiLoop and quantization](#transport-multiloop-and-quantization)
- [Tempo and BPM Lock](#tempo-and-bpm-lock)
- [BPM analysis and correction](#bpm-analysis-and-correction)
- [Waveform and loop editing](#waveform-and-loop-editing)
- [Source Key, Piano and transposition](#source-key-piano-and-transposition)
- [Key Lock](#key-lock)
- [Gain, isolator and master](#gain-isolator-and-master)
- [Stems](#stems)
- [Keyboard and MIDI Learn](#keyboard-and-midi-learn)
- [Recording and output files](#recording-and-output-files)
- [Saving, recovery and closing](#saving-recovery-and-closing)
- [Memory and performance](#memory-and-performance)
- [Troubleshooting](#troubleshooting)

## First session

1. Choose your output device in the operating system. Start the installed app;
   for a prepared Windows source checkout, use `start.bat`. A Portable install
   has its own `START-LOOPER.bat`.
2. Middle-click an empty PAD to select it. In the left sidebar, choose **Load
   Audio**, then a file. Wait for loading; BPM and Key analysis have their own
   progress and can finish later.
3. Left-click the loaded PAD to play/restart it. Right-click it to stop. A middle
   click selects a PAD for editing without changing playback.
4. Select **Adjust Loop**. Set or correct BPM, then choose a loop start and bar
   length. Start quietly, check the boundary by listening and close the editor.
5. Load a second PAD. Enable **MULTI LOOP** to layer PADs. For tempo matching,
   select the intended reference PAD and enable **BPM LOCK**. Verify the
   confirmed **Audio** status.
6. Enable **Q** if you want quantized launches. The fresh step is 1/16; Settings
   offers the other supported steps.
7. Adjust **Gain / Trim** on each PAD and **Master Volume** for the combined mix.
   Watch the clip indicators when layering material.
8. To capture the running mix without restarting it, right-click **RECORD**.
   Click RECORD again to stop capture, then wait for the saved-file status.

Preparation, queuing and recording finalization take time. Read the status text
before repeating an action. A selected PAD, an enabled-looking button or a saved
choice does not by itself confirm what the audio engine is playing.

## Screen and fresh defaults

The center contains a 6 × 6 PAD grid. Six banks provide **216 assignment slots**;
they are storage and performance positions, not a simultaneous-playback target.
The left sidebar edits the selected PAD. The right sidebar holds global tempo,
Key Lock, MultiLoop and BPM Lock. The bottom bar holds recording, START/STOP,
Learn, quantization, stem choices, master volume and Settings. Piano and the loop
editor appear when opened.

Fresh projects use these values. Valid saved settings are restored instead.

| Setting | Fresh value |
| --- | --- |
| MultiLoop / BPM Lock / global Key Lock | OFF |
| Trigger Quantize | OFF, retained step 1/16 |
| Speed / Master Volume | 1.00× / 100% |
| PAD Gain / Low / Mid / High | 0 dB |
| New imported track's Auto-loop | ON, 8 bars; half-bar minimum |
| Input Mapping | Enabled |
| Recording | FLAC, 24-bit, compression 8 |
| Stem separator | BS-RoFormer MUSDB18HQ |
| Preload existing stem loops at startup | OFF |
| Resident audio memory admission | Automatic |
| Automatic BPM range | Disabled; retained range 50–170 BPM |
| Key Lock start behavior | First sound at grid |

Three kinds of feedback matter throughout the app:

| Feedback | How to interpret it |
| --- | --- |
| Selected / requested / saved | The editor or project has that choice. |
| Queued / preparing / pending / waiting | Work or an audio change has not yet been confirmed. |
| Audio / applied / effective | The current audio operation has supplied its matching confirmation. |
| Refused / rejected / unconfirmed / error | The requested outcome is not established; read the reason and retry after resolving it. |

Meters and playheads are useful observations. They are not substitutes for an
operation's error or pending status. Some changes require prepared source audio
and can be deferred by resource pressure.

## Audio output and startup

The app opens the operating system's **default output device** and its default
channel/sample-rate configuration at startup. There is no in-app audio-device
selector. Select the desired interface in the operating system before launching;
restart the app after changing the intended output. MIDI and model processing
device selection are separate from audio output.

For a Windows source installation:

- `start.bat` launches an already prepared Release installation. It does not
  install dependencies or compile Native code.
- `start.bat --check` checks the existing Release Native/profile interface
  without opening the app. It does not verify every analysis worker or model.
- `start-release.bat --build-only` builds and checks Release without opening the
  app. Close Looper windows before a rebuild or update.

Portable installations use their own launcher and private runtime. Run the
installer again with the app closed to update or repair them. Keep the completed
installation together when relocating it. See [Portable setup](portable-windows.md).

Source setup prepares automatic BPM resources separately with
`scripts/setup-runtime.py`; optional stem resources use `--with-stems`. Use the
[source setup instructions](development.md#setup),
[BeatThis setup](beat-this-setup.md) and [stem setup](stem-generation-setup.md)
for the actual commands and required tools. Startup checks are resource checks;
they do not mean a model has analyzed a song or that the audio device has passed
a listening test.

The accompanying terminal shows timestamped technical errors. Keep it visible
when diagnosing startup, loading, worker or recording failures. The UI retains
ordinary progress, state and retry controls.

## Banks, loading and PAD gestures

### Select and load

In normal mode, click a bank button to show its 36 PADs. Bank selection changes
the visible grid; it does not stop playback in another bank. PAD numbers identify
slots across all banks.

Select a PAD with the middle mouse button, then use **Load Audio** in the left
sidebar. The system file picker returns one file to that PAD. Cancelling it does
not import anything. Loading is asynchronous; the sidebar and PAD show its stage,
progress or error. A failed or superseded request must not be treated as loaded.
The picker offers WAV, AIFF/AIF, FLAC, MP3 and OGG; decoding can still fail for an
unsupported or damaged file.

A new track initializes Auto-loop ON at eight bars. Its initial loop/grid can
use a detected source-activity boundary, falling back to the source beginning
when none is available. This is an initial proposal to check by listening, not
proof of a musical downbeat. Restored valid loop settings keep their saved intent.

Imported audio is managed as project material; PAD settings and playback state
remain independent even when PADs share the same underlying material. Keep the
project's sample/material directory with its configuration when making backups.
See [saving and recovery](#saving-recovery-and-closing).

### Normal PAD gestures

| Gesture | Result |
| --- | --- |
| Left-click a loaded PAD | Select and trigger/retrigger it. |
| Middle-click a PAD | Select it without starting or stopping audio. |
| Right-click a PAD | Select and stop it; holding right continues stop requests. |
| Left-click an empty PAD | Select it; use Load Audio to assign a file. |

Selection tells the sidebars and editor which PAD to display. It does not make
that PAD the transport target or a new tempo master. This distinction is useful
when editing one PAD while another plays.

For a loaded PAD, the sidebar offers **Unload Audio**, **Analyze audio**,
**Adjust Loop**, stem controls and Key Lock. Unload removes the assignment;
physical shared material can remain while other assignments, work or readers
still need it. Do not manually delete project containers to accelerate cleanup.

### Re-Arrange

Enable **Re-Arrange** to edit the layout. Its gestures take over the normal PAD
mouse actions:

| Gesture in Re-Arrange | Result |
| --- | --- |
| Left-drag PAD to empty PAD | Move content. |
| Left-drag PAD to occupied PAD | Swap content. |
| Right-drag PAD to another PAD | Copy; an occupied target is an overwrite operation. |
| Left-click another bank button | Copy the current bank into that bank, subject to confirmation. |
| Right-click a bank button | Request clearing that bank, subject to confirmation. |
| Release outside a target, or press Esc | Cancel the owned drag. |

Turn Re-Arrange off to select banks normally. MIDI remains active while
Re-Arrange is enabled. Read the confirmation before overwriting or clearing
content. A pending arrangement can temporarily fence participating PADs; wait
for settlement rather than retrying on a newly occupied slot.

Copies share immutable material while keeping independent musical settings and
runtime voices. Copying a PAD is not a promise that prepared audio, active
playback or source acceptance can be duplicated instantaneously. Interrupted
operations may need guarded startup recovery.
Bank copies include empty source slots and create stopped copies; read the target
overwrite confirmation carefully. The source bank remains selected.

## Transport, MultiLoop and quantization

### START/STOP and output mute

The combined **START/STOP** control has distinct gestures:

| Gesture | Action |
| --- | --- |
| Left press | Start/restart its current transport target or group. |
| Right-click | Stop that target or group. |
| Hold middle mouse button / wheel | Temporarily mute output; release restores output. |

Temporary output mute does not pause the playheads. It restores the latest
master-volume setting on release. Focus loss and closing also release this
momentary mute. RECORD has separate capture semantics described
[below](#recording-and-output-files).

### MultiLoop OFF

OFF gives the one-at-a-time performance behavior. Global START/STOP follows the
**last PAD content that actually became effective through playback**, including
its current position if rearranged. Merely selecting another PAD for editing
does not redirect it. STOP retains that target for a later START.

The loop editor has an intentional exception: its Play/restart and loop-start
playback preserve other PADs, even with MultiLoop OFF. They let you audition/edit
the selected source without stopping peers. Ordinary grid triggering follows the
MultiLoop mode.

If no valid effective target exists, an explicit START can use a loaded selected
PAD as fallback. STOP does not create such a fallback. An empty selection does
not invent playable content.

### MultiLoop ON and switching it off

ON permits concurrent PADs. Global START/STOP acts on its captured group rather
than successively issuing unrelated PAD operations. A deferred start keeps the
original group while preparation completes.

Switching ON → OFF stops peers together and preserves the last effective target.
It does not restart that target, resume a paused/stopped target or replace the
established master tempo. A pending transition is still pending; a refused
transition leaves the previous mode and playback intact. Watch the confirmed
audio state, particularly during preparation or resource pressure.

### Quantization

**Q** toggles Trigger Quantize. Choose **1/64**, **1/32** or **1/16** in Settings.
The fresh retained step is 1/16, with Q initially OFF. With Q enabled, supported
launch/retrigger actions use the future grid derived from the master tempo.
Preparation can delay or refuse an action; it does not silently become a newly
pressed trigger.

Quantization chooses a launch boundary. It does not analyze the musical downbeat
or continuously correct drifting recordings. Set BPM and source Grid Offset
correctly, and check the loop by listening. Tempo, source phase and rhythmic
half/double choice are separate decisions.

### Pause and temporary Hold

In **Adjust Loop**, the Pause icon's left press toggles ordinary pause/resume.
Pause retains the current position; press it again to resume that source.
The Play icon is explicitly play/restart, so use it when you want a
fresh entry rather than treating it as a generic resume toggle.

Right-press the Pause icon for a **temporary Hold**. Keep it held for the
momentary action, then release. Leaving focus, hiding/closing the editor or
closing the app settles that captured Hold. It belongs to the original source;
replacing content does not authorize resuming a new occupant. A stopped or
end-of-file voice is different from a paused voice.

## Tempo and BPM Lock

The right sidebar shows global BPM, a speed fader, **+**, **Reset**, **−**,
**KEY LOCK**, **MULTI LOOP** and **BPM LOCK**. Speed is bounded to **0.5×–2.0×**.
Where a tempo reference is available, the controls operate in BPM terms; without
one they use speed. Double-click the large BPM display to edit its full-precision
value when available. The large display is rounded to two decimal places;
rounding the display does not round the stored timing.

Hover the BPM display for requested speed/BPM, confirmed audio values and
queued/preparing/applied feedback. A requested speed is not necessarily the
currently effective speed while audio preparation is outstanding. Reset returns
the speed control to its reference rate.

To tempo-match material:

1. Load and check the reference PAD's BPM.
2. Select that intended reference and enable BPM Lock.
3. Check the sidebar's **Audio ON/OFF**, pending or unconfirmed text.
4. Trigger additional PADs, using Q if desired.
5. Change the master tempo with the right-side controls.

BPM Lock works with MultiLoop either OFF or ON. It retains the established
master reference as PADs are selected, triggered or rearranged; editor selection
does not keep replacing the master. PADs with valid source tempo use the
source/master tempo ratio. Missing source tempo uses the supported speed path,
so entering a reliable BPM matters when mixing unrelated sources.

This is **constant-rate BPM Lock**. It does not follow changing recorded tempo,
repair a wrong downbeat or guarantee two inaccurately labeled loops remain in
phase. Key Lock controls pitch preservation separately. More detail is in the
[constant-BPM guide](constant-bpm-guide.md) and
[transport architecture](architecture/runtime-and-transport.md).

## BPM analysis and correction

Loading starts automatic analysis. **Analyze audio** runs it again for a loaded
PAD. BPM and Key have independent outcomes: BPM can succeed while Key is skipped
or fails, and a Key result does not certify BPM.
Read the reanalysis confirmation: admitted new detection can replace detected
BPM/Key/grid information, while manual BPM, Grid Offset and loop choices retain
their separate intent. Cancel and retry a stale confirmation instead of applying
it to changed content. Unload also has a confirmation and clears that PAD's audio,
loop and analysis settings after stopping it.

The productive BPM path uses the configured BeatThis worker with corroboration
and constant-tempo checks. A complete source remains available to analysis even
when playback currently holds only a loop-sized resident view. A short visible
loop is not a request to classify only that excerpt.

### Read the result

The sidebar shows BPM, processing stage, available details and the last confirmed
inference device. **CPU (CUDA fallback)** means a CUDA execution failure was
followed by a successful fresh CPU attempt. **Device unknown** means no confirmed
device receipt is available. Choosing CUDA or installing a CUDA package does not
prove the source was processed on the GPU.

If precise automatic BPM fails, a simpler source-derived fallback can supply
**Approximate BPM**. Details warn that it **may be inaccurate**. Check /2, x2
or Tap BPM and adjust Grid Offset separately: a coarse tempo does not identify
beat/downbeat phase. Save/load and PAD copy retain this warning. Existing precise
values and performer overrides take precedence; unusable or cancelled analysis
cannot supply an invented default BPM.

Some difficult material can yield **Possible BPM drift** and **Mean estimate ·
middle 60%**. This is an approximate scalar from supported interior sections.
It is useful for ordinary constant loops but is not live tempo following or proof
that the recording itself varies. Unsupported, insufficient, incomplete or
budget-refused analysis can leave BPM unavailable instead of manufacturing a
confident value.

An automatic estimate is a starting point. Wrong rhythmic level, sparse beats,
introductions, irregular music and detector errors can all matter. Decimal
precision is not musical accuracy. Listen across repeated loop boundaries and
compare against a reliable reference when accuracy matters.

### Manual BPM, half/double and Tap

- Edit the PAD BPM field to enter a manual value. The UI accepts **0.5–400 BPM**
  and retains fractional precision. Use this for a known tempo rather than
  repeatedly reanalyzing the same difficult source.
- **Clear Manual BPM** removes that override to use the available automatic
  result again; it does not initiate a new analysis.
- **BPM /2** and **BPM x2** change the pulse level of the current value. Choose
  the interpretation matching the music before adjusting Grid Offset or bars.
- Use **Tap BPM** repeatedly at a steady beat. The fit uses the current tap
  sequence and can reject misplaced taps. A gap longer than three seconds or a
  source change begins a new sequence. Systematic tapping drift remains possible.

Manual/Tap choices retain their priority over new automatic results. Reanalysis
must not replace a supported precise result for the same unchanged source with
an approximate mean. Independently completed Key work can still update its own
metadata. BPM edits and manual source-Key corrections are separate operations.

### Automatic BPM range

Settings provides **Use automatic BPM range**, **Minimum BPM** and **Maximum
BPM**. Valid bounds are 30–300 BPM with minimum below maximum. This optional
policy applies to **future loads and Analyze audio**. It does not reinterpret
existing BPM, retroactively qualify an older result or restrict manual/Tap input.
If no supported candidate fits, analysis can remain unavailable.

See [BPM architecture](architecture/bpm-analysis.md) for policy and confidence
limits, and [BeatThis setup](beat-this-setup.md) for worker/device repair.

## Waveform and loop editing

Select a loaded PAD and choose **Adjust Loop**. Waveform display and playback
context have separate preparation states. **Preparing source waveform…** can
remain visible while other work is ready; its error has **Retry**. **Preparing
playback context…** refers to audio needed for playback/editing. Cancel is offered
where preparation is cancellable; an already adopting operation must settle.

### Editing gestures

| Control or gesture | Result |
| --- | --- |
| Play icon, left press | Play/restart selected PAD, preserving other PADs. |
| Play icon, right press | Stop selected PAD. |
| Pause icon, left press | Toggle ordinary pause/resume. |
| Pause icon, right hold | Temporary Hold; release settles it. |
| Left-click release in waveform | Set loop start and play, preserving other PADs. |
| Right-click release in waveform | Set loop end when Auto-loop is OFF. |
| Middle press in waveform | Seek to the source position. |
| ALL | Request the full-track loop region. |
| Reset Zoom / Zoom to Loop | Fit the full available view / loop region. |
| Jump to start / Jump to end | Move the view; these are view navigation controls. |
| CLOSE LOOP EDITOR | Close the editor and settle temporary editor actions. |

A plot drag is distinguished from a marker click. Use the plot's zoom/navigation
and the explicit fit/jump controls to inspect distant regions; a view jump does
not itself change a loop marker. Marker positions and the playhead remain in
full-source coordinates even when playback uses a finite resident window.

### Auto-loop and bars

With **Auto-loop** enabled, the end follows the start, BPM and chosen bar length.
A bar is four beats. The bars arrows use powers-of-two steps with the left mouse
button and half-bar steps with the right. The minimum is half a bar; the available
source limits the maximum. Without BPM, the editor indicates that bars are
unavailable. Turn Auto-loop off to set an explicit end with a right-click.

**ALL** requests a whole-track region. A long region may require more resident
audio and can wait for memory allowance or fail admission. A complete source on
disk does not guarantee that every large playback context fits in RAM.

### Grid Offset and musical phase

**Grid Offset** shifts the source grid independently of choosing the loop entry.
Drag horizontally: left uses one-sample steps, right uses ten-sample steps;
middle-click resets the offset to zero. Set the grid to a meaningful beat/downbeat
and then choose loop start and length. BPM controls spacing; Grid Offset controls
phase. Changing one is not a correction for a mistake in the other.

The editor shows physical markers on sample positions while the musical period
can be fractional. The runtime preserves that timing precision. It does not
guarantee a seamless musical cut from an arbitrary marker, nor correct a changing
source tempo. Check the audible boundary and the desired musical phrase.

There is no separate Normal/Intro/Tail/OneShot selector in this editor. Use the
exposed loop controls; do not infer an additional performer mode from internal
engine terminology. Reaching a real source end is not an ordinary pause, and
unavailable source context can refuse a seek/start rather than play missing audio.

## Source Key, Piano and transposition

The selected PAD's **Key + Transposition** area separates source metadata from
the pitch requested for playback.

### Source metadata

**Source** opens the major/minor source-key choices. Use it to correct a wrong
automatic label or label an unknown source. **Use detected source** removes the
manual correction. Correcting this metadata does not itself change pitch or
restart playback.

Automatic Key classification uses the complete source. It can be unavailable,
failed or skipped by admission; it is not a guarantee of the song's musical key.
Reanalysis that fails/skips Key retains prior detection/correction. A matching
successful new Key result can replace the old detection and clear its preexisting
correction. Check the Source label again after reanalysis.

### Piano and offset

Open **Piano** to show the keyboard above the PAD grid. Its notes refer to the
source-key root, not a sampled keyboard instrument. Choosing a note changes the
PAD's numeric base shift. The highlighted note, original root and offset describe
the nominal choice; if the source root is unknown, supply accurate Source metadata
for meaningful musical labels.

**Transposition** adds an independent signed semitone offset. Use **+1**, **−1**
or **Reset**. The expanded **MIDI / octave steps** area offers Piano and Offset
steps of −12, −1, +1 and +12, plus a fixed-offset target. Extra offset is bounded
to −18…+18 semitones; the Piano lane has its own supported keyboard range.

Piano has 36 fixed keys across three octaves. Base shift is limited to −23…+23
semitones; base plus extra must stay within −23…+24. A combination outside that
envelope is refused, so the full extra-offset range is not available at every
base note.

**Original Key** removes source correction and resets both base and extra shift
to the detected original choice. **Transposition Reset** clears only the extra
offset. These buttons have different effects; a reset does not rerun detection.

The sidebar's **Nominal result** is the requested musical relationship. With Key
Lock OFF, changing playback speed also changes the sounding pitch. Do not treat
the nominal label as a measurement of the rendered audio.

### Two Retrigger preferences

Piano and Transposition have separate **Retrigger** checkboxes. With the relevant
option ON, a pitch choice also requests a restart. With it OFF, the choice keeps
the running position or stopped state. Preparation can show **Preparing pitch
choice…**; a requested choice is not yet a confirmed audio effect.

Learned pitch bindings capture their own SET versus SET_RETRIGGER behavior and
target. Editing the visible Retrigger checkbox later does not rewrite an existing
mapping. Fixed offset stores the exact offset chosen before learning.

See [pitch and Key Lock architecture](architecture/pitch-and-key-lock.md) for
numeric ranges, scheduling, preparation and retained sound-quality limits.

## Key Lock

Key Lock is the pitch-preservation processing used when tempo/speed changes.
The global **KEY LOCK** control supplies a default. Each PAD can inherit it or
have an explicit override:

| PAD Key Lock gesture | Intent |
| --- | --- |
| Left-click PAD Key Lock | Explicit ON. |
| Right-click PAD Key Lock | Explicit OFF. |
| GLOBAL | Clear the override and inherit the current global setting. |

A PAD with an explicit ON/OFF stays explicit when the global value changes,
including a repeated same-value global choice. Check the requested/effective
**Audio** caption and preparation/refusal feedback. Merely changing a preference
does not certify audio adoption.

Settings offers **Key Lock start behavior** while playback is stopped:

- **First sound at grid**, the fresh default, stays silent before the grid. A
  source's quiet lead-in can place its main attack correspondingly later.
- **Attack at grid** aligns the main attack; required quiet lead-in may be audible
  before the grid.

These are different onset policies, not universal acoustic latency guarantees.
They do not remove interface buffering, source attacks, processing latency or
time-stretch artifacts. Key Lock, pitch changes and large rate changes need
prepared context and computer headroom. Test demanding combinations on your own
system before performance.

## Gain, isolator and master

### PAD Gain / Trim

Each PAD has **Gain / Trim**, from **−60 to +12 dB**, initially 0 dB.

- Left-click/drag the track for the main adjustment.
- Right-drag for fine adjustment; a right-click without dragging steps by 0.1 dB
  according to the clicked side.
- Wheel over it changes by 0.5 dB per step.
- Middle-click restores 0 dB.

The adjacent PAD meter is before master volume. A PAD clip indication cannot be
cleared by lowering master alone; reduce that PAD's level or processing boost.

### Low, Mid and High isolator

The three per-PAD bands range from **−60 to +6 dB**, initially 0 dB.

| Gesture | Band adjustment |
| --- | --- |
| Left-drag knob | Adjust continuously. |
| Right-click | Set the minimum. |
| Middle-click | Reset to 0 dB. |
| Wheel | Adjust by 1 dB. |
| Double-click numeric value | Enter a value directly. |

These settings belong to that PAD; editing one PAD does not change another
PAD's EQ or gain.

### Master Volume and clipping

The bottom **Master Volume** controls the combined output from 0–100%.
Left-drag adjusts, right-click sets 0%, middle-click sets 100%, and the wheel
steps by 5%. The master meter reflects the summed output. Layering individually
safe PADs can still overload the mix. Leave headroom and reduce levels when
**CLIP** is shown; the indicator is not a guarantee of limiting or distortion-free
output. Temporary START/STOP middle-button mute is separate from editing this
saved volume.

## Stems

Stems are prepared offline. They are not neural separation inside the audio
callback. The fresh separator is **BS-RoFormer MUSDB18HQ**; **Demucs htdemucs**
is also supported. Model setup and FFmpeg requirements are in
[stem setup](stem-generation-setup.md).

### Generate and choose

1. Select a loaded PAD and choose **Generate Stems**.
2. Read progress/stage and the confirmed **Stem inference** device. An alignment
   stage after inference can still be running.
3. Wait for the saved stem set. **Available** describes usable cached files.
4. Choose **ALL STEMS** to request component playback, or **FULL MIX** for the
   original mixture.
5. Check the **Audio** caption before treating the switch as effective.

Generation can use retained readable source audio while the PAD is playing or
paused. It can still be blocked by missing resources, source state, cancellation
or pressure. Settings changes apply to new requests and do not rewrite a running
job or an existing stem set.

### Component buttons

The bottom bar has **V** (vocals), **D** (drums), **M** (melody) and **B** (bass).
Left-click toggles components in a custom mix; right-click solos that component.
**I** requests the instrumental preset; **A** requests all components. Clicking
the active I/A preset again restores the prior custom mask. A component click
from a preset enters a custom component selection.

These buttons work with prepared component audio. FULL MIX is the original
mixture and does not become a separated source by changing a mask. An imported
file with supported declared stem metadata can also supply components; an
ordinary filename alone is not proof of a valid declared stem bundle.

### Saved, resident and effective are different

The sidebar can show:

| Caption | Meaning |
| --- | --- |
| Available / New stems saved | Files exist; they have not necessarily replaced current audio. |
| Stems waiting for resident PCM allowance | Needed audio is waiting for memory admission. |
| Audio: stem selection pending / ALL STEMS pending | Registration or switch remains outstanding. |
| Audio mode unconfirmed | The current request has no successful matching audio confirmation. |
| Last confirmed audio: previous stems | New files are saved while older stems still own playback. |
| Audio: FULL MIX / ALL STEMS | The displayed operation has effective audio confirmation. |

Do not repeatedly regenerate merely because a saved set is awaiting an audio
switch. Resolve the displayed admission/preparation error, or explicitly request
the intended mode. A disk/config choice is not audio adoption.

### Settings, batch work and deletion

Settings provides separator selection and the Demucs-specific shifts/overlap
controls. The fresh Demucs values are one shift and 0.5 overlap. Higher processing
settings can cost substantially more time; they do not change already generated
stems.

**Generate Stems of all loaded PADs** and **Delete Stems of all loaded PADs**
require confirmation. Their captured work covers loaded assignments across all
six banks. Later new content is not silently added; changed assignments can be
skipped. Read the settled/saved/failed/skipped counts. **Cancel batch** withdraws
that batch's interest and waits for its owned work to settle; it is not permission
to terminate unrelated readers. **Dismiss batch results** closes the summary.

**Delete Stems** removes the selected interest/choices and requests safe cleanup.
Shared users and active readers can retain physical artifacts. A settled deletion
summary does not prove all files were immediately removed. Leave cleanup to the
application rather than deleting shared containers by hand.

**Preload existing stem loops at startup** requests preparation of existing
component loops. It generates no missing stems. With this option OFF, a saved
ALL STEMS intent can wait for an explicit component request after restore.
FullMix with preload OFF does not require all components to be resident.

## Keyboard and MIDI Learn

Settings → **Input Mapping** enables/disables mappings. The bottom **L** button
enters Learn when mappings are enabled. There is no universal fixed keyboard
layout: learned bindings are the performer's choices.

1. Click L.
2. Press the keyboard key or operate the MIDI Note/CC control to capture its
   input. The app shows the pending input.
3. Choose the supported on-screen action to bind it: for example a PAD trigger,
   transport, parameter, stem choice, Piano note or offset step.
4. Test the mapping after Learn finishes.

During Learn, the captured input and chosen target are consumed for learning
rather than ordinary playback. Click L to cancel before capture; when an input
is pending, its tooltip offers deletion of that captured mapping. Settings also
has **Delete all Keyboard Mappings** and **Delete all MIDI Mappings**. Use those
deliberately; they affect their respective binding tables.

Pitch's expanded **MIDI / octave steps** supports **Button**, **Absolute
encoder**, **Relative 1 / 127** and **Relative 65 / 63** modes. Match the controller's
actual CC protocol. Set the desired fixed offset and Retrigger preference before
learning; those are captured in the binding. A learned step is different from a
fixed absolute offset. Learn the specific action you intend rather than assuming
all controller types send the same values.

MIDI devices are discovered by the input runtime; there is no MIDI-port picker
in Settings. Ensure the operating system sees the controller. Keyboard handling
respects UI text entry/capture; typing into an editor must not be used as a test
of a performance mapping. Check the pending/error text if an action is unavailable
or refused. Input enablement is saved with the project, while actual bindings
are separate files in `config/input/keyboard.json` and `config/input/midi.json`.

## Recording and output files

RECORD captures the **output mix**. It is independent of whether the selected
PAD is the one currently playing.

| RECORD gesture | Action |
| --- | --- |
| Left press from idle | Prepare capture and start/restart the captured global group. |
| Right-click from idle | Record the current mix without changing PAD playback. |
| Either gesture during effective capture | Stop capture and finalize the take. |

START/STOP continues to control playback independently. Stopping capture does
not itself promise that playback stops. Group-coupled recording retains the
original group while preparing; it cannot substitute newly selected content to
complete an old request.

### Format and lifecycle

Settings → Recording edits defaults for **future takes**:

- **FLAC:** 24-bit or 16-bit; compression 0–8. Fresh default is 24-bit/8.
  Compression changes file size and encoding time while remaining lossless.
- **WAV:** 32-bit float, 24-bit PCM or 16-bit PCM. WAV's retained default precision
  is float32; long output can use RF64.
- **MP3:** CBR 320, 256, 192 or 128 kbit/s; retained default is 320.

A prepared take retains its chosen options even if future defaults change.
Required codec preflight runs before capture; encoding and final file verification
happen outside realtime. Watch **Preparing → Recording → Finalizing → Saved**.
Only confirmed effective capture earns the active recording color. Preparing and
Finalizing hold the one recording owner, so another take cannot begin yet.

Hover RECORD for effective capture, phase, pending work, notice, last saved path
and incomplete candidates. A **Failed** take needs explicit **ACK**, but ACK is
disabled until capture/worker retirement is actually safe. Cleanup uncertainty
cannot be dismissed into permission to overwrite a live owner.

Each take has a unique directory under **`record/`**, relative to the app's
startup working directory. The completed path appears in status/tooltip. A custom
project-config path does not relocate recording output. Retain the whole take
directory if an operation failed; an intermediate spool is not a verified final
recording.

Startup can report owned incomplete takes. It does not automatically repair,
encode, publish or delete them. Keep candidates for diagnosis/recovery instead
of renaming a partial file as successful output.

### Other export expectations

The performer UI provides mix recording and prepared stem files. It has no
separate loop-render/export dialog or DAW-style multitrack export workflow.
Developer PCM/analysis export APIs described elsewhere are diagnostics, not an
additional UI command. Use recording for the implemented mix-output workflow.

## Saving, recovery and closing

Project intent is saved automatically through the ordinary configuration writer.
This includes assignments, musical metadata/loops, PAD mix/pitch/Key Lock/stem
choices, global preferences and selected PAD/bank. It does not serialize a live
audio callback, an in-flight ticket or a recording as if already complete.
Ordinary edits use a ten-second save debounce; do not assume every mouse movement
has already reached disk. Closing attempts a final flush and reports failures.

For ordinary source starts, project configuration is
**`samples/flitzis_looper.config.json`** and managed material is under **`samples/`**.
Keyboard/MIDI bindings are under **`config/input/`**; recordings are under
**`record/`**. Portable keeps installation-owned runtime and data with its install;
use its documented folders rather than a developer workspace's paths.

When backing up a set, close the app cleanly and preserve configuration, the
complete referenced material tree, input mappings and any recordings you want
to retain. Material can be shared across PADs. Copying only a config file or
deleting a seemingly unused container can lose required source/stem/recovery data.

On restart, the app validates saved data, reloads usable assignments and obtains
fresh source/audio confirmation. Saved accepted timing, selected stems or a
Key Lock preference do not immediately prove current Native adoption. Missing
ordinary media can clear an unusable assignment; recovery-owned or ambiguous
operations can remain visible and fenced instead of guessing.

Copy/Swap/Bank and cleanup journals protect interrupted operations. If recovery
cannot prove ownership, preserve the files and the reported error. Do not edit
journals or remove unknown media to clear a fence. Field-level recovery can
retain valid sibling preferences while warning about an invalid setting; a
broader unsafe-config load error can prevent saving. A failed save preserves
the prior config and remains a real error.

Close with the normal window close action. The UI continues polling while
recording, arrangements, holds and workers settle. Recording close handling has
a fixed deadline, not a guarantee that every failed stream or encoder is safely
released. Repeated close attempts do not grant earlier cleanup. Read any save,
recording or shutdown error and keep the affected material/candidate files.

The app's atomic config replacement reduces partial-write risk; it is not a
general power-loss or storage-failure guarantee. Backups remain useful. See
[state and recovery architecture](architecture/state-input-and-ui.md) for the
ownership and persistence boundaries.

## Memory and performance

The productive optimization target is **2, 4 and 6 simultaneous PADs**. Synthetic
performance stress uses at most **10**. More than ten simultaneous PADs can cause
glitches or other performance problems. There is no hard ten-PAD application cap
and no device guarantee for seven to ten; 216 slots are not 216 tested concurrent
voices. Actual capacity depends on CPU, interface, sample rate, loop size,
Key Lock/pitch, stems and concurrent work.

Use a matching Release build for performance. Prepare demanding material before
a set and try the intended combinations on the actual computer/interface.
Large whole-track regions, component stems and pitch history can need more
memory than a short full-mix loop. A resident view is not the complete source;
the complete source remains available on disk for supported preparation/analysis.

### Resident Audio RAM

Settings shows **Automatic memory admission**, the retained manual limit, current
resident accounting, host headroom and preparation status. Automatic admission
uses current host information; missing monitoring can make new allocations wait.
A manual limit bounds the tracked resident-PCM allowance. The UI can choose
1–16 GiB while retaining a previously saved valid limit such as the fresh 512 MiB.

These counters are not total process or system memory. Models, processing state,
OS caches and other allocations have their own costs. Reducing a limit does not
authorize deleting source material or an active reader's buffers. Wait for work
to settle, reduce the requested loop/context or adjust the allowance deliberately.
Preloading stems increases preparation demand; it does not generate missing files.

Automatic analysis has its own bounded preparation admission. Optional Key can
report a budget skip while BPM succeeds. This is a truthful unavailable Key result,
not a fabricated successful classification. A lower resident playback setting and
the analysis preparation budget are different controls.

## Troubleshooting

| Symptom or status | What to check |
| --- | --- |
| No sound | OS default output chosen before launch; master/PAD gain; momentary mute released; PAD actually loaded and started; stem mask; pending/error captions. |
| `start.bat` refuses launch | Prepare the matching Release using the development instructions; do not treat launch-only start as a builder. Close the app before replacing Native. |
| Worker/model missing | Run the documented setup/check path for your installation. Portable rerun repairs its own resources; source uses setup-runtime. Installed files alone do not prove successful inference. |
| Wrong BPM or audible drift | Check half/double, manual/Tap value, loop length and Grid Offset separately. Mean estimates are approximate; changing-tempo material is not phase-followed. |
| Key unknown or musically wrong | Read Key's error/skip independently of BPM; correct Source metadata when known. Classification and displayed nominal pitch are not musical ground truth. |
| Trigger or mode change pending | Wait for source/history/resident preparation and read admission feedback. A selected or saved request is not effective audio. |
| Seek/ALL/context refused | Requested source context may exceed allowance or not be ready. Use a smaller region or resolve resource/error status, then request again. |
| Stems saved but old audio remains | Inspect Audio pending/unconfirmed/previous-stems text. Explicitly request the intended mode after resolving allowance/preparation; generation success is only disk publication. |
| Learn does not play a target | Learn consumes input/target choices. Finish or cancel Learn, check Input Mapping and controller protocol, then test outside text-entry fields. |
| Recording busy or Failed | Wait for finalization/retirement, inspect hover details and terminal; ACK only when enabled. Preserve incomplete take directories. |
| Save/recovery fence | Preserve prior config/material/journals, keep the error details and resolve the actual ownership/storage issue. Do not manually remove unknown files. |
| Glitches under load | Use Release, reduce concurrent PADs/processing/preloading and test the actual output configuration. The six-PAD target is not a guarantee for every device or effect combination. |

For a useful report, retain the exact action sequence, selected PAD/bank, relevant
settings, visible requested/pending/Audio state and terminal error. Include whether
the installation is source or Portable and whether a change was still preparing.
Do not replace a failure report with an assumption that a queued action succeeded.

For current implementation detail, follow the [documentation map](README.md).
Historical reports preserve their original failures, UNKNOWN outcomes and open
acceptance gates; they are not additional performer controls or an automatic
feature queue.
