//! The library's OS touch-points: walking a folder, measuring a file, copying
//! one into the app's store, and deleting a copy the app made.
//!
//! Deliberately raw IO and nothing else. Every *decision* — which files a
//! folder admits, what a rescan does about a file it has seen before, where a
//! book lands on a shelf — is `library_core`'s, running in the frontend where
//! the library state lives. This module answers the questions only a process
//! with a filesystem can answer, and hands back the measurements:
//!
//!   * [`scan_folder`] walks a tree and returns [`FoundFile`] rows, already
//!     filtered by the folder's own options, so a folder of forty thousand
//!     screenshots does not cross the wire;
//!   * [`verify_paths`] re-measures addresses the library already holds — what
//!     sets a book `missing`, and what replaces a migrated book's placeholder
//!     fingerprint with a real one;
//!   * [`store_books`] copies into `<app_data_dir>/Library/<format>/`, the only
//!     directory this module ever writes to;
//!   * [`delete_stored`] removes a copy, and refuses anything outside it.
//!
//! Progress is emitted on [`PROGRESS_EVENT`] rather than returned, because
//! walking a large folder takes longer than a UI is willing to look frozen.
//! Completion is NOT emitted: the frontend owns the task list, and a backend
//! that also declared "done" would be a second opinion about a state only one
//! side can see — a scan is usually followed by copies, and a "done" between
//! them would close the card early.
//!
//! The gates here are the crate's existing filesystem gate
//! (`ensure_readable_document`): these commands are reachable from a webview
//! that parses untrusted documents, so a path with no document suffix is
//! refused outright rather than measured, copied or deleted.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use std::io::Read as _;
use tauri::{AppHandle, Emitter, Manager};

use library_core::book::Fingerprint;
use library_core::folder::FolderOpts;
use library_core::hash::{HEAD_BYTES, head_hash, mtime_ms};
use library_core::scan::{FoundFile, store_dir};
// The wire types live in `library-core` because both sides of this IPC depend
// on it: one declaration, and no contract test needed to prove the halves agree.
use library_core::wire::{ImportPhase, ImportProgress, PathCheck, StoreRequest, StoreResult};

/// The Tauri channel every progress beat is emitted on. The frontend's mirror
/// of this name lives in `src/services/library/mod.rs`, which re-broadcasts it as
/// window event so no component ever registers a Tauri listener of its own.
pub const PROGRESS_EVENT: &str = "library://progress";

/// How deep a walk descends. A tree deeper than this is either a loop this walk
/// did not catch or a directory nobody meant to import; either way the answer
/// is to stop rather than to keep going.
const MAX_DEPTH: usize = 12;

/// How many admitted files one scan returns before it gives up. A cap rather
/// than a guess about how big a library may be: past it the JSON crossing the
/// wire is larger than the state it would update, and the honest answer is to
/// ask for a narrower folder.
const MAX_FOUND: usize = 20_000;

/// Progress emits are batched: one every [`EMIT_EVERY`] files, or
/// [`EMIT_INTERVAL_MS`] since the last, whichever comes first. A 2 000-file
/// folder would otherwise push 2 000 messages through IPC in under a second,
/// and the frontend would spend the import repainting a ring.
const EMIT_EVERY: u32 = 8;
const EMIT_INTERVAL_MS: u128 = 60;

/// The throttle one command carries through its walk or its copies. The beat
/// it emits is [`ImportProgress`], shared with the frontend.
struct Progress {
    task: String,
    phase: ImportPhase,
    done: u32,
    total: u32,
    since_emit: u32,
    last: Instant,
}

impl Progress {
    fn new(task: &str, phase: ImportPhase, total: u32) -> Self {
        Self {
            task: task.to_string(),
            phase,
            done: 0,
            total,
            since_emit: 0,
            last: Instant::now(),
        }
    }

    /// Count one file, and emit when the batch is full or the interval has
    /// passed. A dropped emit is not an error: the next beat carries the same
    /// totals, and the final one is always flushed.
    fn tick(&mut self, app: &AppHandle, name: &str) {
        self.done = self.done.saturating_add(1);
        self.since_emit = self.since_emit.saturating_add(1);
        if self.since_emit < EMIT_EVERY && self.last.elapsed().as_millis() < EMIT_INTERVAL_MS {
            return;
        }
        self.flush(app, name);
    }

    fn flush(&mut self, app: &AppHandle, name: &str) {
        self.since_emit = 0;
        self.last = Instant::now();
        let _ = app.emit(
            PROGRESS_EVENT,
            &ImportProgress {
                task: self.task.clone(),
                phase: self.phase,
                done: self.done,
                total: self.total,
                name: name.to_string(),
            },
        );
    }
}

