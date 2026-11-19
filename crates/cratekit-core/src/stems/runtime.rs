//! Loading ONNX Runtime for stems (#115).
//!
//! The library is not linked into the app: [`load`] opens it at run time from
//! a path (the installer bundles Microsoft's build next to the app). So the
//! app starts on every supported system, and when stems cannot run here,
//! [`Unavailable`] says why: the CPU, a missing or unloadable library, or one
//! that is too old.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use ort::environment::Environment;

/// The oldest ONNX Runtime minor version stems accept (1.23, the last build
/// for Intel Macs). It matches the `api-23` feature of `ort`.
pub const MIN_MINOR_VERSION: u32 = 23;

/// The runtime once loaded: ONNX Runtime allows one per process.
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Makes two first calls to [`load`] at the same time load only once.
static LOADING: Mutex<()> = Mutex::new(());

/// Why stems cannot run on this computer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Unavailable {
    /// An x86_64 CPU without AVX2 or FMA (most Intel CPUs before 2013, and
    /// low-cost Pentium, Celeron and Atom chips).
    #[error("stems need a newer CPU (this one has no {missing})")]
    CpuTooOld {
        /// The first missing instruction set.
        missing: &'static str,
    },
    /// The library file is not there.
    #[error("ONNX Runtime is missing: {}", path.display())]
    LibraryMissing {
        /// Where it was expected.
        path: PathBuf,
    },
    /// The file is there but could not be loaded as ONNX Runtime.
    #[error("ONNX Runtime could not be loaded from {}: {reason}", path.display())]
    LibraryFailed {
        /// The file.
        path: PathBuf,
        /// What the loader said (a missing dependency, another CPU
        /// architecture, not ONNX Runtime at all).
        reason: String,
    },
    /// The library is older than [`MIN_MINOR_VERSION`].
    #[error("ONNX Runtime 1.{found} is too old: stems need 1.{MIN_MINOR_VERSION} or newer")]
    TooOld {
        /// Its minor version.
        found: u32,
    },
}

/// A loaded ONNX Runtime, ready to create sessions.
#[derive(Debug)]
pub struct Runtime {
    library: PathBuf,
    minor_version: u32,
    environment: Environment,
}

impl Runtime {
    /// The library file it was loaded from.
    #[must_use]
    pub fn library(&self) -> &Path {
        &self.library
    }

    /// The library's minor version (32 for ONNX Runtime 1.32).
    #[must_use]
    pub fn minor_version(&self) -> u32 {
        self.minor_version
    }

    /// The environment sessions are created in.
    #[must_use]
    pub fn environment(&self) -> &Environment {
        &self.environment
    }
}

/// Checks that this CPU can run stems: on x86_64 it needs AVX2 and FMA;
/// every arm64 CPU we support passes. Microsoft's builds choose their SIMD
/// code at run time, so they may also run without AVX2, slowly; until that is
/// tested on such a CPU, stems are refused there rather than risk a crash.
///
/// # Errors
///
/// [`Unavailable::CpuTooOld`] with the first missing instruction set.
pub fn check_cpu() -> Result<(), Unavailable> {
    #[cfg(target_arch = "x86_64")]
    {
        cpu_verdict(
            std::arch::is_x86_feature_detected!("avx2"),
            std::arch::is_x86_feature_detected!("fma"),
        )
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        Ok(())
    }
}

/// The verdict for an x86_64 CPU with or without AVX2 and FMA.
#[cfg_attr(
    not(any(target_arch = "x86_64", test)),
    expect(dead_code, reason = "only x86_64 checks instruction sets")
)]
fn cpu_verdict(avx2: bool, fma: bool) -> Result<(), Unavailable> {
    if !avx2 {
        return Err(Unavailable::CpuTooOld { missing: "AVX2" });
    }
    if !fma {
        return Err(Unavailable::CpuTooOld { missing: "FMA" });
    }
    Ok(())
}

