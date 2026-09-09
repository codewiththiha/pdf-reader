//! What a drop can land on, and how the pointer finds it.
//!
//! Targets register themselves instead of being discovered from the event that
//! happens to bubble past. A `dragover`/`dragleave` pair counts child boundaries
//! rather than targets, which is how the shelf's old marker flickered between a
//! card and the grid it sits in, and how a nested plate reported a leave for
//! every cell the pointer crossed. A registry hit-tested against the pointer's
//! own coordinates has no boundaries to cross: one answer per move, the topmost
//! target wins, and a target that has scrolled or unmounted between two moves is
//! simply not there to be hit.
//!
//! The box is read at hit-test time rather than cached at registration, because a
//! shelf that scrolls under a held pointer moves the target and not the pointer.

use leptos::prelude::*;

use app_chrome::hooks::dom::by_id;

/// The kind of thing under the pointer. This — and not the payload — is what the
/// decision table in `crate::features::library::dnd::effect` switches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTargetKind {
    /// A book: a card in the grid or a row in the list. Both are the same target,
    /// because both are the same book at two densities.
    Book,
    /// A shelf drawn as a folder.
    Folder,
    /// A breadcrumb crumb. The way back to a level is therefore also a way to
    /// file whatever is held onto that level from anywhere in the library,
    /// including from inside a folder the reader has not left yet.
    Shelf,
    /// The empty space of the level the page is on. WHICH shelf that is belongs
    /// to the page rather than to the target, so this kind carries no id and the
    /// controller fills one in.
    Level,
}

/// One target: the kind of thing it is and the item it stands for.
///
/// The id is the book's, the shelf's, or empty for the root — never a path, for
/// the reason `crate::services::library::arrange` gives: an in-app move edits a
/// list of ids and never touches the filesystem, and a drag that carried a path
/// would invite somebody to act on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropTargetId(pub DropTargetKind, pub String);

/// A target on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropTargetEntry {
    pub id: DropTargetId,
    /// The element whose box this target occupies. Named rather than held: a
    /// card that unmounted mid-drag leaves an id that finds nothing, which is a
    /// target that cannot be hit rather than a node that has to be checked for
    /// liveness.
    pub dom_id: String,
}

/// Every target on the library page, in the order they registered.
///
/// One per page and shared by every card, the way the single drop-target signal
/// it replaces was: exactly one target is hot at a time, and no card has to know
/// about any other for that to hold.
#[derive(Clone, Copy)]
pub struct DropTargetRegistry {
    entries: RwSignal<Vec<DropTargetEntry>>,
}

impl DropTargetRegistry {
    pub fn new() -> Self {
        Self {
            entries: RwSignal::new(Vec::new()),
        }
    }

    /// Add a target for the life of the owner that asks.
    ///
    /// The cleanup is the reason this is a method and not a write to a signal: a
    /// card that unmounted without leaving the registry would keep a dead id in
    /// it forever, and a shelf the reader drilled through twenty times would be
    /// hit-tested against twenty levels of ghosts.
    pub fn register(&self, entry: DropTargetEntry) {
        let id = entry.id.clone();
        self.entries.update(|list| {
            // Replacing rather than refusing a duplicate: a card re-created under
            // the same key is the same target, and two entries for it would let
            // the older one win a hit-test after the newer one left.
            list.retain(|each| each.id != id);
            list.push(entry);
        });
        let entries = self.entries;
        on_cleanup(move || entries.update(|list| list.retain(|each| each.id != id)));
    }

    /// The topmost target under the pointer.
    ///
    /// Walked in reverse, so the targets that registered last are asked first.
    /// That ordering is the whole of the containment rule: a level's empty space
    /// registers when the page mounts and the cards on it register after, so a
    /// card is found before the space it sits in — and a card that unmounted is
    /// skipped rather than hit, because the element its id names is gone.
    pub fn hit_test(&self, x: f64, y: f64) -> Option<DropTargetId> {
        self.entries.with_untracked(|list| {
            list.iter().rev().find_map(|entry| {
                let rect = by_id(&entry.dom_id)?.get_bounding_client_rect();
                let inside = x >= rect.left()
                    && x <= rect.right()
                    && y >= rect.top()
                    && y <= rect.bottom();
                inside.then(|| entry.id.clone())
            })
        })
    }
}

impl Default for DropTargetRegistry {
    fn default() -> Self {
        Self::new()
    }
}
