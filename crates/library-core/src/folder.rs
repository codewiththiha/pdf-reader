//! A watched folder: the options an import was made with, and the ledger of
//! what that import already did.
//!
//! The ledger half is the reason this is a struct and not a settings row. A
//! rescan runs on every window focus, and "which files did I already place"
//! is not the same question as "which files are in the library": a book the
//! reader dragged off a folder shelf is still in the library, and re-adding it
//! would undo the arrangement; a book the reader deliberately removed is not
//! in the library, and re-adding it would ignore the removal. Both answers
//! live here, per folder, and [`crate::ledger`] reads them.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use reader_core::format::Format;

use crate::book::{Book, Fingerprint};
use crate::scan::{FoundFile, admits, selectable_formats};

/// The default size threshold the import sheet opens on, in bytes: 30 KB. A
/// PDF smaller than that is a stub, a placeholder or a corrupt download, and a
/// text file smaller than that is rarely a book — but the number is a default,
/// not a rule, and the sheet shows it in KB because that is how a reader
/// thinks about it.
pub const DEFAULT_MIN_SIZE: u64 = 30 * 1024;

/// The step the sheet's −/+ buttons move the threshold by, and its bounds.
pub const MIN_SIZE_STEP: u64 = 10 * 1024;
pub const MIN_SIZE_FLOOR: u64 = 0;
pub const MIN_SIZE_CEIL: u64 = 500 * 1024;

/// How one folder is scanned. Every field is a choice the import sheet offers,
/// and every one of them is honoured on EVERY later rescan — editing an option
/// after the fact changes what the next scan admits, and never removes a book
/// the previous scan already placed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderOpts {
    /// The formats the selection names. Always a subset of
    /// [`selectable_formats`]; the sheet offers exactly those.
    #[serde(default = "default_formats")]
    pub formats: BTreeSet<Format>,
    /// `true` = only these formats; `false` = everything but these. One set
    /// and a flip rather than two lists that can contradict each other.
    #[serde(default = "default_true")]
    pub include_selected: bool,
    /// Strict lower bound in bytes: a file of exactly this size is refused.
    #[serde(default = "default_min_size")]
    pub min_size: u64,
    /// Read in place. `true` links each book to the address it was found at;
    /// `false` copies it into the app's store. Default `true`, because linking
    /// is the only thing this app ever did and the copy is the new behaviour —
    /// a reader who upgrades keeps the library they had.
    #[serde(default = "default_true")]
    pub in_place: bool,
    /// Rescan this folder when the app opens or regains focus. Only offered
    /// alongside [`FolderOpts::in_place`] in the sheet (a store copy does not
    /// care what the source folder does next), but honoured independently
    /// here: the ledger's job is the same either way.
    #[serde(default)]
    pub watch: bool,
    /// Cut a shelf per subfolder (`true`) or keep the whole tree on one shelf.
    #[serde(default = "default_true")]
    pub groups: bool,
}

fn default_formats() -> BTreeSet<Format> {
    selectable_formats().into_iter().collect()
}

fn default_true() -> bool {
    true
}

fn default_min_size() -> u64 {
    DEFAULT_MIN_SIZE
}

impl Default for FolderOpts {
    fn default() -> Self {
        Self {
            formats: default_formats(),
            include_selected: true,
            min_size: DEFAULT_MIN_SIZE,
            in_place: true,
            watch: false,
            groups: true,
        }
    }
}

impl FolderOpts {
    /// The threshold stepped by one press of the sheet's −/+, clamped to the
    /// bounds the sheet shows. `delta` is in steps, not bytes, so the caller
    /// cannot invent a value the control could not have produced.
    pub fn step_min_size(&mut self, delta: i32) {
        let steps = delta as i64;
        let next = self.min_size as i64 + steps * MIN_SIZE_STEP as i64;
        self.min_size = next
            .clamp(MIN_SIZE_FLOOR as i64, MIN_SIZE_CEIL as i64)
            as u64;
    }

