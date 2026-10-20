# Preserve independent Copy assignment custody

A cleanup request from one cache assignment currently prevents a surviving valid assignment from admitting an independent Copy. Copy preparation also registers a new durable subscriber before fallible work, without releasing that assignment when preparation fails or is cancelled.

Keep per-assignment authority independent of shared cleanup intent and retain explicit custody of every new subscriber until successful target adoption or safe terminal cancellation. Preserve integrity, retirement and capacity guards and claimed callback ownership.

## Non-goals

No new storage format or realtime ownership registry is introduced, and physical cleanup authority is not broadened.
