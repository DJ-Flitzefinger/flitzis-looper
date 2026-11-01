//! Prospective ARENA ONLY proof: source events, no weights, NN payload or I/O.
//! This module changes header storage, NEVER the production allocator algorithm.
use super::*;
use flitzis_stem_dsp::{
    CHANNELS, Geometry, MAX_CHUNK_FRAMES, RATE, STEMS, SpectralPlan,
    learned::{BANDS, DEPTH, DIM, FEATURES, Model, band_width, weight_bytes},
    track::{BATCH, CHUNK, RING},
};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
struct SparseHeaders(BTreeMap<usize, Header>);
impl HeaderStore for SparseHeaders {
    unsafe fn read(&self, _: *mut u8, offset: usize) -> Header {
        self.0[&offset]
    }
    unsafe fn write(&mut self, _: *mut u8, offset: usize, header: Header) {
        self.0.insert(offset, header);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct State {
    free: Vec<(usize, usize, usize)>,
    live: usize,
}

struct Recipe {
    pool: Pool<SparseHeaders>,
    reached: usize,
    tensors: usize,
    tensor_bytes: usize,
    metadata: usize,
    // Verifier bookkeeping only; never a retained model payload or predictor.
    owners: BTreeMap<usize, Layout>,
}

impl Recipe {
    fn new(capacity: usize, residue: usize) -> Self {
        WorkspaceAllocator::validate_extent(capacity).unwrap();
        assert!([0, 16].contains(&residue));
        let mut pool = Pool {
            // Integer addresses are NEVER dereferenced by SparseHeaders.
            base: (4096 + residue) as *mut u8,
            first: 0,
            stats: Stats {
                capacity,
                ..Stats::default()
            },
            headers: SparseHeaders::default(),
        };
        unsafe {
            pool.write(
                0,
                Header {
                    len: capacity,
                    link: END,
                },
            )
        };
        Self {
            pool,
            reached: 0,
            tensors: 0,
            tensor_bytes: 0,
            metadata: 0,
            owners: BTreeMap::new(),
        }
    }

    fn state(&self) -> State {
        let mut free = Vec::new();
        let mut offset = self.pool.first;
        let mut previous_end = None;
        while offset != END {
            let header = unsafe { self.pool.read(offset) };
            assert_eq!(offset % ALIGN, 0);
            assert!(header.len >= HEADER + ALIGN);
            assert!(offset + header.len <= self.pool.stats.capacity);
            if let Some(end) = previous_end {
                assert!(end < offset);
            }
            free.push((offset, header.len, header.link));
            previous_end = Some(offset + header.len);
            offset = header.link;
        }
        assert_eq!(
            free.iter().map(|x| x.1).sum::<usize>() + self.pool.stats.live,
            self.pool.stats.capacity
        );
        State {
            free,
            live: self.pool.stats.live,
        }
    }

    fn layout(&mut self, layout: Layout) -> usize {
        let address = unsafe { self.pool.allocate(layout) } as usize;
        assert_ne!(
            address, 0,
            "recipe exhausted at {layout:?}, {:?}",
            self.pool.stats
        );
        assert_eq!(address % layout.align(), 0);
        let used = unsafe { self.pool.read(address - self.pool.base as usize - HEADER) };
        self.reached = self.reached.max(used.link + used.len);
        assert!(self.owners.insert(address, layout).is_none());
        address
    }

    fn a(&mut self, floats: usize) -> usize {
        self.layout(Layout::array::<f32>(floats).unwrap())
    }

    fn d(&mut self, address: usize) {
        self.owners
            .remove(&address)
            .expect("each owner releases exactly once");
        unsafe { self.pool.release(address as *mut u8) };
    }

    fn pulse(&mut self, bytes: usize, alignment: usize) {
        let before = self.state();
        let address = self.layout(Layout::from_size_align(bytes, alignment).unwrap());
        self.d(address);
        assert_eq!(self.state(), before, "packing must restore EXACT free list");
    }

    fn packing(&mut self, m: usize, k: usize, n: usize) {
        // Pinned matrixmultiply0.3.10 default std/nonthreaded: one allocation
        // before loops, no intervening Rust allocation, freed before return.
        // Test EVERY selected-kernel layout as well as the conservative maximum;
        // this does not infer first-fit placement from payload-size ordering.
        for (nr, alignment) in [(8, 32), (4, 16), (4, 4)] {
            let bytes =
                4 * k.min(256) * (m.min(64).div_ceil(8) * 8 + n.min(1024).div_ceil(nr) * nr);
            assert!(bytes <= 1_081_344);
            self.pulse(bytes, alignment);
        }
        self.pulse(1_081_344, 32);
    }

    fn linear(&mut self, rows: usize, input: usize, output: usize) -> usize {
        let result = self.a(rows * output);
        self.packing(rows.min(32), input, output);
        if rows > 32 && !rows.is_multiple_of(32) {
            self.packing(rows % 32, input, output);
        }
        // Every identical full tile starts from EXACTLY this same state.
        result
    }

    fn tensor(&mut self, floats: usize) {
        self.a(floats);
        self.tensors += 1;
        self.tensor_bytes += floats * 4;
    }

    fn meta(&mut self, layout: Layout) {
        self.layout(layout);
        self.metadata += 1;
    }

    fn model(&mut self) {
        let [bands, blocks, stems, masks] = Model::allocation_metadata_layouts();
        self.meta(bands);
        for band in 0..BANDS {
            let w = band_width(band);
            for n in [w, w * DIM, DIM] {
                self.tensor(n);
            }
        }
        self.meta(blocks);
        for _ in 0..DEPTH * 2 {
            for n in [
                32,
                DIM,
                DIM * 1536,
                DIM * 8,
                8,
                512 * DIM,
                DIM,
                DIM * 1536,
                1536,
                1536 * DIM,
                DIM,
            ] {
                self.tensor(n);
            }
        }
        self.tensor(DIM);
        self.meta(stems);
        for _ in 0..STEMS {
            self.meta(masks);
            for band in 0..BANDS {
                let w = band_width(band);
                for n in [DIM * 768, 768, 768 * 2 * w, 2 * w] {
                    self.tensor(n);
                }
            }
        }
        assert_eq!(self.tensors, 1355);
        assert_eq!(self.metadata, 7);
        assert_eq!(self.tensor_bytes, weight_bytes() - 8);
    }

    fn transformer(&mut self, n: usize) {
        let before = self.state();
        let norm = self.a(n * DIM);
        let qkv = self.linear(n, DIM, 1536);
        let gates = self.linear(n, DIM, 8);
        self.d(norm);
        let attended = self.a(n * 512);
        let q = self.a(n * 64);
        let k = self.a(n * 64);
        let v = self.a(n * 64);
        let scores = self.a(16 * n);
        let tile = self.a(16 * 64);
        self.packing(n.min(16), 64, n);
        self.packing(n.min(16), n, 64);
        if n > 16 && !n.is_multiple_of(16) {
            self.packing(n % 16, 64, n);
            self.packing(n % 16, n, 64);
        }
        // Eight heads and all query tiles reuse these same owners. Each pulse
        // above restores the exact list; score/softmax/value math allocates none.
        for address in [tile, scores, v, k, q, qkv, gates] {
            self.d(address);
        }
        let attention = self.linear(n, 512, DIM);
        self.d(attended);
        self.d(attention);
        let norm = self.a(n * DIM);
        let expanded = self.linear(n, DIM, 1536);
        self.d(norm);
        let contracted = self.linear(n, 1536, DIM);
        self.d(expanded);
        self.d(contracted);
        assert_eq!(self.state(), before, "transformer restores its caller");
    }

    fn latent(&mut self, s: usize) -> usize {
        let x = self.a(s * BANDS * DIM);
        for band in 0..BANDS {
            let before = self.state();
            let w = band_width(band);
            let selected = self.a(s * w);
            let norm = self.a(s * w);
            self.d(selected);
            let projected = self.linear(s, w, DIM);
            self.d(norm);
            self.d(projected);
            assert_eq!(self.state(), before, "latent band restores X");
        }
        for _ in 0..DEPTH {
            for _ in 0..BANDS {
                let before = self.state();
                let selected = self.a(s * DIM);
                self.transformer(s);
                self.d(selected);
                assert_eq!(self.state(), before, "time selection restores X");
            }
            for _ in 0..s {
                self.transformer(BANDS);
            }
        }
        x // final Norm::apply_in_place has no allocation
    }

    fn project(&mut self, s: usize, count: usize) {
        let caller = self.state();
        for band in 0..BANDS {
            let selected = self.a(s * DIM);
            for _ in 0..count {
                let hidden = self.linear(s, DIM, 768);
                let projected = self.linear(s, 768, 2 * band_width(band));
                self.d(hidden);
                self.d(projected);
            }
            self.d(selected);
            assert_eq!(self.state(), caller, "mask band restores caller");
        }
    }

    fn all_masks(&mut self, s: usize) -> usize {
        let x = self.latent(s);
        let masks = self.a(STEMS * s * FEATURES);
        self.project(s, STEMS);
        self.d(x);
        masks
    }

    fn diagnostic(&mut self, frames: usize, original: bool) {
        let caller = self.state();
        let s = Geometry::new(frames, RATE).unwrap().steps();
        let pcm = self.a(CHANNELS * frames);
        let features = self.a(s * FEATURES);
        self.d(pcm);
        let masks = self.all_masks(s);
        let prediction = self.a(STEMS * CHANNELS * frames);
        // Working vectors remain in the persistent prefix. Plans/windows are
        // PRESEAL System owners, never erased from complete process accounting.
        for address in [prediction, masks, features] {
            self.d(address);
        }
        assert_eq!(self.state(), caller);
        if original {
            let features = self.a(s * FEATURES);
            let masks = self.all_masks(s);
            self.d(masks);
            self.d(features);
            assert_eq!(self.state(), caller, "optional forward restores model");
        }
    }

    fn sample(&mut self) {
        let caller = self.state();
        let s = Geometry::new(CHUNK, RATE).unwrap().steps();
        let features = self.a(s * FEATURES);
        let x = self.latent(s);
        let mask = self.a(s * FEATURES);
        for _ in 0..STEMS {
            self.project(s, 1);
        }
        for address in [mask, x, features] {
            self.d(address);
        }
        assert_eq!(
            self.state(),
            caller,
            "sample restores full batch destination"
        );
    }

    fn track(&mut self) {
        let model = self.state();
        let persistent: Vec<_> = [
            BATCH * CHANNELS * CHUNK,
            CHANNELS * CHUNK,
            STEMS * CHANNELS * RING,
            RING,
            STEMS * CHANNELS * CHUNK,
            CHUNK,
        ]
        .into_iter()
        .map(|n| self.a(n))
        .collect();
        let baseline = self.state();
        for batch in [1, 2, 2, 1] {
            let destination = self.a(batch * STEMS * CHANNELS * CHUNK);
            for _ in 0..batch {
                self.sample();
            }
            // Earlier samples are already INSIDE destination. Prediction dies
            // before flush; no per-track-length allocation can accumulate.
            self.d(destination);
            assert_eq!(
                self.state(),
                baseline,
                "B1/B2 returns exact persistent baseline"
            );
        }
        for address in persistent {
            self.d(address);
        }
        assert_eq!(self.state(), model);
    }

    fn spectral(&mut self, layouts: &[Layout; 6]) {
        for &layout in layouts {
            // Vec::try_reserve_exact(0) does not call the allocator.
            if layout.size() != 0 {
                self.layout(layout);
            }
        }
    }

    fn run(&mut self, layouts: &[Layout; 6]) {
        self.spectral(layouts);
        self.model();
        self.diagnostic(MAX_CHUNK_FRAMES, true);
        self.track();
        // Terminal success/error/cancel cleanup cannot create another NN owner.
        // Free in address order to exercise forward coalescing (not only LIFO).
        let owners: Vec<_> = self.owners.keys().copied().collect();
        for address in owners {
            self.d(address);
        }
        assert_eq!(self.state().free, [(0, self.pool.stats.capacity, END)]);
        assert_eq!(self.pool.stats.live, 0);
        assert_eq!(self.pool.stats.failures, 0);
    }
}

#[test]
fn complete_original_source_recipe_uses_actual_allocator_and_exact_extent() {
    assert_eq!(HEADER, 16);
    assert_eq!(ALIGN, 16);
    assert_eq!(Geometry::new(CHUNK, RATE).unwrap().steps(), 1101);
    // These are the actual selected retained plans, not a guessed FFT branch,
    // trace of model requests or a full-process/native backing claim.
    let plan = SpectralPlan::new(CHUNK, RATE, &|| false).unwrap();
    let layouts = plan.allocation_layouts();
    // A virtual Layout-valid tail derives ADDRESS high water, never max-live
    // arithmetic, RAM headroom, guessed MiB margin or binary monotonic search.
    let discovery = (isize::MAX as usize - 8192) & !(ALIGN - 1);
    let mut extent = 0;
    for residue in [0, 16] {
        let mut recipe = Recipe::new(discovery, residue);
        recipe.run(&layouts);
        extent = extent.max(recipe.reached);
    }
    // Keep the algorithm's exact minimum splittable tail. This structural32B
    // also avoids changing discovery placement by absorbing a final tiny tail.
    extent += HEADER + ALIGN;
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    if std::is_x86_feature_detected!("avx") && std::is_x86_feature_detected!("fma") {
        assert_eq!(
            extent, 768_857_616,
            "reconcile the recorded Windows x64 AVX/FMA recipe receipt"
        );
    }
    for residue in [0, 16] {
        let mut recipe = Recipe::new(extent, residue);
        recipe.run(&layouts); // EXPLICIT replay; no capacity monotonicity assumption
        println!(
            "arena-only extent={extent} residue={residue} highwater={} peak={} requests={} metadata={:?} spectral={layouts:?}",
            recipe.reached,
            recipe.pool.stats.peak,
            recipe.pool.stats.allocations,
            Model::allocation_metadata_layouts()
        );
    }
}

#[test]
fn complete_spectral_fixture_restores_the_persistent_working_prefix() {
    let plan = SpectralPlan::new(4099, RATE, &|| false).unwrap();
    let layouts = plan.allocation_layouts();
    let capacity = crate::spectral_fixture_capacity(&layouts).unwrap();
    for residue in [0, 16] {
        let mut recipe = Recipe::new(capacity, residue);
        recipe.spectral(&layouts);
        let prefix = recipe.state();
        for frames in [4099, 1025, 2501, 4099, 1025] {
            let geometry = Geometry::new(frames, RATE).unwrap();
            let features = recipe.a(geometry.feature_len());
            let masks = recipe.a(geometry.mask_len());
            let output = recipe.a(STEMS * CHANNELS * frames);
            for owner in [output, masks, features] {
                recipe.d(owner);
            }
            assert_eq!(recipe.state(), prefix);
        }
        println!(
            "spectral-fixture extent={capacity} residue={residue} highwater={} layouts={layouts:?}",
            recipe.reached
        );
        let owners: Vec<_> = recipe.owners.keys().copied().collect();
        for owner in owners {
            recipe.d(owner);
        }
        assert_eq!(recipe.state().free, [(0, capacity, END)]);
        assert_eq!(recipe.pool.stats.failures, 0);
    }
}

#[test]
fn sparse_and_native_storage_match_absolute_alignment_and_tinytails() {
    // SAME algorithm with real bytes vs sparse headers. Actual GlobalAlloc
    // realloc old+new/data/failure controls remain in workspace_tests.rs.
    for residue in [0, 16] {
        let backing = Layout::from_size_align(8192 + 32, 128).unwrap();
        let raw = unsafe { System.alloc_zeroed(backing) };
        assert!(!raw.is_null());
        let base = unsafe { raw.add(residue) };
        let mut native = Pool {
            base,
            first: 0,
            stats: Stats {
                capacity: 8192,
                ..Stats::default()
            },
            headers: NativeHeaders,
        };
        unsafe {
            native.write(
                0,
                Header {
                    len: 8192,
                    link: END,
                },
            )
        };
        let mut sparse = Recipe::new(8192, residue);
        let mut values = Vec::new();
        for (size, align) in [(17, 4), (31, 32), (7, 64), (1000, 16), (33, 32), (0, 1)] {
            let layout = Layout::from_size_align(size, align).unwrap();
            let real = unsafe { native.allocate(layout) };
            let fake = sparse.layout(layout);
            assert_eq!(
                real as usize - base as usize,
                fake - sparse.pool.base as usize
            );
            values.push((real, fake));
        }
        for index in [1, 3, 0, 5, 2, 4] {
            let (real, fake) = values[index];
            unsafe { native.release(real) };
            sparse.d(fake);
            assert_eq!(native.first, sparse.pool.first);
            assert_eq!(native.stats, sparse.pool.stats);
            let mut at = native.first;
            while at != END {
                let a = unsafe { native.read(at) };
                let b = unsafe { sparse.pool.read(at) };
                assert_eq!((a.len, a.link), (b.len, b.link));
                at = a.link;
            }
        }
        // Absorb a16B tail in BOTH stores; failed request retains the old owner.
        let layout = Layout::from_size_align(8192 - 32, 16).unwrap();
        let real = unsafe { native.allocate(layout) };
        let old = sparse.layout(layout);
        assert_eq!(
            real as usize - base as usize,
            old - sparse.pool.base as usize
        );
        assert_eq!(sparse.pool.stats.live, 8192);
        assert_eq!(native.stats, sparse.pool.stats);
        assert!(unsafe { native.allocate(Layout::from_size_align(64, 16).unwrap()) }.is_null());
        assert!(
            unsafe {
                sparse
                    .pool
                    .allocate(Layout::from_size_align(64, 16).unwrap())
            }
            .is_null()
        );
        sparse.d(old);
        unsafe { native.release(real) };
        assert_eq!(native.stats, sparse.pool.stats);
        let recovered = unsafe { native.read(native.first) };
        assert_eq!(
            (native.first, recovered.len, recovered.link),
            (0, 8192, END)
        );
        assert_eq!(sparse.state().free, [(0, 8192, END)]);
        unsafe { System.dealloc(raw, backing) };
    }
}

#[test]
fn failed_allocation_preserves_each_attention_prefix_and_cleanup_restores_model() {
    // Exercise every allocation prefix of the largest attention-owner episode,
    // including the norm release. No learned arithmetic or host failure induced.
    let s = Geometry::new(CHUNK, RATE).unwrap().steps();
    let plan = SpectralPlan::new(CHUNK, RATE, &|| false).unwrap();
    let layouts = plan.allocation_layouts();
    for residue in [0, 16] {
        for prefix in 0..=12 {
            let mut recipe = Recipe::new(1 << 30, residue);
            recipe.spectral(&layouts);
            recipe.model();
            let model = recipe.state();
            let mut transient = Vec::new();
            for (index, n) in [
                s * FEATURES,
                s * BANDS * DIM,
                s * DIM,
                s * DIM,
                s * 1536,
                s * 8,
                s * 512,
                s * 64,
                s * 64,
                s * 64,
                16 * s,
                16 * 64,
            ]
            .into_iter()
            .take(prefix)
            .enumerate()
            {
                if index == 6 {
                    recipe.d(transient[3]);
                }
                transient.push(recipe.a(n));
            }
            let before = recipe.state();
            let impossible = Layout::from_size_align(1 << 30, 32).unwrap();
            assert!(unsafe { recipe.pool.allocate(impossible) }.is_null());
            assert_eq!(recipe.state(), before, "failure preserves all old owners");
            for address in transient {
                if recipe.owners.contains_key(&address) {
                    recipe.d(address);
                }
            }
            assert_eq!(
                recipe.state(),
                model,
                "terminal prefix cleanup restores model"
            );
        }
    }
}
