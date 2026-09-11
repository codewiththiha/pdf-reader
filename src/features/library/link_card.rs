//! A link row: the shelf's own card and row shape, with a pointer's facts on it
//! instead of a book's.
//!
//! A link is not a book and cannot borrow a book's card — it has no address to
//! read, no page to render art from, no resume point to draw a bar for and no
//! format to name. What it has is a name (the name of the book it points at,
//! which is what makes it recognisable beside it), a target, and the same three
//! gestures every other row on the shelf answers to: a tap goes to the book, a
//! hold selects it, a movement files it somewhere else. So it wears the shelf's
//! own item shell — `crate::features::library::shelf_item`, the one wiring a
//! card and a list row both wear — and the plate where a cover would be says
//! what it is instead.
//!
//! The one thing a link does that no book does is open somewhere else: a tap
//! reveals the book it points at, on the shelf that book is filed on, and lights
//! its card (`crate::services::library::reveal_book`). That is the whole of the
//! row's purpose, and it is why a link is a row the reader can file anywhere
//! without ever moving a file.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

use crate::features::library::context_menu::MenuTarget;
use crate::features::library::gestures::ShelfItemPolicy;
use crate::features::library::list::row_indent;
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::shelf_item::{SeamVocab, ShelfItemShell};
use crate::services::document;
use crate::state::AppState;

/// The line a link carries where a book carries its author or its page: what it
/// is, and that a tap goes to the book rather than opening a file.
const LINK_LINE: &str = "Link · opens the book where it is";

/// The policy both link surfaces share: the same label, the same open, and the
/// same menu answer — a link is a row like any other to the menu, Open goes to
/// the book, Select and Remove mean what they always mean, and "Find again" is
/// not offered because a pointer has no address to die. One spelling rather
/// than one per density.
fn link_policy(state: AppState, id: &str, name: &str, container: Option<String>) -> ShelfItemPolicy {
    let open_id = id.to_string();
    let menu_id = id.to_string();
    let label = name.to_string();
    ShelfItemPolicy {
        id: id.to_string(),
        label: Signal::stored(label),
        draggable: Signal::derive(|| true),
        open: Callback::new(move |_| document::open_row(state, open_id.clone())),
        menu_target: Callback::new(move |_| MenuTarget::Book {
            id: menu_id.clone(),
            missing: false,
        }),
        container,
    }
}

/// One link on the grid.
#[component]
pub(crate) fn LinkCard(state: AppState, id: String, name: String) -> impl IntoView {
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");

    let reveal_id = id.clone();
    let reveal_class = Signal::derive(move || {
        state
            .library
            .reveal
            .with(|at| at.as_ref().is_some_and(|(each, _)| each == reveal_id.as_str()))
    });
    let remove_id = id.clone();
    let policy = link_policy(state, &id, &name, None);
    let remove = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        remove_sheet.ask(&remove_id);
    };

    view! {
        <ShelfItemShell
            state=state
            vocab=SeamVocab::GridCard
            base_class="book-card book-link"
            policy=policy
            extra_classes=vec![("book-reveal".to_string(), reveal_class)]
        >
            <div class="book-cover-wrap">
                <div class="book-cover" style:aspect-ratio="210 / 297">
                    <div class="book-cover-fallback">
                        <span>{name.clone()}</span>
                    </div>
                    <span class="book-link-badge" title="A pointer at a book, not a copy of one">
                        <Icon name=IconName::Link size=11 />
                    </span>
                </div>
            </div>

            <div class="book-info">
                <span class="book-title" title=name.clone()>{name.clone()}</span>
                <span class="book-page">{LINK_LINE}</span>
            </div>

            <button
                type="button"
                class="book-remove"
                title="Remove this link"
                aria-label=move || format!("Remove the link to {}", name.clone())
                on:click=remove
            >
                <Icon name=IconName::Close size=12 />
            </button>
        </ShelfItemShell>
    }
}

/// One link in the list, at the depth its branch puts it at.
#[component]
pub(crate) fn LinkRow(
    state: AppState,
    id: String,
    name: String,
    depth: usize,
    /// The shelf whose member list renders this row — the tree's own id inside
    /// an expanded branch, `None` in the flat section. See
    /// `crate::features::library::list::ListRow` for why a row carries it.
    parent: Option<String>,
) -> impl IntoView {
    // Asked for rather than expected, the way the list's own row asks: the
    // sidebar's tree mounts this row with no sheet under it, and a row with no
    // sheet has no ✕ to draw.
    let remove_sheet = use_context::<RemoveSheet>();

    let reveal_id = id.clone();
    let reveal_class = Signal::derive(move || {
        state
            .library
            .reveal
            .with(|at| at.as_ref().is_some_and(|(each, _)| each == reveal_id.as_str()))
    });
    let remove_id = id.clone();
    let policy = link_policy(state, &id, &name, parent);
    let indent = row_indent(depth);
    let tooltip = name.clone();

    view! {
        <ShelfItemShell
            state=state
            vocab=SeamVocab::ListRow
            base_class="library-row book-link"
            policy=policy
            style=indent
            extra_classes=vec![("row-reveal".to_string(), reveal_class)]
        >
            <span class="library-row-ext" title="A pointer at a book, not a copy of one">
                <Icon name=IconName::Link size=12 />
            </span>
            <span class="min-w-0 flex-1">
                <span class="block truncate text-sm font-semibold text-ink" title=tooltip.clone()>
                    {name.clone()}
                </span>
                <span class="block truncate text-xs text-muted">{LINK_LINE}</span>
            </span>
            {move || {
                remove_sheet.map(|sheet| {
                    let at = remove_id.clone();
                    view! {
                        <button
                            class="library-row-remove"
                            type="button"
                            title="Remove this link"
                            aria-label=format!("Remove the link to {}", name.clone())
                            on:click=move |ev: leptos::ev::MouseEvent| {
                                ev.stop_propagation();
                                sheet.ask(&at);
                            }
                        >
                            <Icon name=IconName::Close size=12 />
                        </button>
                    }
                })
            }}
        </ShelfItemShell>
    }
}
