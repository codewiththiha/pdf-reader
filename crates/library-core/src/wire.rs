//! The wire contract between the shell's filesystem commands and the frontend
//! that drives them.
//!
//! Both sides depend on this crate, so these types are declared ONCE. That is a
//! deliberate improvement on the app's other wire (the AI chunk stream, whose
//! envelope is written twice — `crates/ai-core/src/types.rs` and
//! `src-tauri/src/ai/schema.rs` — and held together by a contract test): a
//! shared crate does not need a test to prove the two halves agree, because
//! there is only one half.
//!
//! Field names are the serde schema crossing `invoke` and `emit`, so they are
//! the storage contract too — `rename_all = "camelCase"` on everything, which
//! is what the JS side of Tauri's IPC speaks.

use serde::{Deserialize, Serialize};

/// Which half of an import a progress beat belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportPhase {
    /// Walking a folder. The total is not known yet, which is what the dock
    /// reads as "indeterminate".
    Scan,
    /// Copying admitted files into the app's store.
    Copy,
}

/// One progress beat, emitted on the shell's `library://progress` channel and
/// re-broadcast as a window event by `src/services/library/mod.rs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    /// The import run this beat belongs to. Minted by the frontend, echoed
    /// back, so two runs in flight never have their counts mixed.
    pub task: String,
    pub phase: ImportPhase,
    pub done: u32,
    /// `0` during a scan (the count is not known until the walk ends), the
    /// request count during a copy.
    pub total: u32,
    /// The file being worked on — the dock's second line.
    pub name: String,
}

/// What one address resolved to, from `verify_paths`. One row per path asked
/// about, in the order asked, so the caller can zip the answer against its own
/// list without matching on strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathCheck {
    pub path: String,
    /// False for a path that is gone, unreadable, a directory, or refused by
    /// the shell's document gate. Everything below is zero when it is.
    pub exists: bool,
    pub size: u64,
    pub mtime_ms: u64,
    pub head_hash: u32,
}

impl PathCheck {
    /// The measurement as a fingerprint, or `None` when the address did not
    /// resolve — the caller marks that book `missing` rather than re-stamping
    /// it with zeros, which would collide with every other missing book.
    pub fn fingerprint(&self) -> Option<crate::book::Fingerprint> {
        self.exists.then_some(crate::book::Fingerprint {
            size: self.size,
            mtime_ms: self.mtime_ms,
            head_hash: self.head_hash,
        })
    }
}

/// One file to copy into the app's store, for `store_books`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreRequest {
    /// The source address. Must pass the shell's document gate.
    pub path: String,
    /// The book's id, which becomes part of the stored name so two books with
    /// the same title cannot collide in one directory.
    pub id: String,
}

/// What one copy produced. A failure is per-file rather than per-batch: a
/// folder with one locked file in it should still import the other ninety-nine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreResult {
    pub id: String,
    pub src: String,
    /// The stored address, or empty when the copy failed.
    pub store: String,
    pub error: Option<String>,
}

impl StoreResult {
    /// True when the copy landed.
    pub fn is_ok(&self) -> bool {
        self.error.is_none() && !self.store.is_empty()
    }
}

/// The whole library, as the shell hands it over at boot: every book in manual
/// order, every shelf with its members resolved, and every watched folder with its
/// ledger.
///
/// One value rather than three commands because the three are one invariant — a
/// shelf member that names no book is a hole in the grid — and a frontend that
/// fetched them separately would have a window in which that was true.
///
/// The view is NOT here: it is chrome preference, it is needed synchronously at
/// first paint, and it lives in localStorage with the rest of the settings.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    /// Every row the catalog holds, in the order the frontend's own list keeps
    /// them. A catalog answers with book rows only: a link has no table in it
    /// yet, for the reason `src-tauri/src/db/repo.rs`'s `bootstrap` gives.
    pub books: Vec<crate::book::Row>,
    pub shelves: Vec<crate::shelf::Shelf>,
    pub folders: Vec<crate::folder::WatchedFolder>,
}

/// A book's cached cover: page 1, rendered small, as a data URL.
///
/// Kept as a data URL rather than decoded, because that is what the engine hands
/// over and what the `<img>` eats — a migration that transcodes is a migration
/// that takes a minute instead of a few milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cover {
    pub width: f64,
    pub height: f64,
    pub data_url: String,
}

/// One highlight, in the shape the catalog stores it.
///
/// Not `ai_core::gloss::GlossMark`: that type lives in a crate the shell cannot
/// depend on (it is wasm-bound), and the columns here are the ones a full-text
/// index needs — the word and the context it was found in — which the mark's own
/// anchor encoding is none of the database's business. The frontend maps between
/// the two, and the anchor crosses as the JSON it already persists as.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlossRow {
    pub id: String,
    pub book_id: String,
    /// The page the mark is on. `0` for a reflowable mark, whose identity is the
    /// envelope in `context` rather than a page — its pages are re-cut whenever
    /// the typography moves.
    pub page: i64,
    pub word: String,
    pub context: String,
    /// The anchor, as the JSON the mark already carries.
    pub anchor_json: String,
    pub created_ms: u64,
}

