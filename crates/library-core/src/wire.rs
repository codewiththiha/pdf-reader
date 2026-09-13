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

/// One stored copy to move into its own item folder, for `relocate_stored`.
///
/// The old flat store named a copy after the file it came from
/// (`<root>/<format>/<stem>_<id>.<ext>`); the layout in [`crate::store`] names it
/// after the book (`<root>/items/<id>/source.<ext>`). Copies made before that
/// change keep the address recorded in their row and still open, but nothing
/// writes that shape any more, so a one-time pass brings them across.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateRequest {
    /// The address the row currently holds. Must be inside the app's store.
    pub from: String,
    /// The book's id, which names the item folder the copy moves into.
    pub id: String,
}

/// What a relocation pass produced: one row per request, plus the store root the
/// shell moved them inside.
///
/// The root rides along because the frontend cannot compute it — `<app_data_dir>`
/// is the shell's answer — and it needs it to tell a copy that still sits in the
/// old flat bucket from one already in its item folder. Asking for it is a
/// second command and a second round trip; the pass that does the moving already
/// has it in hand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateResult {
    /// The app's store root, `<app_data_dir>/Library`, or empty when the shell
    /// has none — in which case no row moved and nothing is a candidate.
    pub root: String,
    /// One answer per request, in the order asked.
    pub results: Vec<StoreResult>,
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
    fn a_relocation_crosses_the_wire_in_camel_case_and_comes_back_in_order() {
        let requests = [RelocateRequest {
            from: "/app/Library/pdf/dune_ab12.pdf".into(),
            id: "ab12".into(),
        }];
        let json = serde_json::to_string(&requests).unwrap();
        assert!(json.contains("\"from\""), "{json}");
        assert!(json.contains("\"id\""), "{json}");
        // The keys are the contract; the values are paths a reader owns, which
        // carry underscores of their own, so the check names the keys it wants.
        assert!(!json.contains("\"from_\""), "no snake_case keys: {json}");
        assert!(!json.contains("\"_id\""), "no snake_case keys: {json}");
        assert!(json.starts_with("[{\"from\""), "{json}");
        let back: Vec<RelocateRequest> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, requests);

        // The answer carries the root beside the rows, because the frontend
        // cannot compute `<app_data_dir>` itself and needs it to recognise a copy
        // that has not moved yet.
        let answer: RelocateResult = serde_json::from_str(
            r#"{"root":"/app/Library","results":[
                {"id":"ab12","src":"/app/Library/pdf/dune_ab12.pdf",
                 "store":"/app/Library/items/ab12/source.pdf","error":null}]}"#,
        )
        .unwrap();
        assert_eq!(answer.root, "/app/Library");
        assert_eq!(answer.results.len(), 1);
        assert!(answer.results[0].is_ok());
        // A shell with no app-data directory answers with an empty root and no
        // row moved, which is what tells the pass to stop rather than retry.
        let none: RelocateResult =
            serde_json::from_str(r#"{"root":"","results":[]}"#).unwrap();
        assert!(none.root.is_empty() && none.results.is_empty());
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
    }
}