/// Walk `root` and measure every file the folder's options admit.
///
/// Runs on the blocking pool: a walk is a syscall per entry, and a large folder
/// would otherwise hold the async runtime for the whole import.
#[tauri::command]
pub async fn scan_folder(
    app: AppHandle,
    task: String,
    root: String,
    opts: FolderOpts,
) -> Result<Vec<FoundFile>, String> {
    tauri::async_runtime::spawn_blocking(move || scan(&app, &task, &root, &opts))
        .await
        .map_err(|e| format!("scan worker failed: {e}"))?
}

/// One walk, and everything it accumulates. A struct rather than six arguments
/// threaded through a recursive call: the walk is the only place these are
/// touched, and a signature that has to be re-read to be called is a signature
/// that gets called wrong.
struct Scan<'a> {
    app: &'a AppHandle,
    root: &'a Path,
    opts: &'a FolderOpts,
    progress: Progress,
    found: Vec<FoundFile>,
    /// Set once [`MAX_FOUND`] is reached; every level above returns on it, so
    /// the walk unwinds instead of finishing the tree for nothing.
    truncated: bool,
}

impl Scan<'_> {
    /// One directory. Entries are sorted so books land in the order the folder
    /// itself lists them, which is what a reader expects a shelf to look like.
    fn walk(&mut self, dir: &Path, depth: usize) {
        if depth > MAX_DEPTH || self.truncated {
            return;
        }
        let Ok(read) = fs::read_dir(dir) else {
            // An unreadable directory is not a failed import: one subfolder's
            // permissions should not cost the reader the other ninety.
            return;
        };
        let mut entries: Vec<fs::DirEntry> = read.filter_map(Result::ok).collect();
        entries.sort_by_key(fs::DirEntry::file_name);

        for entry in entries {
            if self.truncated {
                return;
            }
            // `file_type` does not follow the link, which is the point: a
            // symlinked directory is a loop this walk has no business entering,
            // and a symlinked file's real address is somewhere else.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                // A hidden directory is somebody's cache, not a bookshelf.
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                self.walk(&path, depth + 1);
            } else if kind.is_file() {
                self.admit(entry, &path);
            }
        }
    }

    /// One file: measure it, ask the folder's options, and keep it if they say
    /// yes.
    fn admit(&mut self, entry: fs::DirEntry, path: &Path) {
        let Ok(meta) = entry.metadata() else {
            return;
        };
        let size = meta.len();
        let ext = extension_of(path);
        if !self.opts.admits_file(&ext, size) {
            return;
        }
        let fp = Fingerprint::of(size, mtime_ms(meta.modified().ok()), &read_head(path));
        let root = self.root;
        let found = FoundFile {
            rel: relative_to(root, path),
            path: path_to_string(path),
            ext,
            size,
            fp,
        };
        self.found.push(found);
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.progress.tick(self.app, &label);
        if self.found.len() >= MAX_FOUND {
            self.truncated = true;
        }
    }
}

fn scan(
    app: &AppHandle,
    task: &str,
    root: &str,
    opts: &FolderOpts,
) -> Result<Vec<FoundFile>, String> {
    let root_path = PathBuf::from(root);
    ensure_walkable(&root_path)?;
    let mut state = Scan {
        app,
        root: &root_path,
        opts,
        progress: Progress::new(task, ImportPhase::Scan, 0),
        found: Vec::new(),
        truncated: false,
    };
    state.walk(&root_path, 0);
    if state.truncated {
        return Err(format!(
            "This folder has more than {MAX_FOUND} documents — try importing a smaller folder."
        ));
    }
    // The last beat carries the real total, so the ring ends on the count
    // rather than on the throttle's last guess.
    let total = state.found.len() as u32;
    state.progress.total = total;
    state.progress.done = total;
    state.progress.flush(app, "");
    Ok(state.found)
}

/// Re-measure a list of addresses the library already holds.
///
/// One row per path asked about, in the order asked, so the caller can zip the
/// answer against its own list. A path that is refused by the document gate,
/// missing, or unreadable answers `exists: false` with zeroed measurements —
/// which is what turns a book `missing` rather than an error the UI has to
/// interpret.
#[tauri::command]
pub async fn verify_paths(paths: Vec<String>) -> Result<Vec<PathCheck>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        paths.iter().map(|path| check_path(path)).collect::<Vec<PathCheck>>()
    })
    .await
    .map_err(|e| format!("verify worker failed: {e}"))
}

