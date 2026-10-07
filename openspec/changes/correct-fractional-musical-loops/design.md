# One loop domain, one source-rate owner

For effective source-bound accepted quarter period T, loaded rate Fs and compatible
logical duration b, P=Fs*T*b is the source-frame loop period. The existing
SourceGrid rule admits compatible short subdivisions and whole bars within the
one-frame endpoint rounding bound. H=end-start remains the persisted physical
PCM range. Manual/Tap/Legacy and incompatible physical loops retain H.

SourcePlayback integrates the existing accepted source rate over active output
frames and wraps virtual source phase modulo P. Rate epochs retain that phase,
including inside the fractional seam; no H/P speed multiplier is introduced.
Intro/tail seeks traverse admitted physical source until loop entry and then use
P. In-range edits retain phase; normal out-of-domain positions normalize to start.
Period-only accepted refresh/clear with unchanged physical geometry preserves the
wrapped fractional residue. Native/FIFO/filter continuity permits only the exact
new-domain projection of the previously expected next phase, with a changed
effective accepted projection and identical source/physical geometry. Explicit
seeks, source replacement and real marker discontinuities retain their reset path.

The PCM knots retain their original integer offsets. The last admitted knot
below P interpolates to the first at P. Thus the seam can be shorter or longer
than one loaded frame, but every sample tap remains inside [start,end). Exact
P=H retains the existing integer interpolation arithmetic. Full mix, stems and
both source-transition sides borrow the same trajectory and seam weights.

The copied domain includes physical extent/bounds and exact admitted period bits.
SourceReadPlan and SourcePlayback compare it during prepared contract/adoption
checks. Existing current-source/accepted/native-history permits remain the
authority; old pinned voices retain their effective timing when the bank changes.
Productive native/FIFO/filter history remains chronological across continuous
same-source feed; stale copied domains cannot authorize prepared adoption.

Independent fixtures declare rational musical periods and separately record the
admitted fitted IEEE period. Actual cursor wraps and rendered features establish
cycle count and recurrence. The one-loaded-frame continuous unwrapped gate is
separate from output sampling, seam feature offset and IEEE threshold sensitivity.
The unchanged private integer-period WAV is a positive control. G3c device and
sustained human listening, C1 immutable copy-first/ABA, B5 audible compensation
and 96-handle/save I/O/CPU measures remain separate gates.
