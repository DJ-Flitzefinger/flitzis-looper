//! Fresh-process ownership attribution for the six frozen Native stderr profiles.
//! The orchestrator never constructs Native state; each child owns exactly one role.

use super::*;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const ROOT_ENV: &str = "LOOPER_NATIVE_DIAGNOSTIC_ROOT";
const ROLE_ENV: &str = "LOOPER_NATIVE_DIAGNOSTIC_ROLE";
const PROFILE_ENV: &str = "LOOPER_NATIVE_DIAGNOSTIC_PROFILE";
const CHILD_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Clone, Copy, Debug)]
enum Role {
    Dut,
    RawA,
    RawO,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Self::Dut => "DUT",
            Self::RawA => "RawA",
            Self::RawO => "RawO",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "DUT" => Self::Dut,
            "RawA" => Self::RawA,
            "RawO" => Self::RawO,
            _ => panic!("undeclared Native diagnostic role: {value}"),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RoleMetadata {
    profile: usize,
    role: String,
    pid: u32,
    output_origin: u64,
    origin: usize,
    frames: usize,
    history_bound: Option<usize>,
    initial_native_pitch_bits: Option<u64>,
    fifos: [(usize, usize); FORMAT_CHANNELS],
    source_sha256: String,
    source_resident_start: usize,
    source_frames: usize,
    dry_sha256: String,
    timeline_sha256: String,
    pcm_sha256: String,
}

#[derive(serde::Serialize)]
struct ChildReport {
    profile: usize,
    role: String,
    pid: u32,
    success: bool,
    exit_code: Option<i32>,
    timed_out: bool,
    elapsed_ms: u128,
    stdout_bytes: usize,
    stderr_bytes: usize,
    insufficient_output_messages: usize,
    insufficient_output_512_511_messages: usize,
    matching_stdout_lines: Vec<String>,
    matching_stderr_lines: Vec<String>,
}

pub(super) fn run() {
    let root = artifact_root();
    match std::env::var(ROLE_ENV) {
        Ok(role) => {
            let profile = std::env::var(PROFILE_ENV)
                .expect("a child role requires its frozen profile index")
                .parse::<usize>()
                .expect("Native diagnostic profile must be an integer");
            assert!(profile < diagnostic_profiles().len());
            run_child_role(&root, profile, Role::parse(&role));
        }
        Err(std::env::VarError::NotPresent) => {
            assert!(std::env::var_os(PROFILE_ENV).is_none());
            run_parent(&root);
        }
        Err(error) => panic!("invalid Native diagnostic role environment: {error}"),
    }
}

fn artifact_root() -> PathBuf {
    let root =
        PathBuf::from(std::env::var_os(ROOT_ENV).expect(
            "LOOPER_NATIVE_DIAGNOSTIC_ROOT must name the verified owned artifact directory",
        ));
    assert!(
        root.is_absolute(),
        "diagnostic artifact root must be absolute"
    );
    assert_plain_directory(&root);
    root
}

fn assert_plain_directory(path: &Path) {
    let metadata = fs::symlink_metadata(path).expect("diagnostic directory must already exist");
    assert!(metadata.is_dir(), "diagnostic path must be a directory");
    assert!(!metadata.file_type().is_symlink());
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        assert_eq!(
            metadata.file_attributes() & 0x400,
            0,
            "diagnostic directories must not be reparse points"
        );
    }
}

fn profile_directory(root: &Path, profile: usize) -> PathBuf {
    root.join(format!("profile-{profile:02}"))
}

fn role_path(directory: &Path, role: Role, suffix: &str) -> PathBuf {
    directory.join(format!("{}.{}", role.name(), suffix))
}

fn frozen_profile(profile: usize) -> (PitchIntent, Attack, usize) {
    let (positive, signal, offset) = diagnostic_profiles()[profile];
    let pitch = if positive {
        PitchIntent::new(6, 18).unwrap()
    } else {
        PitchIntent::new(-5, -18).unwrap()
    };
    (pitch, signal, offset)
}

