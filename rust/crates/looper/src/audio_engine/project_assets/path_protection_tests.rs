use super::*;

fn old_predicate(paths: &[PathBuf], target: &Path) -> bool {
    paths
        .iter()
        .any(|path| path.starts_with(target) || target.starts_with(path))
}

fn path_shapes() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = [
        "",
        ".",
        "./",
        "samples",
        "samples/.",
        "samples//materials/M0123456789abcdef0123456789abcdef/original/Take.wav",
        "samples/materials/M0123456789abcdef0123456789abcdef",
        "samples/materials/M0123456789abcdef0123456789abcdef/original",
        "samples/materials/M0123456789abcdef0123456789abcdef/original/Take.wav",
        "samples/materials/M0123456789abcdef0123456789abcdef/original/Take.wav/child",
        "samples/materials/M0123456789abcdef0123456789abcdef/original/Take.wav.other",
        "samples/materials/M0123456789abcdef0123456789abcdef/original/Other.wav",
        "samples/materials/M0123456789abcdef0123456789abcdef/.pcm-cache/v1/generation-1",
        "samples/materials/M0123456789abcdef0123456789abcdef/stems/.ready-1",
        "samples/materials/M0123456789abcdef0123456789abcdef/stems/.pcm-generation-1",
        "samples/materials/M0123456789abcdef0123456789abcdef/stems/.pair-generation-1.json",
        "samples/materials/M0123456789abcdef0123456789abcdef-other",
        "samples/materials/Mffffffffffffffffffffffffffffffff",
        "samples/stems/#1/.ready-1",
        "samples/stems/#10/.ready-1",
        "samples/materials/../original",
        "samples/./materials",
        "samples/materials/./M0123456789abcdef0123456789abcdef/original/Take.wav/",
        "SAMPLES/materials",
        "samples/materials/Äudio/片段.wav",
        "/",
        "/samples",
        "/samples/materials",
        "/samples-other",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    // These are interpreted by native Windows Path components in the supported
    // product environment, including prefixes which are not filesystem parents.
    paths.extend(
        [
            r"C:",
            r"C:\",
            r"C:\samples",
            r"C:\samples\materials\M1\original\Take.wav",
            r"C:\samples\materials\M1\original\Take.wav.other",
            r"C:\samples\materials\M10\original\Take.wav",
            r"C:samples",
            r"C:samples\materials",
            r"c:\samples",
            r"D:",
            r"D:\",
            r"\samples",
            r"\\server\share",
            r"\\server\share\",
            r"\\server\share\samples",
            r"\\server\share\samples\original\Take.wav",
            r"\\server\share-other\samples",
            r"\\SERVER\share\samples",
            r"\\?\C:",
            r"\\?\C:\",
            r"\\?\C:\samples",
            r"\\?\C:\samples\.\original\Take.wav",
            r"\\?\UNC\server\share",
            r"\\?\UNC\server\share\",
            r"\\?\UNC\server\share\samples",
            r"\\.\C:\samples",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    paths
}

#[test]
fn every_native_path_shape_matches_independent_symmetric_starts_with() {
    let paths = path_shapes();
    for held in &paths {
        let mut index = PathProtectionIndex::default();
        index.insert(held);
        for target in &paths {
            assert_eq!(
                index.intersects(target),
                held.starts_with(target) || target.starts_with(held),
                "held={held:?}, target={target:?}"
            );
        }
        index.remove(held);
        assert!(index.paths.is_empty());
        assert!(index.prefixes.is_empty());
        for target in &paths {
            assert!(!index.intersects(target));
        }
    }
}

#[test]
fn duplicate_spellings_and_shared_prefixes_survive_independent_removal() {
    let paths = path_shapes();
    let mut index = PathProtectionIndex::default();
    let mut held = Vec::new();
    for path in &paths {
        index.insert(path);
        index.insert(path);
        if !held.contains(path) {
            held.push(path.clone());
        }
    }
    for path in paths.iter().rev() {
        index.remove(path);
        held.retain(|item| item != path);
        for target in &paths {
            assert_eq!(index.intersects(target), old_predicate(&held, target));
        }
        // A second removal must not subtract another path's shared prefixes.
        index.remove(path);
    }
    assert!(index.paths.is_empty());
    assert!(index.prefixes.is_empty());

    let original = Path::new("samples/materials/M1/original/Take.wav");
    let equivalent = Path::new("samples//materials/./M1/original/Take.wav/");
    assert_eq!(original, equivalent);
    index.insert(original);
    index.remove(equivalent);
    assert!(index.paths.is_empty());
    assert!(index.prefixes.is_empty());

    let shared = Path::new("samples/materials/M1");
    let first = shared.join("original/Take.wav");
    let second = shared.join("stems/.ready-1");
    index.insert(&first);
    index.insert(&second);
    index.remove(&first);
    assert!(index.intersects(shared));
    assert!(!index.intersects(&first));
    assert!(index.intersects(&second));
    index.remove(&second);
    assert!(!index.intersects(shared));
    assert!(index.paths.is_empty());
    assert!(index.prefixes.is_empty());
}

fn seeded_next(seed: &mut u64) -> usize {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 32) as usize
}

#[test]
fn seeded_insert_remove_and_container_queries_match_old_protection() {
    let mut paths = path_shapes();
    for material in 0..24 {
        for generation in 0..4 {
            let base = PathBuf::from(format!("samples/materials/M{material:032x}"));
            paths.extend([
                base.join(format!("original/Take-{generation}.wav")),
                base.join(format!(".pcm-cache/v1/generation-{generation}")),
                base.join(format!("stems/.ready-{generation:032x}")),
                base.join(format!("stems/.pcm-generation-{generation:032x}")),
                base.join(format!("stems/.pair-generation-{generation:032x}.json")),
            ]);
        }
    }
    let mut seed = 0x47c6_520d_81ab_9023;
    let mut index = PathProtectionIndex::default();
    let mut held = Vec::new();
    for _ in 0..512 {
        let path = &paths[seeded_next(&mut seed) % paths.len()];
        if seeded_next(&mut seed) & 1 == 0 {
            index.insert(path);
            if !held.contains(path) {
                held.push(path.clone());
            }
        } else {
            index.remove(path);
            held.retain(|item| item != path);
        }
        for _ in 0..16 {
            let target = &paths[seeded_next(&mut seed) % paths.len()];
            assert_eq!(index.intersects(target), old_predicate(&held, target));
            for container in target.ancestors().take(3) {
                assert_eq!(index.intersects(container), old_predicate(&held, container));
            }
        }
    }
}

#[test]
fn saturation_shape_queries_have_component_bounded_work() {
    let root = PathBuf::from("D:/Flitzi/samples");
    // Same source/WAV/PCM/common shape as Reader's six-path paired predicate.
    let mut held = Vec::new();
    for material in 0..373 {
        let base = root.join(format!("materials/M{material:032x}"));
        let wav = base.join("stems/.ready-0123456789abcdef0123456789abcdef");
        held.extend([
            wav.clone(),
            wav,
            base.join("stems/.pcm-0123456789abcdef0123456789abcdef"),
            base.join("stems/.pair-0123456789abcdef0123456789abcdef.json"),
            base.join("original/Take.wav"),
            base.join(".pcm-cache/v1/generation-0123456789abcdef0123456789abcdef"),
        ]);
    }
    let mut index = PathProtectionIndex::default();
    for path in &held {
        index.insert(path);
    }
    let mut old_path_checks = 0;
    let mut indexed_probes = 0;
    let mut total_probe_bound = 0;
    for job in 0..MAX_RETIREMENTS {
        let target = root.join(format!("held-job-{job}.wav"));
        let old = held.iter().any(|path| {
            old_path_checks += 1;
            path.starts_with(&target) || target.starts_with(path)
        });
        let mut probes = 0;
        let indexed = index.intersects_observed(&target, || probes += 1);
        assert_eq!(indexed, old);
        assert!(!indexed);
        let bound = target.components().count() + 2;
        assert!(probes <= bound);
        indexed_probes += probes;
        total_probe_bound += bound;
    }
    assert_eq!(old_path_checks, MAX_RETIREMENTS * 373 * 6);
    assert!(indexed_probes <= total_probe_bound);
    assert!(indexed_probes < old_path_checks);
    let key_bytes: usize = index
        .paths
        .iter()
        .chain(index.prefixes.keys())
        .map(|path| path.as_os_str().as_encoded_bytes().len())
        .sum();
    eprintln!(
        "path-protection-work-shape targets={} reader_shapes=373 old_path_checks={old_path_checks} indexed_probes={indexed_probes} exact_keys={} prefix_keys={} encoded_key_bytes={key_bytes} (excludes hash-table/allocator overhead and process RSS)",
        MAX_RETIREMENTS,
        index.paths.len(),
        index.prefixes.len(),
    );
}
