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
    /// The ellipsis that stands for the levels the bar has folded away.
    ///
    /// A target and not a drop: resting on it during a drag opens the panel,
    /// because a captured pointer raises no `mouseenter` for the bar to hear, and
    /// releasing on it does nothing — it is not a level, and filing onto a place
    /// whose name the reader cannot see is a filing they cannot check. It does not
    /// take the sink either, for the same reason: the ghost shrinking INTO a gap
    /// would be a picture of the held items going somewhere they cannot go. What it
    /// is, is a door, and the ghost stays in the reader's hand until the panel
    /// offers them a level to sink into.
    Ellipsis,
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

/// The id of the element a row of the library is drawn by: the drop target's
/// box, the reveal's scroll destination and the node the hit-test resolves are
/// all named from here, because six files used to write `"book-"` and
/// `"folder-"` by hand and a rename in one of them is a drag that hits nothing.
pub fn row_dom_id(kind: DropTargetKind, row_id: &str) -> String {
    match kind {
        DropTargetKind::Book => format!("book-{row_id}"),
        DropTargetKind::Folder => format!("folder-{row_id}"),
        // Crumbs name themselves (`crate::features::library::breadcrumb`), the
        // level's space and the ellipsis have fixed ids: nothing here to derive.
        _ => row_id.to_string(),
    }
}

/// A target on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropTargetEntry {
    pub id: DropTargetId,
    /// The element whose box this target occupies. Named rather than held: a
    /// card that unmounted mid-drag leaves an id that finds nothing, which is a
    /// target that cannot be hit rather than a node that has to be checked for
    /// liveness.
    pub dom_id: String,
    /// The shelf whose member list renders this target's row, when the row
    /// knows it: a book inside an expanded tree answers to THAT shelf rather
    /// than to the level the page happens to be on. `Some(ALL_SHELF)` spells
    /// the library's own order; `None` is "unspecified", and the session
    /// resolves it to the open level — the answer a grid card implies, because
    /// a card is only ever drawn by the shelf the page is on.
    pub shelf: Option<String>,
}

/// One registration: a target, and the token that says WHICH registration of it
/// this is.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Registered {
    token: u64,
    entry: DropTargetEntry,
}

/// Every target on the library page, in the order they registered.
///
/// One per page and shared by every card, the way the single drop-target signal
/// it replaces was: exactly one target is hot at a time, and no card has to know
/// about any other for that to hold.
#[derive(Clone, Copy)]
pub struct DropTargetRegistry {
    entries: RwSignal<Vec<Registered>>,
    /// The next registration token. Not a count of the entries: it has to keep
    /// rising across a level the reader drilled out of and back into.
    tokens: RwSignal<u64>,
}

impl DropTargetRegistry {
    pub fn new() -> Self {
        Self {
            entries: RwSignal::new(Vec::new()),
            tokens: RwSignal::new(0),
        }
    }

    /// Add a target for the life of the owner that asks.
    ///
    /// The cleanup is the reason this is a method and not a write to a signal: a
    /// card that unmounted without leaving the registry would keep a dead id in it
    /// forever, and a shelf the reader drilled through twenty times would be
    /// hit-tested against twenty levels of ghosts.
    ///
    /// The cleanup removes THIS registration by token and not the target by id,
    /// because the two are not the same thing. A crumb is re-created whenever the
    /// chain changes and a card whenever its level does, and the new one registers
    /// the same id the old one is about to leave — so an unmount that removed by id
    /// would take the replacement with it whenever the new owner is built before
    /// the old one is disposed, and the target would silently stop being a target.
    pub fn register(&self, entry: DropTargetEntry) {
        let id = entry.id.clone();
        self.tokens.update(|at| *at += 1);
        let token = self.tokens.get_untracked();
        self.entries.update(|list| {
            // Replacing rather than refusing a duplicate: a card re-created under
            // the same key is the same target, and two entries for it would let
            // the older one win a hit-test after the newer one left.
            list.retain(|each| each.entry.id != id);
            list.push(Registered { token, entry });
        });
        let entries = self.entries;
        on_cleanup(move || entries.update(|list| list.retain(|each| each.token != token)));
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
            list.iter().rev().find_map(|each| {
                let rect = by_id(&each.entry.dom_id)?.get_bounding_client_rect();
                let inside = x >= rect.left()
                    && x <= rect.right()
                    && y >= rect.top()
                    && y <= rect.bottom();
                inside.then(|| each.entry.id.clone())
            })
        })
    }

    /// The live box of one registered target.
    ///
    /// What the ghost's sink is aimed at: a sunk ghost sits at the CENTRE of the
    /// thing it is landing on, and the centre has to be read rather than
    /// remembered, because the shelf can scroll and a level can re-lay itself out
    /// between the moment a drag starts and the moment it rests somewhere.
    pub fn rect_of(&self, id: &DropTargetId) -> Option<web_sys::DomRect> {
        self.entries.with_untracked(|list| {
            list.iter()
                .find(|each| &each.entry.id == id)
                .and_then(|each| by_id(&each.entry.dom_id))
                .map(|node| node.get_bounding_client_rect())
        })
    }

    /// The entry registered under `id`, if one is live.
    ///
    /// What the session asks for a row's own container: the hit-test answers
    /// WHICH target the pointer is on, and the entry carries the shelf whose
    /// member list renders it — the fact an insertion resolves its index
    /// against, so a drop inside an expanded tree lands in the shelf the row
    /// belongs to rather than in the level the page is on.
    pub fn entry_of(&self, id: &DropTargetId) -> Option<DropTargetEntry> {
        self.entries.with_untracked(|list| {
            list.iter()
                .find(|each| &each.entry.id == id)
                .map(|each| each.entry.clone())
        })
    }
}

impl Default for DropTargetRegistry {
    fn default() -> Self {
        Self::new()
    }
}
