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
//!
//! ## The list is of rows, not of books
//!
//! A shelf holds [`book::Row`]s: a [`book::Book`], or a [`book::Row::Link`]
//! that points at one. Everything that asks a question of a row's CONTENT —
//! a fingerprint, an address, a resume point — asks it of the book rows and
//! steps over the links, because a link has none of the three
//! ([`book::book_rows`]). Everything that asks a question of a row's PLACE —
//! a shelf membership, a drag, a removal, the order a level renders in — asks
//! it of the row, whichever kind it is. [`conflict`] is the one rule that is
//! about neither: it asks whether a NAME is already on a level, which is a
//! question a book answers with its title and a link with the name it was
//! made with.

pub mod blob;
pub mod book;
pub mod conflict;
pub mod folder;
pub mod governance;
pub mod hash;
pub mod id;
pub mod ledger;
pub mod query;
pub mod scan;
pub mod shelf;
pub mod sort;
pub mod store;
pub mod text;
pub mod view;
pub mod wire;

/// The shared fixtures every host test builds its books, rows and shelves
/// from. Compiled only for tests: the crate's own `cargo test`, or a
/// dependent crate that asks for the `test-util` feature in its
/// dev-dependencies — which is how the app crate's tests reach it and how a
/// release bundle never does.
#[cfg(any(test, feature = "test-util"))]
pub mod testkit;

