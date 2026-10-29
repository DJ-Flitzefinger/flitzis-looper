'use strict';

// Optional offline nominal-unit evidence. The independently admitted timing
// candidates, cancellation and publication remain owned by Native.
delete process.env.NODE_OPTIONS;
delete process.env.NODE_PATH;
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const os = require('node:os');
const Module = require('node:module');

const BLOCK = __dirname;
const LIMIT = 512 * 1024 * 1024;
const WINDOW = 40 * 256;
const MAX_WINDOWS = 1024;
const CLASSES = 256;
const TFJS_VERSION = '4.22.0';
const NUMERIC_PROFILE_ID = 'tempocnn-0.0.8-deeptemp-k16-3-tfjs4.22-node25.5-cpu-f32-v1';
const REFERENCE_RUNNER = '9bbf0037fe210a04a75c812badc38c003ae900cbc1c05aca4ce52b6bbeaa0f21';
const MAX_RESPONSE = 2 * 1024 * 1024;
let heldNativeBytes = 0;
const MODEL = Object.freeze({
  archive: 'd2bdb20ed767919764cffd87d3246b1af3dc040741eceecda87876e52a4312d3',
  graph: 'f93693c5a913b69aa378438665937600c09286f57714359155f2763496aba184',
  shard: 'f7688e8868706813cd74610442f9923fca51162c1ebde6cf7fecfd62ebd24a15',
  graph_bytes: 33208,
  shard_bytes: 1274372,
});
const NORMALIZATION = Object.freeze({
  scope: 'entire-overlapping-window-tensor',
  precision: 'float64',
  roundtrip: 'float16-to-float32',
});

function requireTrue(condition, message) {
  if (!condition) throw new Error(message);
}

function sha256(data) {
  return crypto.createHash('sha256').update(data).digest('hex');
}

function sha256File(file) {
  const hash = crypto.createHash('sha256');
  const bytes = Buffer.allocUnsafe(1024 * 1024);
  const descriptor = fs.openSync(file, 'r');
  try {
    let count;
    while ((count = fs.readSync(descriptor, bytes, 0, bytes.length, null)) > 0) hash.update(bytes.subarray(0, count));
  } finally {
    fs.closeSync(descriptor);
  }
  return hash.digest('hex');
}

function digest(value) {
  return typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
}

function exactKeys(value, names, where) {
  requireTrue(value && typeof value === 'object' && !Array.isArray(value), `${where}: expected object`);
  const keys = Object.keys(value).sort();
  requireTrue(JSON.stringify(keys) === JSON.stringify([...names].sort()), `${where}: unexpected or missing fields`);
}

function readPinned(file, expected, maximum, expectedBytes = null) {
  requireTrue(digest(expected), `Invalid SHA256 pin for ${file}`);
  const size = fs.statSync(file).size;
  requireTrue(Number.isSafeInteger(size) && size > 0 && size <= maximum, `Input byte bound: ${file}`);
  requireTrue(expectedBytes === null || size === expectedBytes, `Input length differs: ${file}`);
  const bytes = fs.readFileSync(file);
  requireTrue(bytes.length === size && sha256(bytes) === expected, `Input identity differs: ${file}`);
  return bytes;
}

function within(root, file, where) {
  const actual = fs.realpathSync(file);
  const relative = path.relative(root, actual);
  requireTrue(relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative)), `${where}: outside the owned root`);
  return actual;
}

function isBelow(root, file) {
  const relative = path.relative(root, file);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
}

function readReference(ref, base, maximum, requestRoot) {
  exactKeys(ref, ['path', 'sha256'], 'file reference');
  requireTrue(typeof ref.path === 'string' && ref.path.length > 0, 'Missing pinned file path');
  return readPinned(within(requestRoot, path.resolve(base, ref.path), 'request input'), ref.sha256, maximum);
}

function writeNew(file, value, requestRoot) {
  const parent = within(requestRoot, path.dirname(path.resolve(file)), 'request output');
  const output = path.join(parent, path.basename(file));
  const serialized = `${JSON.stringify(value)}\n`;
  requireTrue(Buffer.byteLength(serialized) <= MAX_RESPONSE, 'Packed response exceeds two MiB');
  // Outputs are request-owned, and unknown earlier effects are never overwritten.
  fs.writeFileSync(output, serialized, { flag: 'wx', encoding: 'utf8' });
  return output;
}