fn run_parent(root: &Path) {
    let mut reports = Vec::new();
    for profile in 0..diagnostic_profiles().len() {
        let directory = profile_directory(root, profile);
        fs::create_dir(&directory).expect("each diagnostic run requires fresh profile directories");
        for role in [Role::Dut, Role::RawA, Role::RawO] {
            let report = launch_child(root, &directory, profile, role);
            let success = report.success;
            reports.push(report);
            // Preserve every completed child's raw evidence before failing the gate.
            assert!(
                success,
                "isolated {} child failed for profile {profile}",
                role.name()
            );
        }
    }
    write_json(&root.join("child-processes.json"), &reports);
    // All subprocesses are terminal before any cross-role comparison or metrics.
    let baseline: serde_json::Value = read_json(&role_path(
        &profile_directory(root, 0),
        Role::Dut,
        "identity.json",
    ));
    for profile in 0..diagnostic_profiles().len() {
        compare_profile(&profile_directory(root, profile), profile, &baseline);
    }
    println!(
        "native_role_isolation profiles={} child_processes={} criterion failures remain diagnostic; no acoustic acceptance or product fix inferred",
        diagnostic_profiles().len(),
        reports.len()
    );
}

fn launch_child(root: &Path, directory: &Path, profile: usize, role: Role) -> ChildReport {
    let stdout_path = role_path(directory, role, "stdout.log");
    let stderr_path = role_path(directory, role, "stderr.log");
    let started = Instant::now();
    // module_path! includes the crate name; libtest's exact names omit it.
    let exact_test = DIAGNOSTIC_TEST.split_once("::").unwrap().1;
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            exact_test,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROOT_ENV, root)
        .env(ROLE_ENV, role.name())
        .env(PROFILE_ENV, profile.to_string())
        .stdout(Stdio::from(create_new(&stdout_path)))
        .stderr(Stdio::from(create_new(&stderr_path)))
        .spawn()
        .expect("start the same test executable with one isolated Native role");
    let pid = child.id();
    let (status, timed_out): (ExitStatus, bool) = loop {
        if let Some(status) = child.try_wait().expect("query owned child status") {
            break (status, false);
        }
        if started.elapsed() >= CHILD_TIMEOUT {
            // This is exclusively the process just spawned above, never the Human App.
            if let Err(error) = child.kill() {
                if let Some(status) = child.try_wait().expect("query child after timeout race") {
                    break (status, true);
                }
                panic!("could not terminate timed-out owned diagnostic child: {error}");
            }
            break (child.wait().expect("reap timed-out owned child"), true);
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let stdout = fs::read(&stdout_path).unwrap();
    let stderr = fs::read(&stderr_path).unwrap();
    let matching_stdout_lines = insufficient_output_lines(&stdout);
    let matching_stderr_lines = insufficient_output_lines(&stderr);
    let report = ChildReport {
        profile,
        role: role.name().to_owned(),
        pid,
        success: status.success() && !timed_out,
        exit_code: status.code(),
        timed_out,
        elapsed_ms: started.elapsed().as_millis(),
        stdout_bytes: stdout.len(),
        stderr_bytes: stderr.len(),
        insufficient_output_messages: matching_stdout_lines.len() + matching_stderr_lines.len(),
        insufficient_output_512_511_messages: matching_stdout_lines
            .iter()
            .chain(&matching_stderr_lines)
            .filter(|line| contains_frame_pair(line))
            .count(),
        matching_stdout_lines,
        matching_stderr_lines,
    };
    write_json(&role_path(directory, role, "process.json"), &report);
    println!(
        "NATIVE_ISOLATED_PROCESS {}",
        serde_json::to_string(&report).unwrap()
    );
    report
}

fn insufficient_output_lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("insufficient") && lower.contains("output")
        })
        .map(str::to_owned)
        .collect()
}

fn contains_frame_pair(line: &str) -> bool {
    let numbers = line
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    numbers.contains(&"512") && numbers.contains(&"511")
}

