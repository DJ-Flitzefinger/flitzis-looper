## Existing authority and gap
The UI calls `ProjectPersistence.maybe_flush` each frame and uses
`flush_if_dirty` for immediate settings saves. The common writer verifies current
timing, checks the intent revision, writes and fsyncs a temporary file, then
atomically replaces Config. Only a successful write updates the written revision,
dirty state and last-success timestamp. Failed temporary files already have an
owned cleanup path.

The optional wrappers handle timing and writer-fence rejections, but an ordinary
`OSError` escapes the GUI. Raw `flush`, migration commits and stem-selection
commits must continue raising so their existing owners retain unresolved custody.

## Bounded handling
Both optional wrappers retain the ordinary save error and return false. A separate
monotonic rejection timestamp defers automatic polling for the existing ten-second
debounce interval. An explicit immediate save can attempt again sooner. No sleep,
loop, captured stale snapshot or second persistence pipeline is added: a later
attempt verifies and writes the latest current intent through the same writer.

Only an actual successful snapshot write clears the ordinary save error and retry
timestamp. Timing and fence rejection behavior stays distinct. The existing
console sink observes the retained error with one stable operation name, redacts
paths and deduplicates unchanged failures. Recovery clears the sink observation so
a later recurrence is reported again.

## Verification and limits
Inject failure at the real `os.replace` boundary, inspect unchanged Config bytes,
dirty/revision/timestamp retention and temporary-file cleanup, then permit real
replacement and reopen the latest intent. Cover both optional wrappers, automatic
retry pacing, explicit retry, console deduplication/recurrence and raw migration
commit propagation. Reuse existing timing and writer-fence regressions. These
software checks do not identify the cause of a particular Windows denial or
replace Native, musical, device or Human acceptance gates.
