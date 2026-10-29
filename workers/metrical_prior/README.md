# Optional nominal-unit worker

These assets run only in a separately verified, physical private installation.
They do not use the App's Python3.14 environment, decode audio, download packages,
choose a physical timing estimate, or publish a PAD result. Native borrows its
protected complete mono PCM and owns candidate admission and final publication.

`prepare-features.py` and `prepare-features-streamed.py` are byte-identical copies
of the E4 and301 reference artifacts. Their historical artifact/pilot wording is
intentionally preserved. Do not format or modernize those files. The local Ruff
configuration excludes them and the unchanged upstream Python sources.

Setup copies this directory physically to the installation root, with its complete
private CPython3.12.13/Node25.5.0/package closure. It stages the verified model
archive at `upstream/deeptemp-k16-3-tfjs.zip` and graph/shard in
`upstream/deeptemp-k16-3-tfjs/`. Model weights, archives, interpreters and package
directories are not repository assets. No symlink, artifact-directory alias,
PATH search or analysis-time installation is supported.

The complete author frontend remains magnitude mel40 at11025Hz, FFT1024/hop512,
256-frame patches/hop128, and whole-overlapping-tensor binary64 normalization
followed by binary16 then binary32. Node's `loadModel`, `predict`, `distributions`,
`aggregate`, `firstMaximum` and synthetic-input functions are byte-identical to
the old9BB reference runner. `numeric-profile.json` is compact sorted-key UTF-8
with one final LF; its exact file hash binds these semantics and provenance.
The inference protocol has fixed batch2, complete binary32 patch distributions,
source-order binary64 mean and first argmax classes30..285. A class is a nominal
prior; it supplies no decimal timing or musical certainty.

Normal Node CLI (absolute paths):

```
node.exe infer-prior.cjs --features MANIFEST --manifest-sha256 SHA256 --batch-size 2 --held-native-bytes N --request-root REQUEST --output NEW_JSON --tfjs-module INSTALL/node_modules/@tensorflow/tfjs --model-dir INSTALL/upstream/deeptemp-k16-3-tfjs --installation-root INSTALL
```

The launcher strips `NODE_OPTIONS` and `NODE_PATH` before starting Node. The worker
clears them again, rejects launch options, and restricts nonbuiltin module
resolutions to the frozen installation. Request inputs and create-new outputs
stay below the Native-owned request directory. That directory and installation
must be disjoint. Every request remains single-source with at most1024 patches.

The manifest is closed schema1 with `identity`, `profile`, `frontend_receipt` and
one `cases` entry. Identity uint64 values are canonical decimal strings on this
Node-only boundary; Python's Native boundary retains integer identity fields.
Profile contains `profile_sha256` and `numeric_profile_sha256`. Cases retain the
reference feature metadata; normalization replaces decimal mean/std with exact
`mean_f64_le_hex` and `std_f64_le_hex`. Receipts and features are hash-bound.

Completed responses echo the input identities/pins, use `execution_origin` equal
to `computed`, and carry canonical little-endian packed probabilities/global
means, SHA256 and base64. Responses are at most2MiB and never include per-batch
diagnostics or an App-facing decimal float matrix. Failure exits nonzero; Python
owns bounded failure/cancellation diagnostics and process-tree retirement.

The additional held-Native+RSS/maxRSS/logical guard is sampled and is not a
Windows private-commit or whole-App hard-cap proof. The parent process observer
owns actual private-memory measurements and real process retirement. Node's
`resources_released` refers to synchronous file/tensor ownership only.

Technical `--smoke` replaces source-manifest/batch flags and uses the same two
deterministic synthetic windows as9BB, comparing batch1 with batch2. It is not a
corpus run. Reusing old observations requires the sealed old9BB/frontend/CDF
provenance and explicit `cached_observation` at the bridge; it must never be
reported as a fresh computation by this worker.

See `THIRD_PARTY_NOTICES.md` and the separate upstream code/weight licenses.