function memory() {
  const used = process.memoryUsage();
  const maximum = process.resourceUsage().maxRSS;
  return {
    rss_bytes: used.rss,
    process_max_rss_bytes: maximum > 0 ? maximum * 1024 : null,
    heap_used_bytes: used.heapUsed,
    external_bytes: used.external,
    array_buffer_bytes: used.arrayBuffers,
  };
}

function checkMemory(observed, logicalPeak = 0) {
  requireTrue(Number.isFinite(logicalPeak) && logicalPeak >= 0 && logicalPeak < LIMIT, 'TFJS logical peak exceeds the experiment bound');
  requireTrue(observed.rss_bytes < LIMIT && (observed.process_max_rss_bytes === null || observed.process_max_rss_bytes < LIMIT), 'Observed process RSS exceeds the experiment bound');
  const maximum = Math.max(observed.rss_bytes, observed.process_max_rss_bytes ?? 0, logicalPeak);
  requireTrue(heldNativeBytes + maximum <= LIMIT, 'Held Native plus observed RSS/logical peak exceeds512MiB');
}

function firstMaximum(values) {
  let selected = 0;
  for (let index = 1; index < values.length; index++) {
    if (values[index] > values[selected]) selected = index;
  }
  return selected;
}

function distributions(values, count) {
  requireTrue(values.length === count * CLASSES, 'Output is not Bx256');
  const rows = [];
  for (let row = 0; row < count; row++) {
    const probabilities = Array.from(values.subarray(row * CLASSES, (row + 1) * CLASSES));
    let total = 0;
    for (const value of probabilities) {
      requireTrue(Number.isFinite(value) && value >= 0 && value <= 1, 'Nonfinite or invalid class probability');
      total += value;
    }
    requireTrue(Math.abs(total - 1) <= 1e-5, 'Output does not satisfy the finite softmax sum check');
    rows.push(probabilities);
  }
  return rows;
}

function aggregate(rows) {
  requireTrue(rows.length > 0, 'Cannot aggregate an empty set of patches');
  const sums = new Array(CLASSES).fill(0);
  // JS Number gives explicit binary64 left-to-right accumulation in source
  // patch order. No per-batch normalization, class voting or interpolation.
  for (const row of rows) {
    for (let index = 0; index < CLASSES; index++) sums[index] += row[index];
  }
  const probabilities = sums.map(value => value / rows.length);
  const index = firstMaximum(probabilities);
  return { probabilities, class_index: index, nominal_prior_bpm: index + 30 };
}

function syntheticFeatures() {
  const values = new Float32Array(2 * WINDOW);
  for (let patch = 0; patch < 2; patch++) {
    for (let mel = 0; mel < 40; mel++) {
      for (let frame = 0; frame < 256; frame++) {
        // Exact binary fractions are float16-representable; neither source
        // audio nor any BPM label enters this technical graph probe.
        values[patch * WINDOW + mel * 256 + frame] = (((mel * 13 + frame * 7 + patch * 17) % 257) - 128) / 128;
      }
    }
  }
  return values;
}

