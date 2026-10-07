# Productive loopback and sustained listening packet

G3c3 prepares measurement; G3c/G3 remain open until actual device recordings and
an uninterrupted human listening session have been assessed. Their manual
acceptance is deferred to the end of the authorized pre-port program; separately
authorized later preparation can continue with automated/numerical checks and
realtime/source-identity requirements intact. The corrected
[musical trajectory](loop-period-proof.md) already passes hardware-free proof.
The packet below records productive application output, not a standalone tone
generator or a grid-only simulation. The human starts, plays and closes the app
and recorder. Offline commands never instantiate the application controller.

## Kurzanleitung: Was wird hier getestet?

Dieser Startbefehl öffnet ein eigenes Testprojekt mit einer bekannten Klickdatei.
Es gibt zwei getrennte Prüfungen:

- **Hörtest:** Mindestens 30 Minuten durchgehend hören, ob die Schleifen gleichmäßig
  laufen und ob Knackser, Aussetzer, Verzerrungen oder zunehmender Versatz auftreten.
- **Technische Messung:** Die tatsächlich ausgegebenen Klicks als unveränderte WAV
  aufnehmen. Die Auswertung prüft ihre zeitlichen Abstände am Audioausgang, auch bei
  veränderter Geschwindigkeit und kurzen Schleifen. Die Aufnahme ist dafür nötig;
  sie ersetzt den Hörtest nicht.

Wenn dafür gerade keine Zeit ist, das Testfenster schließen. Es wird dadurch
kein Hör- oder Geräte-Test als bestanden protokolliert. Diese beiden Prüfungen bleiben
offen, bis echte Messungen und deine tatsächlichen Beobachtungen vorliegen.

Für einen späteren vollständigen Test gilt diese Reihenfolge:

1. Ein frisch vorbereitetes Testprojekt starten und im Startterminal auf `READY`
   warten. Das offene Programmfenster allein bedeutet noch nicht, dass die
   Vorbereitung erfolgreich war. Bei `NOT READY` mit einem Vorbereitungsfehler abbrechen; dieser
   Versuch bleibt als fehlgeschlagen erhalten und zählt nicht als Test.
2. Vorher die passende Aufnahme des tatsächlich verwendeten Audioausgangs einrichten.
   Die Messroute und ihre Einstellungen müssen feststehen. Die Aufnahme zuerst
   starten, dann **Pad #1** im Looper anklicken. Die Befehle bezeichnen dasselbe
   Pad als `--pad-ids 0`.
3. Sobald die Klicks gleichmäßig hörbar sind, den Startzustand mit dem untenstehenden
   `request ... --label dry-start --pad-ids 0` sichern. Erst dann das durchgehende
   Hörintervall beginnen. Zunächst nur Pad #1 spielen lassen und dessen Quelle,
   Schleifenlänge, Tempo, Key Lock und Klangregler unverändert lassen.
4. Durchgehend mindestens 30 Minuten zuhören. Bei Beginn sowie Minute 5, 10, 15, 20,
   25 und 30 kurz notieren, was hörbar ist. Bei einem Problem den Zeitpunkt und die
   Art des Problems notieren. Eine Unterbrechung beendet dieses Hörintervall.
5. Während Pad #1 noch spielt, den Endzustand mit `--label dry-end --pad-ids 0`
   sichern. Danach Wiedergabe und Aufnahme stoppen und die ursprüngliche WAV
   speichern. Erst diese echten Dateien und Notizen bilden die Grundlage der
   späteren Auswertung; das Programm erklärt den Test nicht automatisch für bestanden.

Freies Ausprobieren vor dieser Reihenfolge wird nicht als kontrollierter Test
gewertet. Jeder Start bekommt einen eigenen Beobachtungsordner. Gespeicherte
Änderungen am Testprojekt bleiben jedoch auch beim nächsten Start erhalten: Ein
neuer Beobachtungsordner setzt die Regler nicht zurück. Nach verändertem Testprojekt
ein neues Paket mit `prepare --output-dir <neuer-Pfad>` vorbereiten; den bisherigen
Ordner samt Konfiguration und Fehlversuchen erhalten.

## Evidence that must travel together

| Artifact | Required identity and purpose |
| --- | --- |
| Private session plan and isolated project | Source bytes, explicit independent quarter/origin/error/acceptance assertions, pad and loop/control intent; preserves the existing project. |
| Productive run observations | Actual current source and complete accepted revision/acknowledgement, negotiated output clock and rate; historical tickets alone are insufficient. |
| Original capture WAV | Untouched complete PCM recording, capture endpoint/tool/driver/channel identities, negotiated recording rate and clock relation; no trimming, normalization or post-record resampling. |
| Feature receipt and associations | Whole-capture threshold features measured without predicted windows, frozen detector policy, explicit same-feature cycle associations and provenance. |
| Clock calibration | Independently established output frames per recorder frame, uncertainty and provenance; never estimated by fitting the same loop's candidate period. |
| Comparison receipt | Unwrapped recurrence at cycles 75/1000 in loaded-source units, sampling/clock/feature uncertainty and fixed seam/DSP offset separately. |
| Human listening record | Actual observer, uninterrupted >=1800 seconds, run/capture hashes, timestamped audible observations and explicit decision. |

