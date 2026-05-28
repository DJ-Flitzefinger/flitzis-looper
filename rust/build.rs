use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const RERUN_ENV_VARS: &[&str] = &[
    "RUBBERBAND_LIB_DIR",
    "RUBBERBAND_INCLUDE_DIR",
    "RUBBERBAND_LINK_KIND",
    "RUBBERBAND_EXTRA_LIBS",
    "RUBBERBAND_VCPKG_TRIPLET",
    "VCPKG_DEFAULT_TRIPLET",
    "VCPKG_ROOT",
    "LOCALAPPDATA",
    "PKG_CONFIG_PATH",
    "PKG_CONFIG_LIBDIR",
    "PKG_CONFIG_SYSROOT_DIR",
];

fn main() {
    for var in RERUN_ENV_VARS {
        println!("cargo:rerun-if-env-changed={var}");
    }

    if let Some(lib_dir) = env_path("RUBBERBAND_LIB_DIR") {
        validate_dir("RUBBERBAND_LIB_DIR", &lib_dir);
        validate_required_override_include_dir();
        emit_manual_link(&lib_dir);
        return;
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        if let Some(vcpkg_root) = discover_vcpkg_root() {
            emit_vcpkg_link(&vcpkg_root);
            return;
        }

        panic!(
            "Rubber Band library not found. Set RUBBERBAND_LIB_DIR or VCPKG_ROOT, \
             or install vcpkg under %LOCALAPPDATA%\\vcpkg and install rubberband:x64-windows."
        );
    }

    match pkg_config::Config::new().probe("rubberband") {
        Ok(library) => validate_pkg_config_live_api(&library),
        Err(err) => {
            panic!(
                "Rubber Band library not found through pkg-config ({err}). \
                 Install a Rubber Band development package that provides the LiveShifter C API, \
                 configure PKG_CONFIG_PATH, or set RUBBERBAND_LIB_DIR and RUBBERBAND_INCLUDE_DIR."
            );
        }
    }
}

fn validate_pkg_config_live_api(library: &pkg_config::Library) {
    for include_dir in &library.include_paths {
        if let Some(header) = rubberband_header_path(include_dir) {
            validate_live_api_header(&header);
            return;
        }
    }

    for include_dir in [Path::new("/usr/include"), Path::new("/usr/local/include")] {
        if let Some(header) = rubberband_header_path(include_dir) {
            validate_live_api_header(&header);
            return;
        }
    }

    panic!(
        "Rubber Band was found through pkg-config, but rubberband/rubberband-c.h was not found. \
         Install the matching development headers or configure PKG_CONFIG_PATH."
    );
}

fn validate_required_override_include_dir() {
    let include_dir = env_path("RUBBERBAND_INCLUDE_DIR").unwrap_or_else(|| {
        panic!(
            "RUBBERBAND_INCLUDE_DIR must be set when RUBBERBAND_LIB_DIR is used so the build can \
             verify that the selected Rubber Band library provides the LiveShifter C API."
        )
    });
    validate_include_dir(&include_dir);
}

fn emit_vcpkg_link(vcpkg_root: &Path) {
    let triplet = env::var("RUBBERBAND_VCPKG_TRIPLET")
        .or_else(|_| env::var("VCPKG_DEFAULT_TRIPLET"))
        .unwrap_or_else(|_| "x64-windows".to_string());
    let installed_dir = vcpkg_root.join("installed").join(&triplet);
    let include_dir = installed_dir.join("include");
    let lib_dir = installed_dir.join("lib");
    let runtime_dir = installed_dir.join("bin");

    validate_include_dir(&include_dir);
    validate_dir("vcpkg Rubber Band library directory", &lib_dir);

    let import_lib = lib_dir.join("rubberband.lib");
    if !import_lib.is_file() {
        panic!(
            "Rubber Band import library not found at {}. Run `vcpkg install rubberband:{triplet}`.",
            import_lib.display()
        );
    }

    if !runtime_dir.is_dir() {
        println!(
            "cargo:warning=Rubber Band runtime DLL directory was not found at {}. \
             Windows source runs must make the runtime DLLs available before app startup.",
            runtime_dir.display()
        );
    }

    emit_manual_link(&lib_dir);
}

fn emit_manual_link(lib_dir: &Path) {
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib={}={}", link_kind(), "rubberband");

    if let Ok(extra_libs) = env::var("RUBBERBAND_EXTRA_LIBS") {
        for lib in extra_libs
            .split([',', ';'])
            .map(str::trim)
            .filter(|lib| !lib.is_empty())
        {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}

fn link_kind() -> &'static str {
    match env::var("RUBBERBAND_LINK_KIND").as_deref() {
        Ok("static") => "static",
        Ok("dylib") | Err(_) => "dylib",
        Ok(other) => panic!("RUBBERBAND_LINK_KIND must be `dylib` or `static`, got `{other}`"),
    }
}

fn discover_vcpkg_root() -> Option<PathBuf> {
    env_path("VCPKG_ROOT")
        .filter(|path| path.is_dir())
        .or_else(|| {
            env_path("LOCALAPPDATA")
                .map(|path| path.join("vcpkg"))
                .filter(|path| path.is_dir())
        })
}

fn validate_include_dir(include_dir: &Path) {
    validate_dir("Rubber Band include directory", include_dir);

    let Some(header) = rubberband_header_path(include_dir) else {
        panic!(
            "Rubber Band C API header not found under {}. Expected rubberband/rubberband-c.h.",
            include_dir.display()
        );
    };

    validate_live_api_header(&header);
}

fn rubberband_header_path(include_dir: &Path) -> Option<PathBuf> {
    [
        include_dir.join("rubberband").join("rubberband-c.h"),
        include_dir.join("rubberband-c.h"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn validate_live_api_header(header: &Path) {
    let contents = fs::read_to_string(header).unwrap_or_else(|err| {
        panic!(
            "Failed to read Rubber Band C API header at {}: {err}",
            header.display()
        )
    });

    let required_symbols = [
        "rubberband_live_new",
        "rubberband_live_delete",
        "rubberband_live_reset",
        "rubberband_live_set_pitch_scale",
        "rubberband_live_get_pitch_scale",
        "rubberband_live_get_start_delay",
        "rubberband_live_get_block_size",
        "rubberband_live_shift",
        "rubberband_live_get_channel_count",
        "rubberband_live_set_debug_level",
        "rubberband_live_set_default_debug_level",
    ];

    if required_symbols
        .iter()
        .all(|symbol| contents.contains(symbol))
    {
        return;
    }

    panic!(
        "Rubber Band C API header at {} does not provide the LiveShifter `rubberband_live_*` API. \
         The Key Lock backend requires a Rubber Band development package with LiveShifter support; \
         the Ubuntu 24.04 `librubberband-dev` 3.3.0 package is too old. Use a newer distro package, \
         a source install, or another documented install path that provides these symbols.",
        header.display()
    );
}

fn validate_dir(label: &str, path: &Path) {
    if !path.is_dir() {
        panic!(
            "{label} does not exist or is not a directory: {}",
            path.display()
        );
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name).and_then(|value| {
        if value.to_string_lossy().trim().is_empty() {
            None
        } else {
            Some(PathBuf::from(value))
        }
    })
}