async function loadModel(tf, directory) {
  readPinned(path.join(BLOCK, 'upstream', 'deeptemp-k16-3-tfjs.zip'), MODEL.archive, 2 * 1024 * 1024, 1187206);
  const graphBytes = readPinned(path.join(directory, 'model.json'), MODEL.graph, 128 * 1024, MODEL.graph_bytes);
  const shard = readPinned(path.join(directory, 'group1-shard1of1.bin'), MODEL.shard, 2 * 1024 * 1024, MODEL.shard_bytes);
  const graph = JSON.parse(graphBytes.toString('utf8'));
  requireTrue(graph.format === 'graph-model' && graph.weightsManifest.length === 1, 'Unexpected pinned graph format');
  const group = graph.weightsManifest[0];
  requireTrue(JSON.stringify(group.paths) === JSON.stringify(['group1-shard1of1.bin']), 'Unexpected weight shards');
  const input = graph.modelTopology.node.filter(node => node.op === 'Placeholder');
  requireTrue(input.length === 1 && input[0].name === 'input', 'Unexpected graph input nodes');
  requireTrue(JSON.stringify(input[0].attr.shape.shape.dim.map(dim => Number(dim.size))) === JSON.stringify([-1, 40, -1, 1]), 'Unexpected graph input axes');
  requireTrue(graph.modelTopology.node.some(node => node.name === 'output' && node.op === 'Softmax'), 'Missing pinned output node');
  const model = await tf.loadGraphModel(tf.io.fromMemory({
    modelTopology: graph.modelTopology,
    format: graph.format,
    generatedBy: graph.generatedBy,
    convertedBy: graph.convertedBy,
    weightSpecs: group.weights,
    weightData: shard.buffer.slice(shard.byteOffset, shard.byteOffset + shard.byteLength),
  }));
  requireTrue(model.inputs.length === 1 && model.inputs[0].name === 'input', 'Loaded graph input differs');
  return model;
}

async function predict(tf, model, values, count, batchSize) {
  requireTrue((batchSize === 1 || batchSize === 2) && Number.isSafeInteger(count) && count >= 1 && count <= MAX_WINDOWS, 'Unapproved batch or patch bound');
  requireTrue(values.length === count * WINDOW, 'Feature shape differs from its byte extent');
  const rows = [];
  const profiles = [];
  for (let start = 0; start < count; start += batchSize) {
    const size = Math.min(batchSize, count - start);
    const before = tf.memory();
    const started = process.hrtime.bigint();
    let input = null;
    let output = null;
    let profile;
    try {
      profile = await tf.profile(() => {
        input = tf.tensor4d(values.subarray(start * WINDOW, (start + size) * WINDOW), [size, 40, 256, 1], 'float32');
        output = model.execute({ input }, 'output');
        requireTrue(output && !Array.isArray(output) && JSON.stringify(output.shape) === JSON.stringify([size, CLASSES]), 'Graph output axes differ from Bx256');
        return output;
      });
      rows.push(...distributions(await output.data(), size));
    } finally {
      if (output) output.dispose();
      if (input) input.dispose();
    }
    const after = tf.memory();
    requireTrue(after.numTensors === before.numTensors && after.numBytes === before.numBytes, 'Inference left live tensors after explicit disposal');
    const observed = memory();
    checkMemory(observed, profile.peakBytes);
    profiles.push({
      first_patch: start, patches: size,
      milliseconds: Number(process.hrtime.bigint() - started) / 1e6,
      tfjs: { new_bytes: profile.newBytes, new_tensors: profile.newTensors, peak_logical_bytes: profile.peakBytes, before, after },
      process: observed,
    });
  }
  return { rows, global: aggregate(rows), profiles };
}

function float64Hex(value, positive = false) {
  requireTrue(typeof value === 'string' && /^[0-9a-f]{16}$/.test(value), 'Expected canonical little-endian binary64 hex');
  const number = Buffer.from(value, 'hex').readDoubleLE(0);
  requireTrue(Number.isFinite(number) && (!positive || number > 0), 'Invalid binary64 normalization statistic');
  return number;
}

function validateIdentity(value) {
  exactKeys(value, ['pad_id', 'request_id', 'source_id', 'source_generation'], 'identity');
  for (const name of ['pad_id', 'request_id', 'source_generation']) {
    requireTrue(typeof value[name] === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value[name]), 'Identity integer must be an exact canonical decimal string');
    const number = BigInt(value[name]);
    requireTrue(number <= 18446744073709551615n && (name === 'pad_id' || number > 0n), 'Identity integer outside uint64');
  }
  requireTrue(typeof value.source_id === 'string' && value.source_id.length > 0 && value.source_id.length <= 4096, 'Invalid source identity');
}