Keep original WAVs, project/configuration snapshots and all receipts under the
workspace `exports/` or `scratch/`, outside Git. Every run uses a fresh directory
and outputs use fresh filenames. Preserve failed, clipped, interrupted and
inconclusive recordings with their reasons; do not replace them with a rerun.

## Concrete commands

Run offline commands from the active repository. The prepared workspace packet
uses `exports/g3c3-startup-corrected-20261007/`. Only the human executes `run`.
The original `exports/g3c3-device-packet-20261007/` remains retained, including its
failed human-started preparation and modified private project settings. The fresh
packet resets those settings by creating new files, preserving the earlier files.
Relative private artifact paths, including `--plan`, resolve against `--workspace`
regardless of the terminal's working directory. An absolute path inside the
workspace is also accepted; repository and external paths remain rejected.

```powershell
$workspace = "D:\NEUES\Windows Dokumente\Dokumente\CODING-PROJEKTE\flitzis-looper"
$packet = "$workspace\exports\g3c3-startup-corrected-20261007"
uv run --no-sync python -m flitzis_looper.analysis.productive_loop_packet --workspace "$workspace" prepare --source "test-audio/metronom_120_BPM.wav" --authored-quarter-fixture --output-dir "exports/g3c3-new-run" --pads 6 --loop-beats 2 --speed 0.73
uv run --no-sync python -m flitzis_looper.analysis.productive_loop_packet --workspace "$workspace" run --plan "exports/g3c3-startup-corrected-20261007/listening073/run-plan.json"
```

`prepare` verifies the complete known authored PCM24 source and quarter landmarks,
copies it into a new isolated project and freezes its independent source-quarter
assertion. Other sources require a complete separately supplied `--reference`
file; preparation does not invent music labels. It starts no engine or device.
The human-run adapter prepares actual native raw QM, maps each actual event to
those independent landmarks, publishes through the existing controller, and
waits for current acceptance and derived refresh. It does not start any pad.
Retain every native preparation/mapping/error artifact. The run remains unusable
for accepted-path comparison if either acknowledgement is missing.

The complete ten-minute source needs more than the normal 512 MiB timing staging
budget when loaded at 48 kHz/stereo. The explicit adapter records actual loaded
geometry and derives a finite per-preparation budget capped at 1 GiB. Native
allocation checks and complete source/evidence verification remain in force,
including export under the same runtime policy. Normal preparation/restoration
defaults remain 512 MiB; this staging policy is not an overall app memory limit
or a device-performance claim.

Wait for `READY` in the launch terminal and the immutable `setup-ready.json`.
The directory is named in `latest-session.json`; this mutable discovery pointer
is not comparison evidence. Start the recorder, manually start UI pad #1 (native
pad ID 0), and after
controls settle request a fresh immutable observation from a second terminal:

```powershell
uv run --no-sync python -m flitzis_looper.analysis.productive_loop_packet --workspace "$workspace" request --plan "exports/g3c3-startup-corrected-20261007/listening073/run-plan.json" --label dry-start --pad-ids 0
```

For a fixed multi-pad portion, request exactly its playing pads (`--pad-ids 0 1`
or `0 1 2 3 4 5`). Paused, inactive, ambiguous, unsettled or mismatched voices
block comparison. Each pad's callback/output frame is retained separately;
sequential requests are not a simultaneous multi-pad frame observation. The
worker writes `observations/<session-id>/<label>-<request-id>-productive-run.json`
or a retained failure artifact. At the listening interval end, while the reference
is still playing, request `--label dry-end --pad-ids 0` before manual stop.
Only then stop/save the recorder and close the application yourself. Snapshot
requests observe only; they do not play, stop, seek, capture or infer listening.

After manual playback and recording, extract the untouched capture offline:

