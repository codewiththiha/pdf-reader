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

impl ImportProgress {
    /// The beat's fraction complete, or `None` while the total is unknown. One
    /// definition so the ring and the label cannot disagree about what "half
    /// way" means.
    pub fn fraction(&self) -> Option<f64> {
        if self.total == 0 {
            return None;
        }
        Some((f64::from(self.done) / f64::from(self.total)).clamp(0.0, 1.0))
    }

    /// The percentage the ring prints, when there is one to print.
    pub fn percent(&self) -> Option<u32> {
        self.fraction().map(|f| (f * 100.0).round() as u32)
    }
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
    fn a_scan_beat_has_no_total_and_says_so() {
        let beat = ImportProgress {
            task: "t1".into(),
            phase: ImportPhase::Scan,
            done: 300,
            total: 0,
            name: "a.pdf".into(),
        };
        assert_eq!(beat.fraction(), None);
        assert_eq!(beat.percent(), None);
        let done = ImportProgress {
            total: 48,
            done: 12,
            ..beat.clone()
        };
        assert_eq!(done.percent(), Some(25));
        assert_eq!(done.fraction(), Some(0.25));
        let over = ImportProgress {
            total: 10,
            done: 40,
            ..beat
        };
        assert_eq!(over.percent(), Some(100), "a count past the total clamps");
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
