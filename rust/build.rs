use std::env;
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
        validate_optional_include_dir();
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

    if let Err(err) = pkg_config::Config::new().probe("rubberband") {
        panic!(
            "Rubber Band library not found through pkg-config ({err}). \
             Install the system Rubber Band development package, configure PKG_CONFIG_PATH, \
             or set RUBBERBAND_LIB_DIR."
        );
    }
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

fn validate_optional_include_dir() {
    if let Some(include_dir) = env_path("RUBBERBAND_INCLUDE_DIR") {
        validate_include_dir(&include_dir);
    }
}

fn validate_include_dir(include_dir: &Path) {
    validate_dir("Rubber Band include directory", include_dir);

    let nested_header = include_dir.join("rubberband").join("rubberband-c.h");
    let flat_header = include_dir.join("rubberband-c.h");
    if !nested_header.is_file() && !flat_header.is_file() {
        panic!(
            "Rubber Band C API header not found under {}. Expected rubberband/rubberband-c.h.",
            include_dir.display()
        );
    }
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