fn run_child_role(root: &Path, profile: usize, role: Role) {
    let directory = profile_directory(root, profile);
    assert_plain_directory(&directory);
    let (pitch, signal, offset) = frozen_profile(profile);
    println!(
        "NATIVE_ISOLATED_CHILD role={} profile={profile} pid={} Lock=true base={} extra={} total={} attack={signal:?} offset={offset} RATE={RATE} channels={FORMAT_CHANNELS} options=0x10000000 block={FORMAT_BLOCK} F={F} E={E} capture=[{CAPTURE_START},{CAPTURE_END})",
        role.name(),
        std::process::id(),
        pitch.base_shift(),
        pitch.extra_shift(),
        pitch.total_semitones(),
    );
    // Exported-address module identity is observed before this child creates DSP state.
    let identity =
        crate::audio_engine::pitch_runtime_tests::native_identity_tests::identify_loaded_native()
            .expect("identify the DLL actually backing the imported Native exports");
    println!(
        "NATIVE_CHILD_IDENTITY {}",
        serde_json::to_string(&identity).unwrap()
    );
    assert_eq!(identity["process_id"], std::process::id());
    write_json(&role_path(&directory, role, "identity.json"), &identity);
    match role {
        Role::Dut => capture_dut(&directory, profile, pitch, signal, offset),
        Role::RawA | Role::RawO => capture_reference(&directory, profile, role, pitch),
    }
}

fn capture_dut(
    directory: &Path,
    profile: usize,
    pitch: PitchIntent,
    signal: Attack,
    offset: usize,
) {
    let DutCapture {
        fixture,
        source,
        stems,
        output_origin,
        actual,
        timeline,
    } = capture_dut_with_trace(true, pitch, Some((signal, offset)), true);
    // This is the unchanged independent scalar oracle input from the real source.
    // No raw Native instance, reference constructor or history-bound probe runs here.
    let mut dry: [Vec<f32>; FORMAT_CHANNELS] =
        std::array::from_fn(|_| Vec::with_capacity(CAPTURE_END));
    for active in 0..CAPTURE_END {
        let input = format_selected_pcm(&timeline, active, &source, &stems, &[], START, END);
        assert!(input.into_iter().all(f32::is_finite));
        for channel in 0..FORMAT_CHANNELS {
            dry[channel].push(input[channel]);
        }
    }
    let actual = planar(&actual);
    assert_eq!(actual[0].len(), CAPTURE_END);
    let pcm_sha256 = write_pcm(&role_path(directory, Role::Dut, "pcm.f32le"), &actual);
    let dry_sha256 = write_pcm(&directory.join("dry.f32le"), &dry);
    let source_bytes = source
        .samples
        .iter()
        .flat_map(|sample| sample.to_bits().to_le_bytes())
        .collect::<Vec<_>>();
    let metadata = RoleMetadata {
        profile,
        role: Role::Dut.name().to_owned(),
        pid: std::process::id(),
        output_origin,
        origin: 0,
        frames: CAPTURE_END,
        history_bound: None,
        initial_native_pitch_bits: None,
        fifos: [fixture.voice(0).stretch.pending_fifo_frames(); FORMAT_CHANNELS],
        source_sha256: digest(&source_bytes),
        source_resident_start: source.resident_start(),
        source_frames: source.frame_count(),
        dry_sha256,
        timeline_sha256: timeline_digest(&timeline),
        pcm_sha256,
    };
    write_json(&role_path(directory, Role::Dut, "metadata.json"), &metadata);
    println!(
        "NATIVE_ROLE_METADATA {}",
        serde_json::to_string(&metadata).unwrap()
    );
    eprintln!(
        "NATIVE_ROLE_PHASE DUT payload_written; remaining Native activity belongs only to DUT"
    );
    drop(fixture);
    eprintln!("NATIVE_ROLE_PHASE DUT fixture_dropped; process exit establishes final containment");
}