/// Loads ONNX Runtime from `library`, an absolute path to
/// `libonnxruntime.dylib`, `libonnxruntime.so` or `onnxruntime.dll`, with
/// telemetry off.
///
/// ONNX Runtime can be loaded once per process, so the runtime is kept: later
/// calls with the same path return it. A failed load leaves nothing behind,
/// so it can be tried again (for example once the library is in place).
///
/// # Errors
///
/// [`Unavailable`]: the CPU is too old, the file is missing, it does not load
/// as ONNX Runtime (the reason says why), it is older than
/// [`MIN_MINOR_VERSION`], or ONNX Runtime is already loaded from another file.
pub fn load(library: &Path) -> Result<&'static Runtime, Unavailable> {
    let _loading = LOADING.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(runtime) = RUNTIME.get() {
        if runtime.library == library {
            return Ok(runtime);
        }
        return Err(Unavailable::LibraryFailed {
            path: library.to_path_buf(),
            reason: format!(
                "ONNX Runtime is already loaded from {}",
                runtime.library.display()
            ),
        });
    }
    let runtime = open(library)?;
    Ok(RUNTIME.get_or_init(|| runtime))
}

/// Checks and loads the library (see [`load`]).
fn open(library: &Path) -> Result<Runtime, Unavailable> {
    check_cpu()?;
    let failed = |reason: String| Unavailable::LibraryFailed {
        path: library.to_path_buf(),
        reason,
    };
    // A relative path would be searched for, and on Windows could find the
    // system's own older copy.
    if !library.is_absolute() {
        return Err(failed("the path must be absolute".to_owned()));
    }
    if !library.is_file() {
        return Err(Unavailable::LibraryMissing {
            path: library.to_path_buf(),
        });
    }
    // `init_from` keeps the library only when it loads and is new enough, and
    // its error says why not (it also refuses versions below `api-23`).
    let builder = ort::init_from(library).map_err(|err| match err {
        ort::LoadDynamicError::BadVersion {
            ref version_str, ..
        } => minor_of(version_str).map_or_else(
            || failed(err.to_string()),
            |found| Unavailable::TooOld { found },
        ),
        other => failed(other.to_string()),
    })?;
    let environment = builder
        .with_name("cratekit")
        .with_telemetry(false)
        .build()
        .map_err(|err| failed(err.to_string()))?;
    let minor_version = ort::util::probe(library)
        .ok_or_else(|| failed("its version could not be read".to_owned()))?;
    Ok(Runtime {
        library: library.to_path_buf(),
        minor_version,
        environment,
    })
}