function validateManifest(manifest, base, requestRoot, numericProfileSha256) {
  exactKeys(manifest, ['schema_version', 'identity', 'profile', 'frontend_receipt', 'cases'], 'features manifest');
  requireTrue(manifest.schema_version === 1 && Array.isArray(manifest.cases) && manifest.cases.length === 1, 'Expected exactly one source');
  validateIdentity(manifest.identity);
  exactKeys(manifest.profile, ['profile_sha256', 'numeric_profile_sha256'], 'profile');
  requireTrue(digest(manifest.profile.profile_sha256) && manifest.profile.numeric_profile_sha256 === numericProfileSha256, 'Numeric profile differs');
  readReference(manifest.frontend_receipt, base, 1024 * 1024, requestRoot);
  const item = manifest.cases[0];
  exactKeys(item, ['id', 'source_sha256', 'pcm_sha256', 'shape', 'dtype', 'features', 'normalization', 'provenance_receipt'], 'feature case');
  requireTrue(digest(item.id) && item.id === item.source_sha256 && digest(item.pcm_sha256), 'Invalid source/PCM identity');
  requireTrue(Array.isArray(item.shape) && item.shape.length === 4 && Number.isSafeInteger(item.shape[0]) && item.shape[0] >= 1 && item.shape[0] <= MAX_WINDOWS && item.shape[1] === 40 && item.shape[2] === 256 && item.shape[3] === 1, 'Expected bounded Bx40x256x1 features');
  requireTrue(item.dtype === 'float32-le', 'Expected explicit float32-le features');
  exactKeys(item.features, ['path', 'sha256', 'bytes'], 'features file');
  requireTrue(digest(item.features.sha256) && typeof item.features.path === 'string' && item.features.bytes === item.shape[0] * WINDOW * 4, 'Feature pin/byte extent differs');
  exactKeys(item.normalization, ['scope', 'precision', 'roundtrip', 'mean_f64_le_hex', 'std_f64_le_hex'], 'normalization');
  for (const [key, value] of Object.entries(NORMALIZATION)) requireTrue(item.normalization[key] === value, 'Feature normalization contract differs');
  float64Hex(item.normalization.mean_f64_le_hex);
  float64Hex(item.normalization.std_f64_le_hex, true);
  readReference(item.provenance_receipt, base, 1024 * 1024, requestRoot);
  return item;
}

function restrictModules(installationRoot) {
  // The launcher also strips these BEFORE process creation: deleting them here
  // cannot undo an already executed Node preload.
  requireTrue(process.execArgv.length === 0, 'Node launch options are forbidden');
  module.paths.length = 0;
  Module.globalPaths.length = 0;
  const builtin = new Set(Module.builtinModules.flatMap(name => [name, `node:${name}`]));
  const resolve = Module._resolveFilename;
  Module._resolveFilename = function (name, ...rest) {
    if (builtin.has(name)) return resolve.call(this, name, ...rest);
    const resolved = resolve.call(this, name, ...rest);
    within(installationRoot, resolved, 'module resolution');
    return resolved;
  };
}

function packed(rows, global) {
  const bytes = Buffer.allocUnsafe(rows.length * CLASSES * 4);
  for (let row = 0; row < rows.length; row++) {
    for (let index = 0; index < CLASSES; index++) bytes.writeFloatLE(rows[row][index], (row * CLASSES + index) * 4);
  }
  const means = Buffer.allocUnsafe(CLASSES * 8);
  for (let index = 0; index < CLASSES; index++) means.writeDoubleLE(global.probabilities[index], index * 8);
  return {
    probabilities: { encoding: 'float32-le/base64', shape: [rows.length, CLASSES], sha256: sha256(bytes), data: bytes.toString('base64') },
    global: { encoding: 'float64-le/base64', shape: [CLASSES], sha256: sha256(means), data: means.toString('base64'), first_argmax_class: global.nominal_prior_bpm },
  };
}

