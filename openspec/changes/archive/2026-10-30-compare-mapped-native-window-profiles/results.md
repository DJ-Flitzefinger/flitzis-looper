## Bounded comparison result

Both explicit measurements completed their technical assertions: the original
72 streams and the new 72 Short/Medium streams. The original 72 raw/emitted/dry
PCM hashes and stream ledgers, plus all 156 original natural acoustic rows,
reproduced the retained earlier measurement exactly. This verifies the shared
test-helper refactor without changing the earlier negative result.

The new matrix gives these results per existing profile:

| Diagnostic | Short | Medium |
| --- | ---: | ---: |
| Natural isolated acoustic criteria | 0/24 | 0/24 |
| Nominal full-raw timing only | 8/24 | 8/24 |
| Necessary FirstSound peak/energy retention only | 12/24 | 20/24 |
| Nonempty full-raw shared timing interval | 8/24 | 16/24 |
| Joint timing and necessary retention, with natural bounded-response completeness | 4/24 | 8/24 |
| Original natural tone windows | 72/96 | 72/96 |
| Original nominal tone windows | 72/96 | 64/96, eight unavailable |
| Supplemental natural tone windows | 22/48 | 12/48 |
| Supplemental nominal tone windows | 32/48 | 32/48 |

Every nominal timing pass occurs at +7 semitones. Medium retains the first attack
in all eight of those cases; Short meets the joint necessary conditions in four.
Neither profile meets nominal timing at -23 or +24. All 48 nominal fixed end
windows are unavailable in the unchanged capture extent. Combined nominal acoustic
acceptance is therefore explicitly unclaimed. The joint row is a necessary-condition
diagnostic, not actual FirstSound/AttackAtGrid emission or productive admission.

The longer windows preserve eligibility separately. The original natural tone
family has 160/192 eligible windows and 144/192 passes; the nominal family has
184/192 available, 152 eligible and 136 passes. Supplemental natural windows have
92/96 eligible and 34 passes; supplemental nominal windows have 88/96 eligible and
64 passes. Earlier negative or ineligible windows remain part of the result.

Medium is useful for a narrower follow-up on output preparation and first-attack
retention, but this matrix does not justify a productive backend switch or full
SYNC acceptance. Nonzero/KeyLock mapped guards, actual S/T/E integration, complete
intended output, full pitch/rate coverage and every musical/Human/final gate remain.

Exact commands, positive and negative receipts and source-compatible Release reuse
are retained in the existing delivery log; generated synthetic logs are outside Git
under test-artifacts/mapped-native-window-profiles-20261022.
