//! The stems model and its download (#30): the only network call in the app.
//!
//! The model is never bundled or hosted by Cratekit (the licence of the
//! htdemucs weights is unclear, see docs/ARCHITECTURE.md, Stems notes). The
//! user opts in, and [`Model::download`] fetches the files from a URL pinned
//! to one revision. Each file is written to `<name>.part`, checked against its
//! pinned SHA-256, and only then renamed into place: a file under its final
//! name has always been verified. An interrupted download resumes where it
//! stopped.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ring::digest::{Context, SHA256};
use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

use crate::CoreError;
use crate::progress::CancellationToken;

/// One file of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    /// Its name in the model's folder (ONNX Runtime finds the weights file
    /// next to the graph by this name).
    pub name: &'static str,
    /// Where it is downloaded from, pinned to one revision.
    pub url: &'static str,
    /// Its size in bytes.
    pub size: u64,
    /// Its SHA-256, in lowercase hex.
    pub sha256: &'static str,
}

/// A model: a name (its folder under the models folder) and its files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    /// The model's name, also its folder name.
    pub name: &'static str,
    /// Its files.
    pub files: &'static [ModelFile],
}

/// htdemucs, 4 stems, as Intel's split export (STFT outside the graph), from
/// Hugging Face revision `efb3d2bc` of Intel/demucs-openvino. Chosen in the
/// #30 spike: the only export that is fast on both CoreML and CPU.
pub const HTDEMUCS: Model = Model {
    name: "htdemucs",
    files: &[
        ModelFile {
            name: "htdemucs_fwd.onnx",
            url: "https://huggingface.co/Intel/demucs-openvino/resolve/efb3d2bc54f93899b13b72b12c57001e19f09ae2/htdemucs_v4/htdemucs_fwd.onnx",
            size: 2_385_507,
            sha256: "555f511fb653348a1c9495ab6b6091767d1920798f3fba68629a1fe5b135a45f",
        },
        ModelFile {
            name: "htdemucs_fwd.onnx.data",
            url: "https://huggingface.co/Intel/demucs-openvino/resolve/efb3d2bc54f93899b13b72b12c57001e19f09ae2/htdemucs_v4/htdemucs_fwd.onnx.data",
            size: 168_361_984,
            sha256: "23a7c68669c041363caaef94641e961dc8c4e16b9891cb87f58f0e199a5e8820",
        },
    ],
};

/// The step a download is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Receiving bytes.
    Download,
    /// Checking a file's SHA-256.
    Verify,
}

/// Receives a download's progress: the stage and the fraction of the whole
/// model done, 0 to 1.
pub trait DownloadProgress: Send + Sync {
    /// Called often; must return quickly.
    fn progress(&self, stage: Stage, fraction: f32);
}

impl<F: Fn(Stage, f32) + Send + Sync> DownloadProgress for F {
    fn progress(&self, stage: Stage, fraction: f32) {
        self(stage, fraction);
    }
}

/// How much is read before checking for cancellation and reporting progress.
const CHUNK: usize = 256 * 1024;

impl Model {
    /// The model's folder under `models_dir`.
    #[must_use]
    pub fn dir(&self, models_dir: &Path) -> PathBuf {
        models_dir.join(self.name)
    }

    /// The size of all its files, in bytes.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }

    /// Whether every file is in place with its size. Files only get their
    /// final name after their checksum matched.
    #[must_use]
    pub fn is_downloaded(&self, models_dir: &Path) -> bool {
        let dir = self.dir(models_dir);
        self.files.iter().all(|file| {
            fs::metadata(dir.join(file.name))
                .is_ok_and(|meta| meta.is_file() && meta.len() == file.size)
        })
    }

    /// Deletes the model's folder (nothing happens when there is none).
    ///
    /// # Errors
    ///
    /// [`CoreError::Io`] when the folder cannot be deleted.
    pub fn delete(&self, models_dir: &Path) -> Result<(), CoreError> {
        match fs::remove_dir_all(self.dir(models_dir)) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
            _ => Ok(()),
        }
    }

    /// Downloads the files that are not in place yet, resuming a partial
    /// file, and verifies each before giving it its final name.
    ///
    /// # Errors
    ///
    /// [`CoreError::Download`] when the network fails (the partial file is
    /// kept, so the next call resumes), [`CoreError::Checksum`] when a file
    /// is not the expected one (it is deleted), [`CoreError::Cancelled`],
    /// or [`CoreError::Io`].
    pub fn download(
        &self,
        models_dir: &Path,
        progress: &dyn DownloadProgress,
        cancel: &CancellationToken,
    ) -> Result<(), CoreError> {
        let dir = self.dir(models_dir);
        fs::create_dir_all(&dir)?;
        let agent = agent();
        let total = self.size().max(1);
        let mut done_before = 0;
        for file in self.files {
            cancel.check()?;
            let report = |stage: Stage, bytes: u64| {
                progress.progress(stage, fraction(done_before + bytes, total));
            };
            fetch(&agent, file, &dir, &report, cancel)?;
            done_before += file.size;
        }
        progress.progress(Stage::Verify, 1.0);
        Ok(())
    }
}