function resourceSummary(profiles) {
  let logical = 0;
  let rss = 0;
  let maximumRss = 0;
  for (const item of profiles) {
    logical = Math.max(logical, item.tfjs.peak_logical_bytes);
    rss = Math.max(rss, item.process.rss_bytes);
    maximumRss = Math.max(maximumRss, item.process.process_max_rss_bytes ?? 0);
  }
  return { batch_count: profiles.length, peak_tfjs_logical_bytes: logical, max_sampled_rss_bytes: rss,
    process_max_rss_bytes: maximumRss, held_native_bytes: heldNativeBytes, aggregate_limit_bytes: LIMIT,
    scope: 'Node sampled RSS/maxRSS and TFJS logical tensors; Windows private-commit/retirement proof belongs to the parent observer' };
}

function parseArguments(argv) {
  const result = { smoke: false };
  const names = ['--output', '--tfjs-module', '--model-dir', '--features', '--manifest-sha256', '--batch-size', '--held-native-bytes', '--request-root', '--installation-root'];
  for (let index = 0; index < argv.length; index++) {
    const name = argv[index];
    if (name === '--smoke') {
      requireTrue(!result.smoke, 'Duplicate smoke switch');
      result.smoke = true;
    } else {
      requireTrue(names.includes(name) && index + 1 < argv.length && !argv[index + 1].startsWith('--') && result[name] === undefined, `Unknown, duplicate or missing argument: ${name}`);
      result[name] = argv[++index];
    }
  }
  for (const name of ['--output', '--tfjs-module', '--model-dir', '--held-native-bytes', '--request-root', '--installation-root']) requireTrue(result[name], `Required argument: ${name}`);
  requireTrue(/^(0|[1-9][0-9]*)$/.test(result['--held-native-bytes']), 'Held Native bytes must be a canonical integer');
  heldNativeBytes = Number(result['--held-native-bytes']);
  requireTrue(Number.isSafeInteger(heldNativeBytes) && heldNativeBytes >= 0 && heldNativeBytes <= LIMIT, 'Invalid held Native charge');
  if (result.smoke) requireTrue(!result['--features'] && !result['--manifest-sha256'] && !result['--batch-size'], 'Source inference arguments are invalid for smoke');
  else requireTrue(result['--features'] && digest(result['--manifest-sha256']) && result['--batch-size'] === '2', 'Production inference requires pinned features and fixed batch2');
  return result;
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  requireTrue(os.endianness() === 'LE' && process.version === 'v25.5.0', 'Expected pinned little-endian Node25.5.0');
  const installationRoot = fs.realpathSync(options['--installation-root']);
  const requestRoot = fs.realpathSync(options['--request-root']);
  requireTrue(!isBelow(installationRoot, requestRoot) && !isBelow(requestRoot, installationRoot), 'Request and installation directories must be disjoint');
  requireTrue(fs.realpathSync(BLOCK) === installationRoot, 'Entrypoint must be physically inside the frozen installation root');
  requireTrue(fs.statSync(path.join(BLOCK, 'numeric-profile.json')).size <= 64 * 1024, 'Numeric profile exceeds bound');
  const numericBytes = fs.readFileSync(path.join(BLOCK, 'numeric-profile.json'));
  requireTrue(numericBytes.length <= 64 * 1024, 'Numeric profile exceeds bound');
  const numeric = JSON.parse(numericBytes.toString('utf8'));
  const numericProfileSha256 = sha256(numericBytes);
  requireTrue(numeric.numeric_profile_id === NUMERIC_PROFILE_ID && numeric.reference_inference_runner_sha256 === REFERENCE_RUNNER, 'Numeric profile provenance differs');
  requireTrue(sha256File(process.execPath) === numeric.runtime.node_executable_sha256, 'Node executable differs');
  const modelDirectory = within(installationRoot, options['--model-dir'], 'model directory');
  requireTrue(modelDirectory === fs.realpathSync(path.join(BLOCK, 'upstream', 'deeptemp-k16-3-tfjs')), 'Model directory differs from the physical HERE layout');
  const tfjsModule = within(installationRoot, options['--tfjs-module'], 'TFJS module');
  restrictModules(installationRoot);
  checkMemory(memory());
  const tf = require(tfjsModule);
  requireTrue(tf.version.tfjs === TFJS_VERSION, `Expected private TFJS${TFJS_VERSION}`);
  await tf.setBackend('cpu');
  await tf.ready();
  requireTrue(tf.getBackend() === 'cpu', 'CPU backend was not selected');
  const initial = tf.memory();
  let model = null;
  let result;
  const common = { schema_version: 1, runner_sha256: sha256(fs.readFileSync(__filename)),
    numeric_profile_id: NUMERIC_PROFILE_ID, numeric_profile_sha256: numericProfileSha256,
    reference_inference_runner_sha256: REFERENCE_RUNNER, model_pins: MODEL,
    runtime: { node: process.version, tfjs: TFJS_VERSION, backend: 'cpu' }, execution_origin: 'computed' };
  try {
    model = await loadModel(tf, modelDirectory);
    checkMemory(memory(), tf.memory().numBytes);
    if (options.smoke) {
      const values = syntheticFeatures();
      const one = await predict(tf, model, values, 2, 1);
      const two = await predict(tf, model, values, 2, 2);
      let maximum = 0;
      for (let row = 0; row < 2; row++) {
        for (let index = 0; index < CLASSES; index++) maximum = Math.max(maximum, Math.abs(one.rows[row][index] - two.rows[row][index]));
        requireTrue(firstMaximum(one.rows[row]) === firstMaximum(two.rows[row]), 'Technical batch choice changed a per-patch maximum');
      }
      requireTrue(maximum <= 2e-6 && one.global.class_index === two.global.class_index, 'Batch1/batch2 distribution parity failed');
      result = { ...common, status: 'completed', reason: 'completed', mode: 'technical-smoke',
        synthetic_input_sha256: sha256(Buffer.from(values.buffer)), synthetic_shape: [2, 40, 256, 1],
        max_batch_distribution_difference: maximum, verified_batch_sizes: [1, 2],
        batch1: packed(one.rows, one.global), batch2: packed(two.rows, two.global),
        resources: resourceSummary([...one.profiles, ...two.profiles]) };
    } else {
      const manifestPath = within(requestRoot, options['--features'], 'manifest');
      const manifest = JSON.parse(readPinned(manifestPath, options['--manifest-sha256'], 64 * 1024).toString('utf8'));
      const base = path.dirname(manifestPath);
      const item = validateManifest(manifest, base, requestRoot, numericProfileSha256);
      const bytes = readPinned(within(requestRoot, path.resolve(base, item.features.path), 'features'), item.features.sha256, MAX_WINDOWS * WINDOW * 4, item.features.bytes);
      requireTrue(bytes.byteOffset % 4 === 0, 'Unaligned float32 input buffer');
      const values = new Float32Array(bytes.buffer, bytes.byteOffset, bytes.length / 4);
      for (const value of values) requireTrue(Number.isFinite(value), 'Nonfinite normalized feature');
      const prediction = await predict(tf, model, values, item.shape[0], 2);
      result = { ...common, status: 'completed', reason: 'completed', mode: 'sealed-feature-inference',
        identity: manifest.identity, profile: manifest.profile, input_manifest_sha256: options['--manifest-sha256'],
        frontend_receipt: manifest.frontend_receipt, source_sha256: item.source_sha256, pcm_sha256: item.pcm_sha256,
        feature_sha256: item.features.sha256, shape: item.shape, normalization: item.normalization,
        batch_size: 2, ...packed(prediction.rows, prediction.global), resources: resourceSummary(prediction.profiles) };
    }
  } finally {
    if (model) model.dispose();
  }
  const final = tf.memory();
  requireTrue(final.numTensors === initial.numTensors && final.numBytes === initial.numBytes, 'Model disposal did not restore initial tensor ownership');
  result.resources_released = true;
  result.tensor_disposal = { initial_tensors: initial.numTensors, initial_bytes: initial.numBytes, final_tensors: final.numTensors, final_bytes: final.numBytes };
  result.process_final = memory();
  checkMemory(result.process_final, final.numBytes);
  const output = writeNew(options['--output'], result, requestRoot);
  checkMemory(memory(), final.numBytes);
  process.stdout.write(`${JSON.stringify({ status: result.status, output })}\n`);
}

main().catch(error => {
  process.stderr.write(`${error.stack || error}\n`);
  process.exitCode = 1;
});
