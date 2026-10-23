# Fixed source checkout, private runtime and recoverable activation

The BAT bootstraps verified private tools and acquires the exact remote main commit.
The final installation contains one complete Git checkout Root/flitzis-looper and
one Root/Runtime; current.json schema2 records installed source identity/status,
not a version path. Candidate source/runtime and backup paths use short operation
IDs under Root/cache. Native DLLs and the built extension belong beside the tracked
Python Native wrapper in the checkout. No duplicate Runtime/app source tree remains.
Start uses that checkout as cwd and reads its native samples/settings/config/input.

START-LOOPER and the installed dispatcher take a shared .install.lock for the entire
child process lifetime. Updating takes an exclusive lock and positively excludes
legacy running runtimes; inaccessible process state stops before mutation. No process
is killed. Candidate preflight has an explicit installer-owned mode so it does not
try to reacquire the updater's lock. All candidate checks use actual physical paths.

Source/runtime identity, private DLL origins, imports, offline-worker and FFmpeg paths
are verified before activation. Data capture occurs BEFORE the original checkout is
renamed, while historical absolute config references still identify their real files.
The Native hook captures bounded recognized registrations/journal histories, exact
Samples and config/metadata identities and bytes. Known configs outside the moved
Samples subtree, unknown/gapped/unsafe records, live custody, changed files, reparse,
collisions or cross-volume moves stop with data preserved. After code swaps the same
Samples directory may be addressed through its backup alias. Native revalidates its
captured physical custody, performs the same-volume rename to the final checkout,
and revalidates IDs/bytes. It never copies/recreates artifact identities.

Historical owner references use one Native location resolver. An exact location
projection inside Samples is checked against a Native custody anchor attached to
the actual directory. Captured root/parent FileIDs and exact relative config address
bind location; first move also checks captured config identity/bytes. Later legitimate
atomic config saves may change the config FileID, while the parent/location custody
continues. This projection supplies no material birth, retirement, runtime readiness,
current revision or callback acknowledgement authority. Existing current config and
fresh own source ACK checks remain authoritative. Journal/config bytes are not rewritten.
Unsupported custody-anchor filesystems stop safely. Rollback restores the prior projection
and Native Samples location before the PowerShell code/runtime swaps are reversed.

Each activation is journalled outside Samples with a validated operation ID. Recovery
verifies actual paths and captured custody; it does not infer successful publication
from existence alone. Successful cleanup removes only complete recorded installer-owned
candidates/downloads/backups. Samples, PCM caches, models, logs and useful measured
build/tool caches remain. New manifests record full source/runtime file inventories.
Legacy Runtime cleanup requires authoritative exact Git blobs, installed distribution
RECORD, pinned interpreter/FFmpeg and original Native manifest proofs; unknown Runtime
extras stop before activation. A recognized current-layout checkout may contain
unregistered files or directories while every registered source file is still complete
and hash-correct. Any such extra disables the unchanged-main execution fast path and
requires a clean candidate. The old checkout is renamed through the same journaled
swap, its Samples/Input move through the existing custody path, and the entire remaining
old checkout stays at its original `cache/o-<operation>` path when extras remain.
Cleanup reports its physical path and completes the accepted journal. It neither
merges extras into the candidate nor executes or recursively removes that backup.

Previous-checkout admission, restored previous-checkout validation and old-backup
cleanup share one narrow generated-Native preservation case. Only the present
`src/flitzis_looper_audio/flitzis_looper_audio.cp314-win_amd64.pyd` may have changed
registered bytes, and only when a valid accepted `native_sha256` equals its original
registered file hash. This generated untracked artifact is distinct from tracked
Python/source files. Changed bytes count as extras, forcing the clean-candidate
path and whole-backup retention without importing or copying the old Native.
Missing Native or a missing/invalid/inconsistent declaration still fails. No other
registered source, ABI filename, Runtime or candidate exception is introduced.
The same strict candidate-custody checks remain unchanged. The preserved binary's
provenance is not inferred from its bytes, timestamp or registration.

New manifests add optional `checkout_directories` beside the schema-2 file inventories.
The builder records empty Git directories and creates only the structural `config`
parent before recording ownership. File-only older manifests derive directory ownership
from registered file ancestors; unproved empty directories trigger conservative
retention once. An already active file-only legacy checkout may pass strict executable
file validation without granting directory-deletion authority. Active Persistent checks
exempt only Samples/Input data and the structural config parent. Old-checkout cleanup
has no Persistent exemption, so any leftover data prevents recursive removal. Runtime
retains strict file ownership and permits only its own root release manifest exception;
the exception never applies to source or Input.

The shared candidate-custody guard validates exact journal commit, schema2,
`source_dirty=false`, complete source and Runtime files/hashes and reparse paths
before candidate execution. Recovery uses it before Native rollback relocation and
before accepted private preflight/model checks. Staged source rejects unknown files
and directories; Runtime rejects unknown or changed files. Directory inventories
cover source only; Runtime directory handling is unchanged and is not verified by
these checks. Active new source allows only journal-bound persistent data paths. Failed
candidate cleanup keeps diagnostics when directory ownership cannot be proved.
Unregistered/changed Runtime files, changed or missing registered source outside
the narrow previous generated-Native preservation case, unknown installations
and reparse trees remain refusals. Models, tools and useful package/
Native caches retain their existing locations and reuse behavior.

Model preparation calls the existing Demucs htdemucs resolver/checksum and pinned
BS-RoFormer musdb18hq asset installer. Parent orchestration is Torch-free. Preparation
children exit before two fresh sequential CPU-only deserializing children; missing,
corrupt or failed models fail the preflight. No inference, GUI or audio device opens.
An unchanged source/runtime can repair only models. Controller and both offline children
share one explicit root/cache environment, including TORCH_HOME.

Public Engine work follows short Bound capture, owned preparation after releasing the
borrow, and fresh try_borrow/current admission. Timing busy permits and source/producer
custody are owned. Actual file verification guards survive through final admission.
Only constant-time current state and existing runtime publication/ownership operations
run during the final Engine borrow. Material source generation, request, preparation
epoch, current sample and own audio ACK must still match. No callback disk/JSON/GIL,
logging, inference, allocation, blocking lock or unbounded work is introduced.

Validation includes actual public receiver/control/render concurrency during real I/O,
stale request/source/unload/timing/capacity negatives, independent fresh Engine behavior,
actual Native relocation/recovery/FileID/bytes/held-reader guards, installer update and
failure paths, fresh private Release/import/preflight and actual model CPU loads.
Clean-Windows bootstrap and Human GUI/device/listening acceptance remain explicit
open gates. Exact successful unchanged-input checks may be reused; failures remain evidence.
Generated-Native preservation regressions use an explicitly labelled MOCK Native
import ledger and fixture bytes, not a real extension load. They distinguish old
changed Native from clean-candidate imports and do not establish real Native,
separator-model, device or Human acceptance.