```powershell
uv run --no-sync python -m flitzis_looper.analysis.loop_capture --workspace "$workspace" features --capture "exports/g3c3-startup-corrected-20261007/listening073/capture.wav" --policy "exports/g3c3-startup-corrected-20261007/detector-policy.json" --evidence-kind real_device_loopback --output "exports/g3c3-startup-corrected-20261007/listening073/features.json"
uv run --no-sync python -m flitzis_looper.analysis.loop_capture --workspace "$workspace" comparison-draft --run "exports/g3c3-startup-corrected-20261007/listening073/observations/start.json" --features "exports/g3c3-startup-corrected-20261007/listening073/features.json" --output "exports/g3c3-startup-corrected-20261007/listening073/comparison-draft.json"
uv run --no-sync python -m flitzis_looper.analysis.loop_capture --workspace "$workspace" listening-draft --run "exports/g3c3-startup-corrected-20261007/listening073/observations/start.json" --features "exports/g3c3-startup-corrected-20261007/listening073/features.json" --output "exports/g3c3-startup-corrected-20261007/listening073/listening-draft.json"
```

Replace `start.json` with the actual fresh immutable observation filename returned
by the runner. Fill a new copy of the comparison draft with recorder/clock
identity and calibration artifact, capture UTC start, explicit channel/pad
identity, same-feature event/cycle associations and offset/uncertainty provenance.
Keep the entire measured edge sequence. Event IDs identify edges, not cycles.
For the two-quarter authored loop there are normally two source pulses per loop;
do not equate each edge to a loop without inspecting the actual source/loop and
declaring the association. Missing, extra or inseparable features need explicit
assessment. The listening draft additionally binds the actual end observation,
capture frame interval, human UTC interval and observations.

Choose declared cycle 0 from a stable repeated occurrence after startup, not the
first manually launched pulse. Discard startup, tails and processing transitions
from the steady recurrence segment. Fractional seam interpolation can displace
the repeated threshold feature relative to launch; Key Lock adds its own startup
offset. Retain initial-launch absolute alignment separately. Fixed offsets cancel
only when comparing the same recurring feature in the same processing context.

```powershell
uv run --no-sync python -m flitzis_looper.analysis.loop_capture --workspace "$workspace" compare --input "exports/g3c3-startup-corrected-20261007/listening073/comparison-input.json" --output "exports/g3c3-startup-corrected-20261007/listening073/comparison-receipt.json"
uv run --no-sync python -m flitzis_looper.analysis.loop_capture --workspace "$workspace" listening-receipt --input "exports/g3c3-startup-corrected-20261007/listening073/listening-input.json" --output "exports/g3c3-startup-corrected-20261007/listening073/listening-receipt.json"
```

Both receipts leave G3 gates open for independent review. Empty pre-capture forms
can be generated by omitting `--run` and `--features` from either draft command.
The prepared packet already contains those forms. They intentionally contain no
recorded hashes, actual clock calibration, listening declarations or results.
Its engineering detector policy uses channel 0, high 0.01, low 0.001, 96
consecutive below-low samples to rearm and minimum gap 1. Those thresholds were
checked against the unchanged reference independently scaled by the prepared
master gain 0.1. The original capture retains all channels; this policy measures
only the named channel. For additional channels, freeze a new declared policy
before that recording. Actual recorder attenuation, noise or different DSP may
make the policy unsupported; retain the preliminary take and its reason before
declaring a new policy. Do not tune thresholds against candidate-predicted beats.

| Prepared private project | Initial pads / beats / speed | Purpose |
| --- | --- | --- |
| `listening073/run-plan.json` | 6 / 2 / 0.73 | Main sustained session; start the reference manually, then add other pads by the recorded protocol. |
| `wholequarter-unity/run-plan.json` | 1 / 2 / 1 | Whole-quarter unity reference; actual P/H decide integer status. |
| `short073/run-plan.json` | 1 / 1/16 / 0.73 | Fast fractional seam recording through 1000 cycles; observe actual recurrence and feature suitability. |
| `short125/run-plan.json` | 1 / 1/16 / 1.25 | Faster fractional-rate companion; sampling uncertainty remains explicit. |

The short cases take roughly one minute to reach cycle 1000 at the known source
tempo. Use the actual accepted P/r and negotiated output rate to determine the
recorded duration, keeping pre/post-roll. Native Key Lock may make these short
features unsuitable for this threshold policy; retain that failure and compare
the stable two-quarter case separately. Run projects serially, closing one before
starting the next. The packet freezes preparations, not device capability or
current realtime performance claims.

## Recording conditions and coverage

Before recording, identify the actual default output endpoint used by the run,
its host/driver, negotiated rate/channels/buffer policy, recorder endpoint/tool,
and how recorder and output clocks relate. The negotiated native descriptor and
output-clock snapshots complement those human declarations. CPAL's estimated
playback timestamp does not measure acoustic onset or downstream device delay.
Nominally identical WAV/output rates do not establish a common clock. A physical
interface or a loopback recorder needs its actual clock relationship recorded;
unknown calibration gives inconclusive drift rather than an invented pass.

Record the full productive master output from before the first manual trigger
until after the last audible tail. Disable recorder normalization, silence
removal and automatic gain. Keep channel identity and original recording rate.
Check available recording space and headroom in a short preliminary recording;
clipping or missing channel coverage invalidates the affected measurements.
Digital loopback measures its recorded path. A physical output/cable/ADC path
measures additional converters and clocks. State which path was actually used.