/// One library search result: which book, how well it matched, and — for a hit
/// that came out of a highlight rather than the catalog — the fragment that
/// matched, since "the book where I underlined this" is only useful if you can see
/// the bit you underlined.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: String,
    /// FTS5's rank: more negative is a better match. Carried rather than dropped
    /// so a caller unioning catalog hits with highlight hits can order the two.
    pub rank: f64,
    #[serde(default)]
    pub snippet: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_beat_crosses_the_wire_in_camel_case() {
        let beat = ImportProgress {
            task: "t1".into(),
            phase: ImportPhase::Copy,
            done: 12,
            total: 48,
            name: "dune.pdf".into(),
        };
        let json = serde_json::to_string(&beat).unwrap();
        assert!(json.contains("\"phase\":\"copy\""), "{json}");
        // No snake_case keys: the JS side of Tauri's IPC speaks camelCase, and
        // a mismatched name deserialises as a default rather than as an error.
        assert!(!json.contains('_'), "{json}");
        let back: ImportProgress = serde_json::from_str(&json).unwrap();
        assert_eq!(back, beat);
    }

    #[test]
    fn a_path_check_only_becomes_a_fingerprint_when_it_resolved() {
        let live = PathCheck {
            path: "/books/a.pdf".into(),
            exists: true,
            size: 10,
            mtime_ms: 20,
            head_hash: 30,
        };
        assert_eq!(
            live.fingerprint(),
            Some(crate::book::Fingerprint {
                size: 10,
                mtime_ms: 20,
                head_hash: 30
            })
        );
        let gone = PathCheck {
            exists: false,
            ..live
        };
        assert_eq!(gone.fingerprint(), None);
    }

    #[test]
    fn the_path_check_parses_the_shape_the_shell_emits() {
        let check: PathCheck = serde_json::from_str(
            r#"{"path":"/a.pdf","exists":true,"size":1,"mtimeMs":2,"headHash":3}"#,
        )
        .unwrap();
        assert_eq!(check.mtime_ms, 2);
        assert_eq!(check.head_hash, 3);
    }

    #[test]
    fn a_store_result_is_only_ok_when_it_has_an_address() {
        let ok = StoreResult {
            id: "b1".into(),
            src: "/downloads/a.pdf".into(),
            store: "/app/Library/pdf/a_b1.pdf".into(),
            error: None,
        };
        assert!(ok.is_ok());
        let failed = StoreResult {
            store: String::new(),
            error: Some("locked".into()),
            ..ok.clone()
        };
        assert!(!failed.is_ok());
        // An empty address with no error is still not a copy that landed.
        let empty = StoreResult {
            id: "b1".into(),
            src: "/downloads/a.pdf".into(),
            store: String::new(),
            error: None,
        };
        assert!(!empty.is_ok());
        assert!(ok.is_ok());
    }

    #[test]
    fn a_snapshot_carries_all_three_halves_of_the_library() {
        let snapshot = LibrarySnapshot::default();
        assert!(snapshot.books.is_empty() && snapshot.shelves.is_empty());
        let json = serde_json::to_string(&snapshot).unwrap();
        assert_eq!(json, "{\"books\":[],\"shelves\":[],\"folders\":[]}");
        let back: LibrarySnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, snapshot);
    }

    #[test]
    fn a_highlight_crosses_with_the_columns_the_index_needs() {
        let row = GlossRow {
            id: "m1".into(),
            book_id: "b1".into(),
            page: 12,
            word: "palimpsest".into(),
            context: "a palimpsest of earlier drafts".into(),
            anchor_json: "{\"x\":1,\"y\":2,\"w\":3,\"h\":4,\"r\":5}".into(),
            created_ms: 7,
        };
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains("\"bookId\""), "{json}");
        assert!(json.contains("\"anchorJson\""), "{json}");
        assert!(!json.contains('_'), "{json}");
        let back: GlossRow = serde_json::from_str(&json).unwrap();
        assert_eq!(back, row);
    }

    #[test]
    fn a_cover_is_the_shape_the_engine_hands_over() {
        let cover = Cover {
            width: 240.0,
            height: 320.0,
            data_url: "data:image/jpeg;base64,x".into(),
        };
        let json = serde_json::to_string(&cover).unwrap();
        assert!(json.contains("\"dataUrl\""), "{json}");
        let back: Cover = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cover);
    }

    #[test]
    fn a_search_hit_may_carry_the_fragment_that_matched() {
        let catalog = SearchHit {
            id: "b1".into(),
            rank: -1.5,
            snippet: None,
        };
        let highlight = SearchHit {
            id: "b2".into(),
            rank: -0.5,
            snippet: Some("…a «palimpsest» of earlier drafts…".into()),
        };
        for hit in [&catalog, &highlight] {
            let json = serde_json::to_string(hit).unwrap();
            let back: SearchHit = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, hit);
        }
        // A hit from before the highlight index existed has no snippet.
        let older: SearchHit = serde_json::from_str(r#"{"id":"b1","rank":-1.0}"#).unwrap();
        assert_eq!(older.snippet, None);
    }

    #[test]
    fn a_store_request_round_trips() {
        let request = StoreRequest {
            path: "/downloads/a.pdf".into(),
            id: "b1".into(),
        };
        let json = serde_json::to_string(&request).unwrap();
        let back: StoreRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, request);
    }
}