fn capture_reference(directory: &Path, profile: usize, role: Role, pitch: PitchIntent) {
    let dut: RoleMetadata = read_json(&role_path(directory, Role::Dut, "metadata.json"));
    assert_eq!(dut.profile, profile);
    assert_eq!(dut.role, Role::Dut.name());
    let (dry, dry_sha256) = read_pcm(&directory.join("dry.f32le"));
    assert_eq!(dry[0].len(), CAPTURE_END);
    assert_eq!(dry_sha256, dut.dry_sha256);
    let timeline = format_chronology(START, END, &EVENTS, CAPTURE_END);
    assert_eq!(timeline_digest(&timeline), dut.timeline_sha256);
    assert_eq!(timeline[F].rate.to_bits(), 0.9200000000000003_f64.to_bits());
    assert_eq!(timeline[E].rate.to_bits(), 1.29_f64.to_bits());
    assert_eq!(timeline[E].frame, MARKER_BASE);
    let (origin, history_bound) = match role {
        Role::RawA => {
            eprintln!("NATIVE_ROLE_PHASE RawA bound_queries BEGIN");
            let (origin, bound) = format_reset_origin(&timeline, E, RATE, pitch, true);
            eprintln!("NATIVE_ROLE_PHASE RawA bound_queries END A={origin} H={bound}");
            (origin, Some(bound))
        }
        Role::RawO => (0, None),
        Role::Dut => unreachable!(),
    };
    // A and O each keep their own first completed O-relative block's cold pitch.
    let initial_pitch = format_native_pitch(timeline[origin + FORMAT_BLOCK - 1], pitch, true);
    eprintln!(
        "NATIVE_ROLE_PHASE {} constructor BEGIN origin={origin} first_pitch={initial_pitch:.17}",
        role.name()
    );
    let mut raw = StereoRawNative::new(RATE, initial_pitch);
    eprintln!(
        "NATIVE_ROLE_PHASE {} constructor END; replay BEGIN",
        role.name()
    );
    let mut output: [Vec<f32>; FORMAT_CHANNELS] =
        std::array::from_fn(|_| Vec::with_capacity(CAPTURE_END - origin));
    for active in origin..CAPTURE_END {
        let input = std::array::from_fn(|channel| dry[channel][active]);
        let native = format_native_pitch(timeline[active], pitch, true);
        if active % FORMAT_BLOCK == FORMAT_BLOCK - 1 {
            eprintln!(
                "NATIVE_CALL {} active={active} pitch={native:.17}",
                role.name()
            );
        }
        let frame = raw.frame(input, native);
        assert!(frame.into_iter().all(f32::is_finite));
        for channel in 0..FORMAT_CHANNELS {
            output[channel].push(frame[channel]);
        }
    }
    eprintln!("NATIVE_ROLE_PHASE {} replay END", role.name());
    let pcm_sha256 = write_pcm(&role_path(directory, role, "pcm.f32le"), &output);
    let metadata = RoleMetadata {
        profile,
        role: role.name().to_owned(),
        pid: std::process::id(),
        output_origin: dut.output_origin,
        origin,
        frames: CAPTURE_END - origin,
        history_bound,
        initial_native_pitch_bits: Some(initial_pitch.to_bits()),
        fifos: raw.fifos(),
        source_sha256: dut.source_sha256,
        source_resident_start: dut.source_resident_start,
        source_frames: dut.source_frames,
        dry_sha256,
        timeline_sha256: dut.timeline_sha256,
        pcm_sha256,
    };
    write_json(&role_path(directory, role, "metadata.json"), &metadata);
    println!(
        "NATIVE_ROLE_METADATA {}",
        serde_json::to_string(&metadata).unwrap()
    );
    drop(raw);
    eprintln!(
        "NATIVE_ROLE_PHASE {} deleted; process exit establishes final containment",
        role.name()
    );
}