fn check_path(path: &str) -> PathCheck {
    let missing = PathCheck {
        path: path.to_string(),
        exists: false,
        size: 0,
        mtime_ms: 0,
        head_hash: 0,
    };
    // The same gate the reader's own file reads pass through: "does this exist
    // and what are its first 8 KiB" is a question about ANY file, so it gets
    // the same answer the reader gets — documents only.
    if crate::ensure_readable_document(path).is_err() {
        return missing;
    }
    let p = Path::new(path);
    let Ok(meta) = fs::metadata(p) else {
        return missing;
    };
    if !meta.is_file() {
        return missing;
    }
    PathCheck {
        path: path.to_string(),
        exists: true,
        size: meta.len(),
        mtime_ms: mtime_ms(meta.modified().ok()),
        head_hash: head_hash(&read_head(p)),
    }
}

/// Copy files into the app's store, one result per request.
///
/// A failure is per-file rather than per-batch: a reader importing a folder
/// with one locked file in it should get the other ninety-nine, plus a line
/// naming the one that did not copy.
#[tauri::command]
pub async fn store_books(
    app: AppHandle,
    task: String,
    requests: Vec<StoreRequest>,
) -> Result<Vec<StoreResult>, String> {
    // No `?` here: the worker's own answer is already the whole result, so the
    // only error this can add is the worker failing to run at all.
    tauri::async_runtime::spawn_blocking(move || store(&app, &task, &requests))
        .await
        .map_err(|e| format!("store worker failed: {e}"))
}

fn store(app: &AppHandle, task: &str, requests: &[StoreRequest]) -> Vec<StoreResult> {
    let mut progress = Progress::new(task, ImportPhase::Copy, requests.len() as u32);
    let mut out = Vec::with_capacity(requests.len());
    for request in requests {
        let name = Path::new(&request.path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(copy_one(app, request, &mut progress, &name));
    }
    if !requests.is_empty() {
        progress.flush(app, "");
    }
    out
}

fn copy_one(
    app: &AppHandle,
    request: &StoreRequest,
    progress: &mut Progress,
    name: &str,
) -> StoreResult {
    let fail = |error: String| StoreResult {
        id: request.id.clone(),
        src: request.path.clone(),
        store: String::new(),
        error: Some(error),
    };
    if crate::ensure_readable_document(&request.path).is_err() {
        return fail(format!("not a document this app may copy: {}", request.path));
    }
    let target = match store_path(app, &request.path, &request.id) {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    if let Some(parent) = target.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return fail(format!("could not create the store directory: {e}"));
    }
    if let Err(e) = fs::copy(&request.path, &target) {
        return fail(format!("could not copy {}: {e}", request.path));
    }
    progress.tick(app, name);
    StoreResult {
        id: request.id.clone(),
        src: request.path.clone(),
        store: path_to_string(&target),
        error: None,
    }
}

/// Remove a file the app itself stored.
///
/// The containment check is the whole safety story: the argument arrives from
/// the webview, and a delete primitive that trusted it would be `rm` with an IPC
/// wrapper. Only a path inside this app's own store directory is removed, and
/// the comparison is on canonicalised paths so a `..` cannot walk out.
#[tauri::command]
pub fn delete_stored(app: AppHandle, path: String) -> Result<(), String> {
    let root = store_root(&app)?;
    let target = PathBuf::from(&path);
    let inside = match (target.canonicalize(), root.canonicalize()) {
        (Ok(t), Ok(r)) => t.starts_with(&r),
        // A store file that is already gone is what the caller wanted; a path
        // that cannot be canonicalised for any other reason is refused rather
        // than guessed at.
        (Err(_), Ok(r)) => target.starts_with(&r) && !target.exists(),
        _ => false,
    };
    if !inside {
        return Err(format!("refusing to delete a file outside the store: {path}"));
    }
    match fs::remove_file(&target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("could not delete {path}: {e}")),
    }
}

/// `<app_data_dir>/Library` — the only directory this module writes to.
fn store_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("Library"))
        .map_err(|e| format!("no app data directory: {e}"))
}

/// Where one source file lands in the store: `<root>/<format>/<stem>_<id>.<ext>`.
///
/// The format directory keeps a store of mixed books navigable in a file
/// manager; the id suffix is what lets two books both called `report.pdf`
/// coexist. The stem is sanitised rather than trusted — it came from a filename
/// on disk, which may carry a separator, a control character, or two hundred
/// characters.
fn store_path(app: &AppHandle, src: &str, id: &str) -> Result<PathBuf, String> {
    let root = store_root(app)?;
    let source = Path::new(src);
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{src} has no file name"))?;
    let ext = extension_of(source);
    let stem = name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(name.as_str());
    let file = format!(
        "{}_{}.{}",
        sanitize_component(stem),
        sanitize_component(id),
        ext
    );
    Ok(root.join(store_dir(&ext)).join(file))
}

