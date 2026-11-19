# Stems (`cratekit-core::stems`)

Stem separation (SPEC, Module 4): vocals, drums, bass and other with htdemucs on ONNX Runtime. The decisions behind it (runtime, libraries per target, model and its licence, CoreML) are in `docs/ARCHITECTURE.md`, Stems notes (#29). This page covers the code.

So far: loading ONNX Runtime (#115) and downloading the model (#30, the core part). The app commands for the download, the separator (#31) and backend selection (#32) come next.

## Loading ONNX Runtime (`stems::runtime`)

ONNX Runtime is not linked into the app. `load(path)` opens Microsoft's library at run time (the installers bundle it next to the app), so Cratekit starts on every supported system, and when stems cannot run, the reason is a value the UI can show.

| Item | What it is |
| --- | --- |
| `load(library: &Path) -> Result<&'static Runtime, Unavailable>` | Checks the CPU and the file, then loads the library (an absolute path) with telemetry off. ONNX Runtime allows one per process, so the runtime is kept: later calls with the same path return it, another path is refused. A failed load leaves nothing behind and can be tried again. |
| `check_cpu() -> Result<(), Unavailable>` | x86_64 needs AVX2 and FMA; arm64 always passes. Refused: most Intel CPUs from before 2013 and low-cost Pentium, Celeron and Atom chips. Microsoft's builds choose their SIMD code at run time, so they may run there too (slowly); until that is tested on such a CPU, stems are refused rather than risk a crash. |
| `Runtime` | The loaded library: `library()`, `minor_version()` (32 for 1.32) and `environment()` for creating sessions. |
| `Unavailable` | Why stems cannot run: `CpuTooOld { missing }`, `LibraryMissing { path }`, `LibraryFailed { path, reason }` (the loader's own message: a missing dependency, another CPU architecture, not ONNX Runtime), `TooOld { found }`. |
| `MIN_MINOR_VERSION` | 23: ONNX Runtime 1.23, the last build for Intel Macs; matches `ort`'s `api-23` feature. |

The `ort` crate is used with `default-features = false`, `load-dynamic` and `api-23`: nothing is downloaded when building, and any library from 1.23 on works (1.32 serves API 23 too).

## The model and its download (`stems::models`)

The only network call in the app. The model is never bundled or hosted by Cratekit (its weights' licence is unclear): the user opts in, and the files come from the source, pinned to one revision.

| Item | What it is |
| --- | --- |
| `HTDEMUCS` | htdemucs 4 stems, Intel's split export from Hugging Face revision `efb3d2bc`: `htdemucs_fwd.onnx` (2.4 MB) and `htdemucs_fwd.onnx.data` (168.4 MB), each with its size and SHA-256. |
| `Model::download(models_dir, progress, cancel)` | Downloads the files that are not in place into `<models_dir>/htdemucs/`. Each file goes to `<name>.part`, is checked against its SHA-256 and only then renamed: a file under its final name has always been verified. An interrupted download keeps the partial file and the next call resumes it with an HTTP Range request (a server that ignores the range sends the whole file again). Cancel is checked every 256 KB. |
| `Model::is_downloaded(models_dir)` | Every file in place with its size. |
| `Model::delete(models_dir)` | Removes the model's folder. |
| `Stage`, `DownloadProgress` | Progress as a stage (`Download`, `Verify`) and the fraction of the whole model, 0 to 1. |

HTTPS is `ureq` with `rustls` and `ring`, trusting the operating system's certificates (`rustls-platform-verifier`): no certificate bundle and no OpenSSL. Errors: `CoreError::Download { url, reason }` (no connection, an HTTP status, a connection closed early) and `CoreError::Checksum { file }` (the file was deleted); the app codes are `download` and `checksum`.

Not handled yet: a connection that stalls without closing blocks until the operating system gives up on it (there is no idle timeout per read), and cancel takes effect only when the next bytes arrive.

## Getting the library

`.github/scripts/fetch-onnxruntime.sh <dest dir>` downloads Microsoft's NuGet package for the machine's OS and CPU, checks it against a pinned SHA-256, and copies the library out (1.32.0, or 1.23.2 on Intel Macs). It prints the library's path. CI uses it; the installers will use it to bundle the library (#31).

**Windows needs the Visual C++ runtime.** Microsoft's `onnxruntime.dll` imports `VCRUNTIME140.dll`, `VCRUNTIME140_1.dll`, `MSVCP140.dll` and `MSVCP140_1.dll` (ARM64: the first and the last two). The app itself links the runtime statically, but this DLL does not, and every GitHub Windows image has the runtime installed, so CI cannot catch a machine without it. Bundling the library (#31) must deal with this, and the installer check must then scan every bundled DLL, not only the app.

## Tests

- Download tests against a small HTTP server inside the test (no internet): every file verified and progress moving forward to `Verify` 1.0; a wrong checksum deletes the file; a connection cut halfway keeps the partial file and the next call resumes it with a Range request; a server that ignores ranges starts over; a downloaded model makes no request; cancel before any request; an HTTP 404; delete. `the_real_model_downloads_and_verifies` (ignored, 171 MB) downloads the pinned model from Hugging Face: about 80 s, both checksums match.
- Unit tests: the CPU verdict for every AVX2 and FMA combination, a missing file, a relative path, a file that is not ONNX Runtime, reading a version string, the wording of the reasons.
- `a_real_onnx_runtime_loads_and_runs_a_model` (ignored by default): loads a real library from `CRATEKIT_ONNXRUNTIME` and runs a tiny model (`y = x * x`, written as protobuf bytes in the test, so there is no model file), then checks that a second `load` returns the same runtime and that another library is refused. Locally:

  ```bash
  lib=$(.github/scripts/fetch-onnxruntime.sh /tmp/onnxruntime)
  CRATEKIT_ONNXRUNTIME=$lib cargo test -p cratekit-core --lib stems::runtime -- --ignored
  ```

- The `Stems runtime` workflow (`.github/workflows/stems-runtime.yml`) runs that test on all six targets plus Windows Server 2022, on Ubuntu 22.04 (the glibc floor) and with 1.23.2 on the Intel Mac: manually, weekly, and on pull requests that change the stems runtime.

Checked by hand on Apple Silicon: 1.32.0 and 1.23.2 load and run the model; 1.22.0 is refused with `TooOld { found: 22 }`; the Intel build gives the loader's own "failed to load" message.