fn compare_profile(directory: &Path, profile: usize, baseline: &serde_json::Value) {
    let metadata: [RoleMetadata; 3] = [Role::Dut, Role::RawA, Role::RawO]
        .map(|role| read_json(&role_path(directory, role, "metadata.json")));
    let [dut, a, o] = &metadata;
    for (role, value) in [Role::Dut, Role::RawA, Role::RawO]
        .into_iter()
        .zip(&metadata)
    {
        let identity: serde_json::Value = read_json(&role_path(directory, role, "identity.json"));
        let process: serde_json::Value = read_json(&role_path(directory, role, "process.json"));
        assert_eq!(identity["process_id"], value.pid);
        assert_eq!(process["pid"], value.pid);
        assert_eq!(process["success"], true);
        for key in ["native_executable", "python314_dll", "rubberband_dll"] {
            assert_eq!(
                identity[key], baseline[key],
                "changed {key} across isolated roles"
            );
        }
        assert_eq!(value.profile, profile);
        assert_eq!(value.role, role.name());
        assert_eq!(value.output_origin, dut.output_origin);
        assert_eq!(value.source_sha256, dut.source_sha256);
        assert_eq!(value.source_resident_start, dut.source_resident_start);
        assert_eq!(value.source_frames, dut.source_frames);
        assert_eq!(value.dry_sha256, dut.dry_sha256);
        assert_eq!(value.timeline_sha256, dut.timeline_sha256);
    }
    let (actual, actual_digest) = read_pcm(&role_path(directory, Role::Dut, "pcm.f32le"));
    let (raw_a, a_digest) = read_pcm(&role_path(directory, Role::RawA, "pcm.f32le"));
    let (raw_o, o_digest) = read_pcm(&role_path(directory, Role::RawO, "pcm.f32le"));
    let (dry, dry_digest) = read_pcm(&directory.join("dry.f32le"));
    assert_eq!(actual_digest, dut.pcm_sha256);
    assert_eq!(a_digest, a.pcm_sha256);
    assert_eq!(o_digest, o.pcm_sha256);
    assert_eq!(dry_digest, dut.dry_sha256);
    assert_eq!(dut.origin, 0);
    assert_eq!(o.origin, 0);
    assert!(a.origin <= E);
    let bound = a
        .history_bound
        .expect("RawA must preserve its independent bound");
    assert!(bound <= E);
    assert_eq!(a.origin, (E - bound) / FORMAT_BLOCK * FORMAT_BLOCK);
    assert_eq!(a.origin % FORMAT_BLOCK, 0);
    assert_eq!(actual[0].len(), CAPTURE_END);
    assert_eq!(dry[0].len(), CAPTURE_END);
    assert!(dry.iter().flatten().all(|sample| sample.is_finite()));
    assert_eq!(raw_o[0].len(), CAPTURE_END);
    assert_eq!(raw_a[0].len(), CAPTURE_END - a.origin);
    for (value, pcm) in metadata.iter().zip([&actual, &raw_a, &raw_o]) {
        assert_eq!(value.frames, pcm[0].len());
        assert!(pcm.iter().flatten().all(|sample| sample.is_finite()));
    }
    assert!(
        actual
            .iter()
            .all(|channel| channel[..CAPTURE_START].iter().all(|sample| *sample == 0.0)),
        "earlier actual response cannot be discarded"
    );
    assert!(
        dry.iter()
            .all(|channel| channel[..CAPTURE_START].iter().all(|sample| *sample == 0.0))
    );
    let mut a_error = [0.0_f32; FORMAT_CHANNELS];
    let mut o_error = [0.0_f32; FORMAT_CHANNELS];
    for active in E..CAPTURE_END {
        for channel in 0..FORMAT_CHANNELS {
            a_error[channel] = a_error[channel]
                .max((actual[channel][active] - raw_a[channel][active - a.origin]).abs());
            o_error[channel] =
                o_error[channel].max((actual[channel][active] - raw_o[channel][active]).abs());
        }
    }
    // The original absolute provenance gate is unchanged by process isolation.
    assert!(
        a_error.into_iter().all(|error| error < 2.0e-6),
        "exact A provenance failed: {a_error:?}"
    );
    assert_eq!(a.fifos, dut.fifos);
    assert_eq!(a.fifos, o.fifos);
    let dry_q = energy_times(&stereo_energy(&dry), RATE);
    let actual_window = std::array::from_fn(|channel| actual[channel][CAPTURE_START..].to_vec());
    let masks = [
        marker_metrics("public_uncropped", &actual_window, CAPTURE_START, dry_q),
        marker_metrics("A_uncropped", &raw_a, a.origin, dry_q),
        marker_metrics("O_uncropped", &raw_o, 0, dry_q),
    ];
    let (pitch, signal, offset) = frozen_profile(profile);
    let comparison = serde_json::json!({
        "profile": profile,
        "lock": true,
        "base_shift": pitch.base_shift(),
        "extra_shift": pitch.extra_shift(),
        "total_semitones": pitch.total_semitones(),
        "attack": format!("{signal:?}"),
        "offset": offset,
        "sample_rate": RATE,
        "channels": FORMAT_CHANNELS,
        "native_options": "0x10000000",
        "native_block": FORMAT_BLOCK,
        "output_O": dut.output_origin,
        "A": a.origin,
        "H": a.history_bound,
        "F": F,
        "E": E,
        "capture_start": CAPTURE_START,
        "capture_end": CAPTURE_END,
        "A_max_error": a_error,
        "O_max_error": o_error,
        "fifos": a.fifos,
        "dry_q": dry_q,
        "public_A_O_criterion_failure_masks": masks,
        "criterion_failures_are_diagnostic": true,
        "acoustic_acceptance_inferred": false,
    });
    write_json(&directory.join("comparison.json"), &comparison);
    println!(
        "NATIVE_ISOLATED_COMPARISON {}",
        serde_json::to_string(&comparison).unwrap()
    );
}