    /// A size in bytes, as the sheet prints it. Whole KB drops the decimals so
    /// the control reads "30 KB" and not "30.0 KB".
    pub fn min_size_label(&self) -> String {
        if self.min_size.is_multiple_of(1024) {
            format!("{} KB", self.min_size / 1024)
        } else {
            format!("{:.1} KB", self.min_size as f64 / 1024.0)
        }
    }

    /// [`admits`] bound to these options, for the walk that filters as it goes.
    pub fn admits_file(&self, ext: &str, size: u64) -> bool {
        admits(self, ext, size)
    }
}

/// One folder the library watches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolder {
    pub id: String,
    /// The absolute path the walk starts at. Never rewritten by the app: a
    /// moved folder is a missing folder, and the sheet offers a new path
    /// rather than guessing.
    pub root: String,
    #[serde(default)]
    pub opts: FolderOpts,
    /// Fingerprints THIS folder has already placed. Membership is what makes a
    /// rescan honest: a book the reader moved to another shelf, or off the
    /// folder's shelf entirely, keeps its fingerprint here, so the next scan
    /// skips it instead of putting it back.
    #[serde(default)]
    pub placed: HashSet<Fingerprint>,
    /// The books the reader deliberately removed from the library. A tombstone
    /// per removal: the file is still on disk and still admitted by `opts`, so
    /// without one the next rescan would re-add exactly what was just deleted.
    /// Only written for books this folder placed.
    ///
    /// A list of records rather than a set of fingerprints, because a tombstone
    /// has a second job: it is what the folder's import menu reads to offer the
    /// book back. A set could say "not this one again" and nothing more.
    #[serde(default)]
    pub ignored: Vec<Tombstone>,
    /// What the latest scan saw, restricted to the fingerprints this folder has
    /// placed: fingerprint to the address it was found at.
    ///
    /// The import menu's other half. "Did a book I filed here move somewhere
    /// else?" is answerable from this and the library's membership lists alone,
    /// which is what lets the menu open instantly instead of walking the tree
    /// again — and the restriction to placed fingerprints is what bounds it: a
    /// folder cannot have placed more books than the library holds.
    #[serde(default)]
    pub last_seen: Vec<(Fingerprint, String)>,
    /// Subfolder (relative, `/`-separated, `""` for the root) to the shelf its
    /// books were placed on. Persisted so a rescan adds to the shelf the last
    /// one created rather than making a second shelf with the same name.
    #[serde(default)]
    pub shelf_map: BTreeMap<String, String>,
    /// When this folder was last rescanned, in milliseconds since the epoch.
    /// `0` until the first scan completes. Diagnostic only — no decision reads
    /// it, which is why a stale stamp can never suppress a scan.
    #[serde(default)]
    pub scanned_ms: u64,
}

/// Every rung of a shelf key's path, root first and the key itself last: `""`,
/// then `"2"`, then `"2/deep"`. The root rung is always first because the watched
/// folder's own shelf is the top of every chain it mints.
pub fn key_chain(key: &str) -> Vec<&str> {
    let mut out = vec![""];
    if key.is_empty() {
        return out;
    }
    for (at, _) in key.match_indices('/') {
        out.push(&key[..at]);
    }
    out.push(key);
    out
}

/// The rung a shelf key sits inside: `"2/deep"` is inside `"2"`, `"2"` is inside
/// the root, and the root is inside nothing. What a rescan re-hangs a folder
/// shelf's `parent` from.
pub fn parent_key(key: &str) -> Option<&str> {
    if key.is_empty() {
        return None;
    }
    match key.rfind('/') {
        Some(at) => Some(&key[..at]),
        None => Some(""),
    }
}

impl WatchedFolder {
    /// The ledger key for a found file: its subfolder when the folder groups,
    /// the empty string for the root otherwise. One function owns the choice so
    /// the walk, the shelf creation and the persisted map cannot disagree about
    /// it.
    ///
    /// Owned rather than borrowed because the caller goes straight from this to
    /// [`WatchedFolder::shelf_chain_for`], which takes `&mut self`: a key borrowed
    /// from the folder would still be alive when the folder is mutated.
    pub fn shelf_key(&self, found: &FoundFile) -> String {
        if self.opts.groups {
            found.subfolder().to_string()
        } else {
            String::new()
        }
    }