Freeze each steady measurement case before playback. Wait for actual current
accepted ownership and derived refresh completion; a formatted BPM display or
normal Analyze result cannot establish that ownership. Do not change source,
loop, tempo, Key Lock, mask or DSP during a recurrence segment. Restart a fresh
segment and record its boundaries after a deliberate change. A controller intent
snapshot alone cannot prove precisely when a parameter became audible.

| Case | Human operation and evidence |
| --- | --- |
| Whole-quarter control | Unchanged private metronome, compatible whole-quarter/whole-bar loop, unity source rate, Key Lock off, full mix and neutral EQ. Measure the same feature through 1000 cycles. Call it an integer-period control only if the actual snapshot proves P=H. |
| Fractional loop | Current accepted compatible loop with P != H; preserve exact P/H and endpoints. Run source rates 0.73 and 1.25 where actually admitted. Measure 75 and 1000 cycles, without modulo phase scoring. |
| Native processing | Repeat a fixed fractional-rate case with Key Lock enabled. Record steady recurrence, startup offset and wet/dry transitions separately. |
| Source selection / DSP | If prepared current same-source stems exist, record full mix, ALL STEMS and selected masks with their actual cache identities; exercise nonneutral EQ separately. Missing stems remain uncovered, not fabricated. |
| Inter-pad and listening | Exercise 1/2/4/6 pads as an operating test target using actual accepted sources. Preserve configurations and audible observations; this is not a performance guarantee. |

A master recording of several identical coincident pulses cannot identify each
pad separately. Use solo reference takes and distinguishable source features or
independently identified channels for inter-pad measurements. Unresolvable mixed
features are unsupported. Do not claim per-pad waveform alignment from an
aggregate peak. Independent authored diagnostic pulse counts are fixture
assertions; they are not general musical labels or analyzer/default acceptance.
The productive raw-QM accepted fit is retained exactly, including its uncertainty;
the authored source's 120-BPM identity does not authorize replacing that fit with
exact 120 BPM. Numerical recurrence is measured against the actual accepted P,
while source-tempo estimation quality remains its separate acceptance question.

## Thirty-minute human session

Begin the >=1800-second uninterrupted interval only once the selected accepted
configuration is ready and audible. Start the recorder first and record the
listening interval's capture frame boundaries and UTC times. Listen continuously
through the actual output path. Request immutable checkpoint snapshots manually
at the beginning, end and each steady configuration change.
Keep one steady accepted reference audible throughout and identify any deliberate
changes to additional pads. If you stop listening, pause the reference, lose the
output, or change its timing/source/Key Lock/stem/EQ configuration, end that
interval and retain it as incomplete.

Record observations at the beginning and at least at minutes 5, 10, 15, 20, 25
and 30. Note accumulating offset, missed/doubled pulses, clicks at loop seams,
dropouts, distortion and inter-pad alignment, with `none observed` only when
actually assessed. Add event timestamps for manual starts/stops, pause/resume,
seek/retrigger, loop edits, Key Lock toggles, stem-mask and EQ changes on the
other pads; these are transition observations, not steady-period points. Keep
one, two, four and six-pad portions identifiable. Record unsupported or failed
conditions explicitly, including unavailable prepared stems.

The final human record gives `pass`, `fail` or `inconclusive` and names the
assessor. A tool can check duration, identities and consistency; it cannot prove
that the human listened or automatically approve G3. A fixed Key Lock onset
offset, seam feature displacement, recorder drift and cumulative musical error
must retain their separate explanations. B5 audible compensation and C1
immutable-source/cache work remain separate; current 96-handle and save I/O/CPU
costs remain unmeasured.

## Assessment and continuation

Compare only identical source/accepted revision, loop, steady source rate and
processing conditions. For matched feature cycles n, convert capture differences
to output frames using the independent clock relationship, then to loaded frames
using the once-authoritative source rate. Subtract n*P, never reduce modulo P.
Cancel a fixed feature offset only for the same feature in the same condition;
measure absolute inter-pad/DSP alignment separately. Report the whole uncertainty
interval. A point estimate within one frame whose uncertainty exceeds that limit
does not establish the strict gate.

The numerical <=1 loaded-frame criterion stays unchanged. Synthetic tests,
schema receipts, guessed shared clocks or incomplete listening cannot close the
device/human gates. Actual evidence must be reviewed against the published G3
contracts at the deferred final manual acceptance, with any findings corrected
then. C0 and later separately authorized preparation retain their automated,
numerical and source/realtime checks. There is no automatic analyzer cutover,
variable-map SYNC or full Rust application-port work in this packet.