fn create_new(path: &Path) -> fs::File {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("write only fresh files in the verified owned artifact directory")
}

fn write_json(path: &Path, value: &impl serde::Serialize) {
    let mut file = create_new(path);
    serde_json::to_writer_pretty(&mut file, value).unwrap();
    file.write_all(b"\n").unwrap();
    file.flush().unwrap();
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_pcm(path: &Path, channels: &[Vec<f32>; FORMAT_CHANNELS]) -> String {
    assert_eq!(channels[0].len(), channels[1].len());
    let mut bytes = Vec::with_capacity(channels[0].len() * FORMAT_CHANNELS * size_of::<f32>());
    for frame in 0..channels[0].len() {
        for channel in channels {
            bytes.extend_from_slice(&channel[frame].to_bits().to_le_bytes());
        }
    }
    let mut file = create_new(path);
    file.write_all(&bytes).unwrap();
    file.flush().unwrap();
    digest(&bytes)
}

fn read_pcm(path: &Path) -> ([Vec<f32>; FORMAT_CHANNELS], String) {
    let bytes = fs::read(path).unwrap();
    assert_eq!(bytes.len() % (FORMAT_CHANNELS * size_of::<f32>()), 0);
    let mut channels = std::array::from_fn(|_| Vec::with_capacity(bytes.len() / 8));
    for frame in bytes.chunks_exact(FORMAT_CHANNELS * size_of::<f32>()) {
        for (channel, samples) in channels.iter_mut().enumerate() {
            let first = channel * size_of::<f32>();
            let bits = u32::from_le_bytes(frame[first..first + 4].try_into().unwrap());
            samples.push(f32::from_bits(bits));
        }
    }
    (channels, digest(&bytes))
}

fn timeline_digest(timeline: &[FormatFrame]) -> String {
    let mut hash = Sha256::new();
    for frame in timeline {
        hash.update((frame.frame as u64).to_le_bytes());
        hash.update(frame.fraction.to_bits().to_le_bytes());
        hash.update(frame.rate.to_bits().to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}