    /// The shelf a found file belongs on, minting EVERY rung between the folder's
    /// root shelf and the file's own subfolder, and reporting each rung it mints
    /// through `made` (rung, id, name, the parent id above it) so the caller can
    /// put a shelf row under the id.
    ///
    /// A walk reports files, not directories: minting only the leaf would hang a
    /// subfolder's shelf off the root with a hole above it, and a library that
    /// showed the tree flat beside the tree nested was two logics wearing one
    /// shelf list. The chain is minted level by level instead, so an intermediate
    /// directory with no books of its own is an empty folder card rather than a
    /// missing rung, and the shelf at every depth is the directory at that depth.
    ///
    /// Rungs already in [`WatchedFolder::shelf_map`] are reused rather than
    /// re-minted, which is what makes a rescan continue the tree instead of
    /// growing a twin beside it. Called from the frontend, which owns the shelf
    /// list and the id sequence; the shell's walk only reports what it found.
    pub fn shelf_chain_for(
        &mut self,
        key: &str,
        mut mint: impl FnMut(&str) -> String,
        mut name_of: impl FnMut(&str) -> String,
        mut made: impl FnMut(&str, &str, String, Option<String>),
    ) -> String {
        let mut current: Option<String> = None;
        let mut id = String::new();
        for rung in key_chain(key) {
            id = match self.shelf_map.get(rung) {
                Some(known) => known.clone(),
                None => {
                    let fresh = mint(rung);
                    self.shelf_map.insert(rung.to_string(), fresh.clone());
                    made(rung, &fresh, name_of(rung), current.clone());
                    fresh
                }
            };
            current = Some(id.clone());
        }
        id
    }

    /// Record that this folder placed a file, so the next rescan skips it.
    pub fn mark_placed(&mut self, fp: Fingerprint) {
        self.placed.insert(fp);
    }

    /// Whether this folder is holding a removal against `fp` — the question a
    /// rescan asks before any other. A tombstone wins over everything: the
    /// reader said no, and the file being unchanged since is not a new
    /// argument.
    pub fn is_ignored(&self, fp: &Fingerprint) -> bool {
        self.ignored.iter().any(|entry| &entry.fp == fp)
    }

    /// Remember what this scan saw, for the fingerprints this folder placed.
    ///
    /// Written on every scan, including one that changed nothing: the menu's
    /// "moved out of this folder" answer is only as fresh as the last walk, and a
    /// walk that found nothing to do is still a walk that saw every file.
    pub fn record_seen(&mut self, found: &[FoundFile]) {
        let seen: Vec<(Fingerprint, String)> = found
            .iter()
            .filter(|file| self.placed.contains(&file.fp))
            .map(|file| (file.fp, file.path.clone()))
            .collect();
        self.last_seen = seen;
    }
}