/// `part` of `total` as a fraction, 0 to 1.
fn fraction(part: u64, total: u64) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a progress fraction needs no more precision than f64 gives"
    )]
    let ratio = part as f64 / total as f64;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the ratio is clamped to 0 to 1, which f32 holds"
    )]
    let ratio = ratio.clamp(0.0, 1.0) as f32;
    ratio
}

/// An HTTPS client that trusts the operating system's certificates.
fn agent() -> Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    Agent::config_builder()
        .tls_config(tls)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .user_agent(concat!("Cratekit/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// Downloads one file into `dir` (resuming `<name>.part`), verifies it and
/// renames it into place. Does nothing when the file is already in place.
fn fetch(
    agent: &Agent,
    file: &ModelFile,
    dir: &Path,
    report: &dyn Fn(Stage, u64),
    cancel: &CancellationToken,
) -> Result<(), CoreError> {
    let final_path = dir.join(file.name);
    if fs::metadata(&final_path).is_ok_and(|meta| meta.len() == file.size) {
        report(Stage::Verify, file.size);
        return Ok(());
    }
    let part_path = dir.join(format!("{}.part", file.name));
    let mut have = fs::metadata(&part_path).map_or(0, |meta| meta.len());
    if have > file.size {
        fs::remove_file(&part_path)?;
        have = 0;
    }
    if have < file.size {
        have = receive(agent, file, &part_path, have, report, cancel)?;
    }
    if have != file.size {
        return Err(download_error(
            file,
            format!("received {have} of {} bytes", file.size),
        ));
    }
    report(Stage::Verify, file.size);
    if sha256_hex(&part_path)? != file.sha256 {
        fs::remove_file(&part_path)?;
        return Err(CoreError::Checksum {
            file: file.name.to_owned(),
        });
    }
    fs::rename(&part_path, &final_path)?;
    Ok(())
}

/// Receives the rest of `file` into `part_path`, which already holds `have`
/// bytes. Returns how many bytes the partial file holds afterwards.
fn receive(
    agent: &Agent,
    file: &ModelFile,
    part_path: &Path,
    have: u64,
    report: &dyn Fn(Stage, u64),
    cancel: &CancellationToken,
) -> Result<u64, CoreError> {
    let mut request = agent.get(file.url);
    if have > 0 {
        request = request.header("Range", format!("bytes={have}-"));
    }
    let response = request
        .call()
        .map_err(|err| download_error(file, err.to_string()))?;
    let status = response.status().as_u16();
    // 206: the server sends the rest. 200: it ignored the range and sends the
    // whole file, so start over.
    let (mut out, mut have) = match status {
        206 if have > 0 => (OpenOptions::new().append(true).open(part_path)?, have),
        200 => (File::create(part_path)?, 0),
        _ => return Err(download_error(file, format!("HTTP status {status}"))),
    };
    let mut body = response.into_body().into_reader();
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        cancel.check()?;
        let read = body
            .read(&mut buffer)
            .map_err(|err| download_error(file, err.to_string()))?;
        if read == 0 {
            break;
        }
        let chunk = buffer.get(..read).unwrap_or_default();
        out.write_all(chunk)?;
        have += read as u64;
        if have > file.size {
            return Err(download_error(
                file,
                "the server sent more than expected".to_owned(),
            ));
        }
        report(Stage::Download, have);
    }
    out.sync_all()?;
    Ok(have)
}

fn download_error(file: &ModelFile, reason: String) -> CoreError {
    CoreError::Download {
        url: file.url.to_owned(),
        reason,
    }
}

/// The SHA-256 of a file, in lowercase hex.
fn sha256_hex(path: &Path) -> Result<String, CoreError> {
    let mut input = File::open(path)?;
    let mut context = Context::new(&SHA256);
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        context.update(buffer.get(..read).unwrap_or_default());
    }
    Ok(context
        .finish()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use super::*;

    /// How the test server answers.
    #[derive(Clone, Copy)]
    enum Serve {
        /// Honour a Range header with 206.
        Ranges,
        /// Ignore Range headers and always send the whole file (200).
        IgnoreRanges,
        /// Close the connection after this many bytes of the first response.
        CutFirstAfter(usize),
    }

    /// A tiny HTTP server for the test files: `/<name>` serves that body.
    struct Server {
        base: String,
        requests: Arc<AtomicUsize>,
        ranges: Arc<Mutex<Vec<String>>>,
    }

    fn serve(files: Vec<(&'static str, Vec<u8>)>, mode: Serve) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let requests = Arc::new(AtomicUsize::new(0));
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let (count, seen) = (Arc::clone(&requests), Arc::clone(&ranges));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let number = count.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut first = String::new();
                reader.read_line(&mut first).expect("request line");
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let mut range_start = None;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("header");
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        let value = value.trim().trim_end_matches('-').to_owned();
                        seen.lock().expect("ranges").push(value.clone());
                        range_start = value.parse::<usize>().ok();
                    }
                }
                let Some((_, body)) = files.iter().find(|(name, _)| path == format!("/{name}"))
                else {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                    );
                    continue;
                };
                let (status, payload) = match (mode, range_start) {
                    (Serve::Ranges | Serve::CutFirstAfter(_), Some(start)) => {
                        ("206 Partial Content", body.get(start..).unwrap_or_default())
                    }
                    _ => ("200 OK", body.as_slice()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let payload = match mode {
                    Serve::CutFirstAfter(bytes) if number == 0 => {
                        payload.get(..bytes).unwrap_or(payload)
                    }
                    _ => payload,
                };
                let _ = stream.write_all(payload);
            }
        });
        Server {
            base,
            requests,
            ranges,
        }
    }

    /// Bytes that differ along the file, so a misplaced chunk changes the hash.
    fn content(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    fn sha(bytes: &[u8]) -> String {
        ring::digest::digest(&SHA256, bytes)
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// A model of the given files served by `server` (leaked: tests only).
    fn model(server: &Server, files: &[(&'static str, &[u8])]) -> &'static Model {
        let files: Vec<ModelFile> = files
            .iter()
            .map(|(name, bytes)| ModelFile {
                name,
                url: Box::leak(format!("{}/{name}", server.base).into_boxed_str()),
                size: bytes.len() as u64,
                sha256: Box::leak(sha(bytes).into_boxed_str()),
            })
            .collect();
        Box::leak(Box::new(Model {
            name: "test-model",
            files: Box::leak(files.into_boxed_slice()),
        }))
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cratekit-models-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn downloads_and_verifies_every_file() {
        let (graph, weights) = (content(1000, 1), content(700_000, 2));
        let server = serve(
            vec![("graph", graph.clone()), ("weights", weights.clone())],
            Serve::Ranges,
        );
        let model = model(&server, &[("graph", &graph), ("weights", &weights)]);
        let dir = temp_dir("ok");
        let seen = Mutex::new(Vec::new());
        let progress =
            |stage: Stage, fraction: f32| seen.lock().expect("seen").push((stage, fraction));
        assert!(!model.is_downloaded(&dir));

        model
            .download(&dir, &progress, &CancellationToken::new())
            .expect("download");

        assert!(model.is_downloaded(&dir));
        assert_eq!(
            fs::read(model.dir(&dir).join("weights")).expect("read"),
            weights
        );
        assert!(!model.dir(&dir).join("weights.part").exists());
        let seen = seen.into_inner().expect("seen");
        assert!(
            seen.windows(2).all(|pair| pair[0].1 <= pair[1].1),
            "progress goes forward"
        );
        assert_eq!(seen.last(), Some(&(Stage::Verify, 1.0)));
        assert!(seen.iter().any(|(stage, _)| *stage == Stage::Download));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_with_the_wrong_checksum_is_deleted() {
        let body = content(5000, 3);
        let server = serve(vec![("graph", body.clone())], Serve::Ranges);
        let mut tampered = body.clone();
        tampered[10] ^= 1;
        let model = model(&server, &[("graph", &tampered)]);
        let dir = temp_dir("checksum");

        let result = model.download(&dir, &|_: Stage, _: f32| {}, &CancellationToken::new());

        assert!(
            matches!(result, Err(CoreError::Checksum { ref file }) if file == "graph"),
            "{result:?}"
        );
        assert!(!model.dir(&dir).join("graph").exists());
        assert!(!model.dir(&dir).join("graph.part").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_interrupted_download_resumes_where_it_stopped() {
        let body = content(600_000, 4);
        let server = serve(
            vec![("weights", body.clone())],
            Serve::CutFirstAfter(250_000),
        );
        let model = model(&server, &[("weights", &body)]);
        let dir = temp_dir("resume");
        let cancel = CancellationToken::new();

        let first = model.download(&dir, &|_: Stage, _: f32| {}, &cancel);
        assert!(
            matches!(first, Err(CoreError::Download { .. })),
            "{first:?}"
        );
        let kept = fs::metadata(model.dir(&dir).join("weights.part"))
            .expect("partial file")
            .len();
        assert!(kept > 0 && kept < body.len() as u64);

        model
            .download(&dir, &|_: Stage, _: f32| {}, &cancel)
            .expect("resume");

        assert_eq!(
            fs::read(model.dir(&dir).join("weights")).expect("read"),
            body
        );
        assert_eq!(
            *server.ranges.lock().expect("ranges"),
            vec![kept.to_string()]
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_server_that_ignores_ranges_gets_a_fresh_start() {
        let body = content(300_000, 5);
        let server = serve(vec![("weights", body.clone())], Serve::IgnoreRanges);
        let model = model(&server, &[("weights", &body)]);
        let dir = temp_dir("norange");
        fs::create_dir_all(model.dir(&dir)).expect("dir");
        fs::write(model.dir(&dir).join("weights.part"), &body[..1000]).expect("partial");

        model
            .download(&dir, &|_: Stage, _: f32| {}, &CancellationToken::new())
            .expect("download");

        assert_eq!(
            fs::read(model.dir(&dir).join("weights")).expect("read"),
            body
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_downloaded_model_makes_no_request() {
        let body = content(2000, 6);
        let server = serve(vec![("graph", body.clone())], Serve::Ranges);
        let model = model(&server, &[("graph", &body)]);
        let dir = temp_dir("again");
        let cancel = CancellationToken::new();
        model
            .download(&dir, &|_: Stage, _: f32| {}, &cancel)
            .expect("first");
        let after_first = server.requests.load(Ordering::SeqCst);

        model
            .download(&dir, &|_: Stage, _: f32| {}, &cancel)
            .expect("second");

        assert_eq!(server.requests.load(Ordering::SeqCst), after_first);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cancelling_stops_before_any_request() {
        let body = content(2000, 7);
        let server = serve(vec![("graph", body.clone())], Serve::Ranges);
        let model = model(&server, &[("graph", &body)]);
        let dir = temp_dir("cancel");
        let cancel = CancellationToken::new();
        cancel.cancel();

        let result = model.download(&dir, &|_: Stage, _: f32| {}, &cancel);

        assert!(matches!(result, Err(CoreError::Cancelled)), "{result:?}");
        assert_eq!(server.requests.load(Ordering::SeqCst), 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_http_error_is_a_download_error() {
        let server = serve(vec![], Serve::Ranges);
        let model = model(&server, &[("missing", b"x")]);
        let dir = temp_dir("404");

        let result = model.download(&dir, &|_: Stage, _: f32| {}, &CancellationToken::new());

        assert!(
            matches!(result, Err(CoreError::Download { ref reason, .. }) if reason.contains("404")),
            "{result:?}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_removes_the_model_and_tolerates_none() {
        let body = content(100, 8);
        let server = serve(vec![("graph", body.clone())], Serve::Ranges);
        let model = model(&server, &[("graph", &body)]);
        let dir = temp_dir("delete");
        model
            .download(&dir, &|_: Stage, _: f32| {}, &CancellationToken::new())
            .expect("download");

        model.delete(&dir).expect("delete");
        model.delete(&dir).expect("delete again");

        assert!(!model.is_downloaded(&dir));
        assert!(!model.dir(&dir).exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_pinned_model_is_about_171_mb() {
        assert_eq!(HTDEMUCS.size(), 170_747_491);
        assert!(
            HTDEMUCS
                .files
                .iter()
                .all(|file| file.sha256.len() == 64 && file.url.starts_with("https://"))
        );
    }

    /// Downloads the real model from Hugging Face (171 MB). Not run in CI.
    #[test]
    #[ignore = "downloads 171 MB from Hugging Face"]
    fn the_real_model_downloads_and_verifies() {
        let dir = temp_dir("real");
        HTDEMUCS
            .download(&dir, &|_: Stage, _: f32| {}, &CancellationToken::new())
            .expect("download");
        assert!(HTDEMUCS.is_downloaded(&dir));
        fs::remove_dir_all(&dir).ok();
    }
}
