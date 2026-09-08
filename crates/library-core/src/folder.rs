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

use crate::book::Fingerprint;
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
    /// Fingerprints the reader deliberately removed from the library. A
    /// tombstone: the file is still on disk and still admitted by `opts`, so
    /// without this the next rescan would re-add exactly what was just
    /// deleted. Only written for books this folder placed.
    #[serde(default)]
    pub ignored: HashSet<Fingerprint>,
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

impl WatchedFolder {
    /// The ledger key for a found file: its subfolder when the folder groups,
    /// the empty string for the root otherwise. One function owns the choice so
    /// the walk, the shelf creation and the persisted map cannot disagree about
    /// it.
    ///
    /// Owned rather than borrowed because the caller goes straight from this to
    /// [`WatchedFolder::shelf_for`], which takes `&mut self`: a key borrowed
    /// from the folder would still be alive when the folder is mutated.
    pub fn shelf_key(&self, found: &FoundFile) -> String {
        if self.opts.groups {
            found.subfolder().to_string()
        } else {
            String::new()
        }
    }

    /// The shelf a found file belongs on, minting one through `mint` when this
    /// is the first file from that subfolder and remembering it in
    /// [`WatchedFolder::shelf_map`] so the next rescan reuses it. Called from
    /// the frontend, which owns the shelf list and the id sequence; the shell's
    /// walk only reports what it found.
    pub fn shelf_for(&mut self, key: &str, mint: impl FnOnce(&str) -> String, name: &str) -> String {
        if let Some(id) = self.shelf_map.get(key) {
            return id.clone();
        }
        let id = mint(name);
        self.shelf_map.insert(key.to_string(), id.clone());
        id
    }

    /// Record that this folder placed a file, so the next rescan skips it.
    pub fn mark_placed(&mut self, fp: Fingerprint) {
        self.placed.insert(fp);
    }

    /// Whether a rescan may place this fingerprint. A tombstone wins over
    /// everything: the reader said no, and the file being unchanged since is
    /// not a new argument.
    pub fn may_place(&self, fp: &Fingerprint) -> bool {
        !self.ignored.contains(fp)
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            ignored: HashSet::new(),
            shelf_map: BTreeMap::new(),
            scanned_ms: 0,
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
        assert!(f.may_place(&fp(1)));
        f.mark_placed(fp(1));
        assert!(f.placed.contains(&fp(1)));
        // Placing is not a tombstone: the book is on a shelf, so a rescan
        // skips it through `placed`, and removing it later still has to stick.
        assert!(f.may_place(&fp(1)));
        f.ignored.insert(fp(1));
        assert!(!f.may_place(&fp(1)), "a removal outranks everything");
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
        let first = f.shelf_for("scifi", |_| "s1".to_string(), "scifi");
        let again = f.shelf_for("scifi", |_| "s2".to_string(), "scifi");
        assert_eq!(first, "s1");
        assert_eq!(again, "s1", "a rescan never mints a second shelf for a subfolder");
        assert_eq!(f.shelf_map.len(), 1);
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