/// One path component, made safe to write: separators, characters Windows
/// reserves, and control characters become `_`; the result is trimmed of the
/// dots and spaces that would make it a relative path, capped, and replaced
/// with `"book"` when nothing is left.
fn sanitize_component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let capped: String = cleaned.trim_matches(['.', ' ']).chars().take(60).collect();
    match capped.as_str() {
        "" | "." | ".." => "book".to_string(),
        other => other.to_string(),
    }
}

/// The lower-case extension of a path, without its dot. Empty for a name Rust
/// reads as having none (`Makefile`, and a dotfile like `.gitignore`, whose
/// leading dot does not start an extension) — which the format registry also
/// refuses, so an extension-less file is never admitted, measured or copied.
fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// `path` relative to `root`, with `/` separators on every platform. The
/// subfolder half of this string is what a grouped import cuts its shelves
/// from, so it is normalised here rather than at three call sites.
fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
        .replace('\\', "/")
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The first [`HEAD_BYTES`] of a file, or nothing when it cannot be read. An
/// unreadable head is not a failed scan: the size and the stamp still identify
/// the file, and a book that opens is worth more than a hash that is exact.
fn read_head(path: &Path) -> Vec<u8> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = Vec::new();
    let _ = file.take(HEAD_BYTES as u64).read_to_end(&mut buf);
    buf
}

/// The gate on a walk's starting point: absolute, and a directory. Not the
/// document-suffix gate — a folder is not a document — but the same refusal of
/// a relative path the crate's other filesystem commands apply.
fn ensure_walkable(root: &Path) -> Result<(), String> {
    let text = root.to_string_lossy();
    let looks_absolute = text.starts_with('/')          // POSIX
        || text.starts_with("\\\\")                      // Windows UNC share
        || text.as_bytes().get(1) == Some(&b':');        // Windows drive letter
    if !looks_absolute {
        return Err(format!("refusing to walk a relative path: {text}"));
    }
    match fs::metadata(root) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(format!("not a directory: {text}")),
        Err(e) => Err(format!("cannot read {text}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{extension_of, relative_to, sanitize_component};
    use std::path::Path;

    #[test]
    fn an_extension_is_lower_case_and_has_no_dot() {
        assert_eq!(extension_of(Path::new("/books/Dune.PDF")), "pdf");
        assert_eq!(extension_of(Path::new("/books/notes.markdown")), "markdown");
        assert_eq!(extension_of(Path::new("/books/Makefile")), "");
        // Rust reads a dotfile as having no extension, and no extension is
        // nothing the format registry admits — the two agree without a rule.
        assert_eq!(extension_of(Path::new("/books/.gitignore")), "");
    }

    #[test]
    fn a_relative_path_always_uses_forward_slashes() {
        // The subfolder a shelf is cut from is matched against a persisted
        // ledger key, so it cannot be platform-shaped.
        let root = Path::new("/books");
        assert_eq!(relative_to(root, Path::new("/books/a.pdf")), "a.pdf");
        assert_eq!(
            relative_to(root, Path::new("/books/scifi/deep/a.pdf")),
            "scifi/deep/a.pdf"
        );
        // A path that is not under the root at all still answers with something
        // usable rather than an empty string that would name the root's shelf.
        assert_eq!(relative_to(root, Path::new("/other/a.pdf")), "/other/a.pdf");
    }

    #[test]
    fn a_stored_name_cannot_escape_its_directory() {
        assert_eq!(sanitize_component("dune"), "dune");
        assert_eq!(sanitize_component("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize_component("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_component("..."), "book");
        assert_eq!(sanitize_component(""), "book");
        assert_eq!(sanitize_component("  "), "book");
        assert_eq!(sanitize_component("."), "book");
        assert_eq!(sanitize_component(".."), "book");
        assert!(!sanitize_component("a/b").contains('/'));
    }

    #[test]
    fn a_long_stem_is_capped_not_truncated_into_nothing() {
        let long = "a".repeat(400);
        assert_eq!(sanitize_component(&long).chars().count(), 60);
        assert_eq!(sanitize_component(&format!("{long}.pdf")).chars().count(), 60);
    }

    #[test]
    fn a_control_character_never_reaches_a_file_name() {
        assert_eq!(sanitize_component("a\u{0}b\u{1f}c"), "a_b_c");
    }
}