/// A book the reader removed from the library, remembered by the folder that
/// placed it.
///
/// Two jobs, and the second is why it carries more than a fingerprint: it keeps
/// the file out of every later rescan, and it is the record the folder's import
/// menu reads to offer the book back — with a name, a size and an address to
/// re-measure before it promises anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tombstone {
    /// The fingerprint the book carried when it was removed. What a rescan
    /// matches against, and what a restore looks up by.
    pub fp: Fingerprint,
    /// The name the shelf showed. `None` for a book that was imported and never
    /// opened, which is most of them; the menu falls back to the file's stem.
    #[serde(default)]
    pub title: Option<String>,
    pub format: Format,
    /// Where the file lived when it was removed. A restore re-measures this
    /// address first: a file that has since moved is a relink, not a restore, and
    /// the menu says so rather than importing a path that is not there.
    pub last_path: String,
    /// The shelf it was filed on, when it was filed on one. A restore puts it
    /// back there if the shelf still exists, and on the folder's root shelf if it
    /// does not — a removed book should not come back somewhere new.
    #[serde(default)]
    pub shelf_id: Option<String>,
    /// When it was removed, in milliseconds since the epoch.
    #[serde(default)]
    pub removed_ms: u64,
    /// True when the removal was a MOVE and not a deletion: the book left its
    /// folder as the library's own stored copy, so the library still holds it
    /// and the folder's restore menu must not offer it back as a book that is
    /// gone. The file on disk is untouched — this log is about the shelf, not
    /// about the filesystem.
    #[serde(default)]
    pub moved: bool,
    /// The row that represents this file to the folder, when one came home: a
    /// stored copy moved back onto a shelf this folder owns under the name the
    /// log remembers binds itself here, and a later import of the file lights
    /// that row up instead of minting a linked neighbour beside it. `None`
    /// until a return binds it, and stale ids are checked against the library
    /// before they are trusted.
    #[serde(default)]
    pub returned_row: Option<String>,
}

impl Tombstone {
    /// The record for a book about to be removed. `shelf_id` is the first of the
    /// folder's shelves the book was on, if any — one answer, deterministically
    /// chosen, because a book on three of a folder's shelves still comes back to
    /// one.
    pub fn of(book: &Book, shelf_id: Option<String>, now_ms: u64) -> Self {
        Self {
            fp: book.fp,
            title: book.title.clone(),
            format: book.format,
            last_path: book.path().to_string(),
            shelf_id,
            removed_ms: now_ms,
            moved: false,
            returned_row: None,
        }
    }

    /// What a restore row calls the book: its own title, else the file's stem.
    pub fn label(&self) -> String {
        crate::text::display_or_stem(self.title.as_deref(), &self.last_path)
    }
}