/// The minor version in an ONNX Runtime version string ("1.22.0" gives 22).
fn minor_of(version: &str) -> Option<u32> {
    let mut parts = version.split('.');
    match (parts.next(), parts.next()) {
        (Some("1"), Some(minor)) => minor.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x86_64_needs_avx2_and_fma() {
        assert_eq!(cpu_verdict(true, true), Ok(()));
        assert_eq!(
            cpu_verdict(false, true),
            Err(Unavailable::CpuTooOld { missing: "AVX2" })
        );
        assert_eq!(
            cpu_verdict(true, false),
            Err(Unavailable::CpuTooOld { missing: "FMA" })
        );
        assert_eq!(
            cpu_verdict(false, false),
            Err(Unavailable::CpuTooOld { missing: "AVX2" })
        );
    }

    #[test]
    fn a_missing_library_says_where_it_was_expected() {
        if check_cpu().is_err() {
            return; // The CPU reason comes first on such a machine.
        }
        let path = Path::new("/nowhere/libonnxruntime.so");
        assert_eq!(
            load(path).map(|_| ()),
            Err(Unavailable::LibraryMissing {
                path: path.to_path_buf()
            })
        );
    }

    #[test]
    fn a_file_that_is_not_onnx_runtime_is_refused() {
        if check_cpu().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("cratekit-ort-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let fake = dir.join("libonnxruntime.so");
        std::fs::write(&fake, b"not a library").expect("write");
        let result = load(&fake).map(|_| ());
        std::fs::remove_dir_all(&dir).ok();
        assert!(
            matches!(result, Err(Unavailable::LibraryFailed { ref path, .. }) if *path == fake),
            "{result:?}"
        );
    }

    #[test]
    fn a_relative_path_is_refused() {
        if check_cpu().is_err() {
            return;
        }
        let result = load(Path::new("onnxruntime.dll")).map(|_| ());
        assert!(
            matches!(result, Err(Unavailable::LibraryFailed { ref reason, .. }) if reason.contains("absolute")),
            "{result:?}"
        );
    }

    #[test]
    fn reads_the_minor_version() {
        assert_eq!(minor_of("1.22.0"), Some(22));
        assert_eq!(minor_of("1.32.0"), Some(32));
        assert_eq!(minor_of("2.0.0"), None);
        assert_eq!(minor_of("garbage"), None);
    }

    #[test]
    fn reasons_read_as_sentences() {
        assert_eq!(
            Unavailable::TooOld { found: 20 }.to_string(),
            "ONNX Runtime 1.20 is too old: stems need 1.23 or newer"
        );
        assert_eq!(
            Unavailable::CpuTooOld { missing: "AVX2" }.to_string(),
            "stems need a newer CPU (this one has no AVX2)"
        );
    }

    /// Loads a real ONNX Runtime and runs a tiny model on it. CI runs this on
    /// every target with Microsoft's library from
    /// `.github/scripts/fetch-onnxruntime.sh`; locally:
    /// `CRATEKIT_ONNXRUNTIME=/path/to/libonnxruntime.dylib cargo test -p cratekit-core stems -- --ignored`.
    #[test]
    #[ignore = "needs CRATEKIT_ONNXRUNTIME, the path to an ONNX Runtime library"]
    fn a_real_onnx_runtime_loads_and_runs_a_model() {
        let library = std::env::var_os("CRATEKIT_ONNXRUNTIME").expect("set CRATEKIT_ONNXRUNTIME");
        let library = Path::new(&library);
        let runtime = load(library).expect("load ONNX Runtime");
        assert!(runtime.minor_version() >= MIN_MINOR_VERSION);
        assert_eq!(runtime.library(), library);
        // Loaded once per process: the same path gives the same runtime, and
        // another library is refused with the one in use.
        assert!(std::ptr::eq(runtime, load(library).expect("load again")));
        let other = library.with_file_name("another-onnxruntime");
        assert!(
            matches!(load(&other), Err(Unavailable::LibraryFailed { ref reason, .. }) if reason.contains("already loaded")),
        );

        let model = square_model();
        let mut session = ort::session::Session::builder(runtime.environment())
            .expect("session builder")
            .commit_from_memory(&model)
            .expect("load the model");
        let input = ort::value::Tensor::from_array(([4_usize], vec![1.0_f32, -2.0, 3.0, 0.5]))
            .expect("input tensor");
        let outputs = session.run(ort::inputs![input]).expect("run");
        let (_, squared) = outputs[0]
            .try_extract_tensor::<f32>()
            .expect("output tensor");
        assert_eq!(squared, &[1.0, 4.0, 9.0, 0.25]);
    }

    /// An ONNX model computing `y = x * x` for `x` of 4 floats, written as
    /// protobuf bytes by hand so the test needs no model file.
    fn square_model() -> Vec<u8> {
        let dim = field_varint(1, 4); // dim_value
        let shape = field_bytes(1, &dim); // dim
        let tensor_type = [field_varint(1, 1), field_bytes(2, &shape)].concat(); // FLOAT, shape
        let value_type = field_bytes(1, &tensor_type); // tensor_type
        let value_info =
            |name: &str| [field_bytes(1, name.as_bytes()), field_bytes(2, &value_type)].concat();
        let node = [
            field_bytes(1, b"x"),
            field_bytes(1, b"x"),
            field_bytes(2, b"y"),
            field_bytes(4, b"Mul"),
        ]
        .concat();
        let graph = [
            field_bytes(1, &node),
            field_bytes(2, b"square"),
            field_bytes(11, &value_info("x")),
            field_bytes(12, &value_info("y")),
        ]
        .concat();
        let opset = [field_bytes(1, b""), field_varint(2, 17)].concat();
        [
            field_varint(1, 8),
            field_bytes(8, &opset),
            field_bytes(7, &graph),
        ]
        .concat()
    }

    fn varint(mut value: u64, out: &mut Vec<u8>) {
        while value >= 0x80 {
            out.push((value as u8) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    fn field_varint(field: u64, value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        varint(field << 3, &mut out);
        varint(value, &mut out);
        out
    }

    fn field_bytes(field: u64, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        varint((field << 3) | 2, &mut out);
        varint(bytes.len() as u64, &mut out);
        out.extend_from_slice(bytes);
        out
    }
}
