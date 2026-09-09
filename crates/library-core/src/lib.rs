//! The library's domain: what a book is, how the app holds one, and the rules
//! that decide what a folder scan does next.
//!
//! Everything here is pure — no filesystem, no wasm, no DOM, no leptos — so
//! the decisions that are easy to get wrong and impossible to see in a
//! browser are decisions a host test can hold to account
//! (`cargo test -p library-core`). The two halves that DO touch the machine
//! live elsewhere and meet this crate at its edges:
//!
//!   * walking a folder, measuring a file and copying one into the store is
//!     the Tauri shell's business (its `commands` module), which hands this
//!     crate the [`scan::FoundFile`] rows it dug out of the filesystem and
//!     performs the [`ledger::ScanAction`]s it gets back;
//!   * rendering the shelf, the shelves and the import sheet is
//!     `src/features/library`, which reads the persisted [`blob::LibraryBlob`]
//!     through `src/state/library.rs`.
//!
//! ## The one rule the whole crate is built around
//!
//! A book is an ADDRESS, not a copy. Reading in place is the only mode the
//! reader ever had, so the default holds: a [`book::Origin::Linked`] book is
//! the path it was opened from, and nothing in this crate ever moves a file
//! the user owns. [`book::Origin::Stored`] is the opt-in other way round — the
//! app copies the bytes into its own store and keeps the source path only as
//! provenance. In-app moves (a drag from one shelf to another) edit
//! [`shelf::Shelf::books`] membership and never the filesystem.
//!
//! ## What may live here
//!
//! * A rule the library needs and no format owns.
//! * No engine, no IPC, no signals: a function that takes values and returns
//!   values, so a test can call it.

pub mod blob;
pub mod book;
pub mod folder;
pub mod hash;
pub mod id;
pub mod ledger;
pub mod query;
pub mod scan;
pub mod shelf;
pub mod sort;
pub mod text;
pub mod view;
pub mod wire;

// Re-exported because the Tauri shell stores a format in a column and reads one
// back, and the shell deliberately does not name `reader-core` directly: the
// format registry is the frontend's, and `tools/check-formats.ts` keeps the
// shell's own copy of the extension list honest. One re-export is cheaper than a
// second dependency edge and cannot drift.
pub use reader_core::format::Format;
pub use blob::{LibraryBlob, RecentBook, migrate_v1};
pub use book::{BOOKS_CAP, Book, Fingerprint, Origin, ReadPoint, apply_check, record_read};
pub use folder::{FolderOpts, Tombstone, WatchedFolder};
pub use ledger::{
    KnownBook, Recovered, ScanAction, diff_folder, find_tombstone, index_by_fp, prune_tombstones,
    recoverables, restore_deleted, tombstone,
};
pub use scan::{FoundFile, admits};
pub use shelf::{ALL_SHELF, Shelf, ShelfKind, ancestors, can_nest, children_of, shelf_add};
pub use sort::{SortKey, sort_books};
pub use text::{human_age, human_size, plural};
pub use view::{CoverFit, LibraryLayout, LibraryView};
pub use wire::{
    Cover, GlossRow, ImportPhase, ImportProgress, LibrarySnapshot, PathCheck, SearchHit,
    StoreRequest, StoreResult,
};