/// Make a persisted folder list internally valid: drop rows with no id or no
/// root, dedupe by root (first wins), clamp the size threshold into the range
/// the sheet can produce, and empty a format set that would admit nothing.
/// Idempotent.
pub fn sanitize(folders: &mut Vec<WatchedFolder>) {
    let mut seen = HashSet::new();
    folders.retain(|f| {
        !f.id.trim().is_empty() && !f.root.trim().is_empty() && seen.insert(f.root.clone())
    });
    for f in folders.iter_mut() {
        f.opts.min_size = f
            .opts
            .min_size
            .clamp(MIN_SIZE_FLOOR, MIN_SIZE_CEIL);
        if f.opts.formats.is_empty() {
            f.opts.formats = default_formats();
        }
        // A watch on a folder that is not read in place still means something
        // (the source may gain a file worth copying), so it is left alone; the
        // sheet simply does not offer it there.
        f.shelf_map.retain(|k, v| !v.trim().is_empty() && !k.contains('\\'));
        // One tombstone per fingerprint: a book removed twice (it can happen —
        // restore it, then remove it again) must not leave two rows offering the
        // same file back, and the newest is the one that knows where it last was.
        let mut stones = HashSet::new();
        f.ignored.retain(|t| !t.last_path.trim().is_empty() && stones.insert(t.fp));
        let mut seen = HashSet::new();
        f.last_seen.retain(|(fp, path)| !path.trim().is_empty() && seen.insert(*fp));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_a_chain_of_rungs_root_first() {
        assert_eq!(key_chain(""), vec![""]);
        assert_eq!(key_chain("2"), vec!["", "2"]);
        assert_eq!(key_chain("2/deep"), vec!["", "2", "2/deep"]);
        assert_eq!(parent_key(""), None);
        assert_eq!(parent_key("2"), Some(""));
        assert_eq!(parent_key("2/deep"), Some("2"));
    }

    fn fp(n: u32) -> Fingerprint {
        Fingerprint {
            size: u64::from(n),
            mtime_ms: u64::from(n),
            head_hash: n,
        }
    }

    fn folder(root: &str) -> WatchedFolder {
        WatchedFolder {
            id: "f1".into(),
            root: root.into(),
            opts: FolderOpts::default(),
            placed: HashSet::new(),
            ignored: Vec::new(),
            shelf_map: BTreeMap::new(),
            last_seen: Vec::new(),
            scanned_ms: 0,
        }
    }

    fn stone(n: u32) -> Tombstone {
        Tombstone {
            fp: fp(n),
            title: Some(format!("Book {n}")),
            format: Format::Pdf,
            last_path: format!("/books/{n}.pdf"),
            shelf_id: None,
            removed_ms: 5,
            moved: false,
            returned_row: None,
        }
    }

    #[test]
    fn the_defaults_are_the_ones_the_sheet_opens_on() {
        let o = FolderOpts::default();
        assert_eq!(o.min_size, DEFAULT_MIN_SIZE);
        assert_eq!(o.min_size_label(), "30 KB");
        assert!(o.include_selected);
        assert!(o.in_place, "read in place is the mode the app always had");
        assert!(!o.watch, "watching is opt-in");
        assert!(o.groups);
        assert_eq!(o.formats.len(), selectable_formats().len());
    }

    #[test]
    fn a_blob_from_before_the_folder_options_existed_loads_them() {
        let f: WatchedFolder =
            serde_json::from_str(r#"{"id":"f1","root":"/books"}"#).unwrap();
        assert_eq!(f.opts, FolderOpts::default());
        assert!(f.placed.is_empty() && f.ignored.is_empty());
        assert!(f.shelf_map.is_empty());
    }

    #[test]
    fn the_size_dial_steps_in_kb_and_stops_at_its_bounds() {
        let mut o = FolderOpts::default();
        o.step_min_size(1);
        assert_eq!(o.min_size, 40 * 1024);
        o.step_min_size(-2);
        assert_eq!(o.min_size, 20 * 1024);
        for _ in 0..100 {
            o.step_min_size(-1);
        }
        assert_eq!(o.min_size, MIN_SIZE_FLOOR);
        assert_eq!(o.min_size_label(), "0 KB");
        for _ in 0..200 {
            o.step_min_size(1);
        }
        assert_eq!(o.min_size, MIN_SIZE_CEIL);
        assert_eq!(o.min_size_label(), "500 KB");
    }

    #[test]
    fn a_sub_thousand_byte_threshold_still_prints_honestly() {
        let mut o = FolderOpts::default();
        o.min_size = 512;
        assert_eq!(o.min_size_label(), "0.5 KB");
    }

    #[test]
    fn the_ledger_skips_what_it_placed_and_honours_a_tombstone() {
        let mut f = folder("/books");
        assert!(!f.is_ignored(&fp(1)));
        f.mark_placed(fp(1));
        assert!(f.placed.contains(&fp(1)));
        // Placing is not a tombstone: the book is on a shelf, so a rescan
        // skips it through `placed`, and removing it later still has to stick.
        assert!(!f.is_ignored(&fp(1)));
        f.ignored.push(stone(1));
        assert!(f.is_ignored(&fp(1)), "a removal outranks everything");
    }

    #[test]
    fn grouping_decides_whether_a_subfolder_is_its_own_shelf() {
        let mut f = folder("/books");
        let found = FoundFile {
            path: "/books/scifi/dune.pdf".into(),
            rel: "scifi/dune.pdf".into(),
            ext: "pdf".into(),
            size: 1,
            fp: fp(1),
        };
        assert_eq!(f.shelf_key(&found), "scifi");
        f.opts.groups = false;
        assert_eq!(f.shelf_key(&found), "", "one flat shelf for the whole tree");
        // A file at the root of a grouped folder is on the folder's own shelf.
        f.opts.groups = true;
        let at_root = FoundFile { rel: "dune.pdf".into(), ..found };
        assert_eq!(f.shelf_key(&at_root), "");
    }

    #[test]
    fn the_shelf_map_reuses_the_shelf_it_minted() {
        let mut f = folder("/books");
        let mut made: Vec<(String, String, String, Option<String>)> = Vec::new();
        let mut seq = 0usize;
        // A file two subfolders deep mints the WHOLE chain — the folder's root
        // shelf, the rung under it, and the leaf — reporting each rung with the
        // parent above it, so an intermediate directory with no books of its
        // own is an empty folder card rather than a missing rung.
        let leaf = f.shelf_chain_for(
            "scifi/deep",
            |_| {
                seq += 1;
                format!("s{seq}")
            },
            |rung| rung.rsplit('/').next().unwrap_or(rung).to_string(),
            |rung, id, name, parent| made.push((rung.to_string(), id.to_string(), name, parent)),
        );
        assert_eq!(
            made.iter().map(|(rung, _, _, _)| rung.as_str()).collect::<Vec<_>>(),
            vec!["", "scifi", "scifi/deep"]
        );
        assert_eq!(made[2].2, "deep", "the leaf is named by its own subfolder");
        assert_eq!(made[0].3, None, "the folder's own shelf hangs at the level it was on");
        assert_eq!(made[1].3.as_deref(), Some(made[0].1.as_str()));
        assert_eq!(made[2].3.as_deref(), Some(made[1].1.as_str()));
        assert_eq!(leaf, made[2].1);
        assert_eq!(f.shelf_map.len(), 3);

        // The second file reuses every rung it shares with the first: a rescan
        // never mints a second shelf for a subfolder, and mints only the rung
        // that is genuinely new.
        made.clear();
        let again = f.shelf_chain_for(
            "scifi/deep",
            |_| {
                seq += 1;
                format!("s{seq}")
            },
            |rung| rung.to_string(),
            |rung, id, name, parent| made.push((rung.to_string(), id.to_string(), name, parent)),
        );
        assert_eq!(again, leaf);
        assert!(made.is_empty(), "no rung was minted, so none was reported");

        let sibling = f.shelf_chain_for(
            "scifi/deep/er",
            |_| {
                seq += 1;
                format!("s{seq}")
            },
            |rung| rung.to_string(),
            |rung, id, name, parent| made.push((rung.to_string(), id.to_string(), name, parent)),
        );
        assert_eq!(made.len(), 1, "only the new leaf");
        assert_eq!(made[0].3.as_deref(), Some(leaf.as_str()));
        assert_ne!(sibling, leaf);
    }

    #[test]
    fn sanitize_dedupes_roots_and_clamps_the_dial() {
        let mut folders = vec![
            WatchedFolder {
                opts: FolderOpts {
                    min_size: 10_000_000,
                    ..FolderOpts::default()
                },
                ..folder("/books")
            },
            folder("/books"),
            folder(""),
            WatchedFolder {
                id: " ".into(),
                ..folder("/other")
            },
        ];
        sanitize(&mut folders);
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].opts.min_size, MIN_SIZE_CEIL);
    }

    #[test]
    fn an_empty_format_set_is_not_a_folder_that_admits_nothing() {
        // A hand-edited blob, or a future format removed from the registry,
        // must not silently turn a watched folder into a dead one.
        let mut folders = vec![WatchedFolder {
            opts: FolderOpts {
                formats: BTreeSet::new(),
                ..FolderOpts::default()
            },
            ..folder("/books")
        }];
        sanitize(&mut folders);
        assert_eq!(folders[0].opts.formats.len(), selectable_formats().len());
    }

    #[test]
    fn a_backslash_never_survives_into_the_shelf_map() {
        // `rel` is normalised to `/` by the walk, so a `\` in a key means the
        // blob was written by something that did not normalise it — and that
        // key would never match a found file again.
        let mut folders = vec![WatchedFolder {
            shelf_map: BTreeMap::from([
                ("scifi".to_string(), "s1".to_string()),
                ("scifi\\deep".to_string(), "s2".to_string()),
            ]),
            ..folder("/books")
        }];
        sanitize(&mut folders);
        let keys: Vec<&String> = folders[0].shelf_map.keys().collect();
        assert_eq!(keys, vec!["scifi"]);
    }
}
