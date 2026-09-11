//! The ⋯ menu: how the shelf looks.
//!
//! Four decisions and nothing else — the layout, the column count, the cover
//! treatment and the sort — because those are the four a reader actually changes
//! while looking at a shelf. Every one of them is a `LibraryView` field, so the
//! menu writes the view and the view writes the page: no row here reaches into
//! the DOM to arrange anything itself.
//!
//! The Columns row is the one that can be impossible, and it says so by going
//! quiet rather than by hiding: a list has no columns. Auto is not impossible —
//! it is a count the grid reports on every resize (see
//! `crate::features::library::grid`), so the row shows the live number and the
//! stepper's first press pins it and steps from there. Every press leaves the
//! stored value whole, so switching back to a grid returns the columns the
//! reader last picked.

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use library_core::sort::SortKey;
use library_core::view::{CoverFit, LibraryLayout, LibraryView};

use crate::components::primitives::controls::option_button::OptionButton;
use crate::services::library::create_shelf_and_enter;
use crate::components::primitives::menu::menu_item::MenuItem;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::components::shell::titlebar::toolbar_popover::MenuPopover;
use crate::state::AppState;

/// The sort keys the menu offers, in the order it offers them. The labels come
/// from the keys themselves, so a key added to `library_core::sort` is one row
/// here and no copy to keep in step.
const SORTS: [SortKey; 5] = [
    SortKey::Manual,
    SortKey::Title,
    SortKey::Author,
    SortKey::Added,
    SortKey::LastRead,
];

/// Write one view change and persist it. Every control in this menu goes through
/// here, so the "a knob that moves is a knob that is saved" rule is stated once.
fn set_view(state: AppState, change: impl FnOnce(&mut LibraryView) + 'static) {
    state.library.view.update(change);
    crate::storage::persist_library(state.library);
}

#[component]
pub(crate) fn ViewMenu(state: AppState) -> impl IntoView {
    let open = RwSignal::new(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();

    let is_list = Signal::derive(move || state.library.view.with(|v| v.is_list()));
    // The count the stepper shows and steps from: the pinned count when there is
    // one, else the count Auto's flow is producing right now — which the grid
    // reports on every resize (see `crate::features::library::grid`). An en-dash
    // was what this row used to show under Auto; the live count is a better
    // answer to "how many across am I looking at", and it is what the bounds
    // below read, so Auto reports its own edges instead of having none.
    let columns = Signal::derive(move || {
        state.library.view.with(|v| v.columns.or(Some(v.auto_fit)))
    });
    // "Auto" is selected when no count is pinned — the raw field, not `columns`,
    // which now answers for Auto too.
    let auto = Signal::derive(move || state.library.view.with(|v| v.columns.is_none()));
    let stepper_live = Signal::derive(move || state.library.view.with(|v| v.columns_enabled()));
    let at_min = Signal::derive(move || {
        columns.with(|c| c.is_some_and(|n| n <= library_core::view::COLUMNS_MIN))
    });
    let at_max = Signal::derive(move || {
        columns.with(|c| c.is_some_and(|n| n >= library_core::view::COLUMNS_MAX))
    });
    let fit = Signal::derive(move || state.library.view.with(|v| v.cover == CoverFit::Fit));
    let sort = Signal::derive(move || state.library.view.with(|v| v.sort));
    let ascending = Signal::derive(move || state.library.view.with(|v| v.sort_asc));
    // Manual order has no direction: it IS the order. Offering one would be a
    // control that appears to do something and rearranges nothing.
    let has_direction = Signal::derive(move || !sort.get().is_manual());

    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <IconButton
                icon=IconName::More
                title="Shelf view"
                pressed=Signal::derive(move || open.get())
                on_click=move || open.set(!open.get_untracked())
            />
            <MenuPopover
                open=open
                anchor=root_ref
                width=264u32
                coordinate_space="toolbar-row"
                class="p-2".to_string()
            >
                <MenuItem
                    icon=IconName::Plus
                    label="New shelf"
                    on_click=move || {
                        // Drills straight in, so the reader is looking at the shelf
                        // they just made and the breadcrumb is offering its name.
                        open.set(false);
                        create_shelf_and_enter(state, None);
                    }
                />
                <Separator spacing="my-1.5" />
                <MenuItem
                    label="List"
                    selected=Signal::derive(move || is_list.get())
                    check=true
                    on_click=move || {
                        set_view(state, |v| v.layout = LibraryLayout::List);
                    }
                />
                <MenuItem
                    label="Grid"
                    selected=Signal::derive(move || !is_list.get())
                    check=true
                    on_click=move || {
                        set_view(state, |v| v.layout = LibraryLayout::Grid);
                    }
                />

                <Separator spacing="my-1.5" />
                <SectionLabel text="Columns" />
                <div class="flex items-center justify-between gap-2 px-1 py-1">
                    <OptionButton
                        selected=auto
                        on_click=move || {
                            set_view(state, LibraryView::auto_columns);
                        }
                        variant_class="px-2 py-1 text-xs"
                    >
                        <span>"Auto"</span>
                    </OptionButton>
                    // A control that cannot act reads as quiet without help: the
                    // disabled buttons already carry the app's disabled treatment,
                    // and a dimmed wrapper on top of it was a second statement of
                    // one fact — and the fact was wrong whenever it dimmed a
                    // stepper that was live.
                    <div class="flex items-center gap-0.5">
                        <IconButton
                            icon=IconName::Minus
                            size=13
                            title="Fewer columns"
                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                            disabled=Signal::derive(move || !stepper_live.get() || at_min.get())
                            on_click=move || {
                                set_view(state, |v| v.step_columns(-1));
                            }
                        />
                        <span class="w-5 text-center text-xs tabular-nums text-ink">
                            {move || {
                                columns.get().map_or_else(|| "–".to_string(), |n| n.to_string())
                            }}
                        </span>
                        <IconButton
                            icon=IconName::Plus
                            size=13
                            title="More columns"
                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                            disabled=Signal::derive(move || !stepper_live.get() || at_max.get())
                            on_click=move || {
                                set_view(state, |v| v.step_columns(1));
                            }
                        />
                    </div>
                </div>

                <Separator spacing="my-1.5" />
                <SectionLabel text="Book covers" />
                <MenuItem
                    label=CoverFit::Fit.label()
                    selected=Signal::derive(move || fit.get())
                    check=true
                    on_click=move || {
                        set_view(state, |v| v.cover = CoverFit::Fit);
                    }
                />
                <MenuItem
                    label=CoverFit::Crop.label()
                    selected=Signal::derive(move || !fit.get())
                    check=true
                    on_click=move || {
                        set_view(state, |v| v.cover = CoverFit::Crop);
                    }
                />

                <Separator spacing="my-1.5" />
                <SectionLabel text="Sort by" />
                {move || {
                    has_direction.get().then(|| {
                        view! {
                            <div class="mb-1 flex gap-1.5 px-1">
                                <OptionButton
                                    selected=Signal::derive(move || ascending.get())
                                    on_click=move || {
                                        set_view(state, |v| v.sort_asc = true);
                                    }
                                    variant_class="flex flex-1 items-center justify-center gap-1 px-2 py-1 text-xs"
                                    title="Ascending"
                                >
                                    <Icon name=IconName::ChevronUp size=12 />
                                    <span>"Ascending"</span>
                                </OptionButton>
                                <OptionButton
                                    selected=Signal::derive(move || !ascending.get())
                                    on_click=move || {
                                        set_view(state, |v| v.sort_asc = false);
                                    }
                                    variant_class="flex flex-1 items-center justify-center gap-1 px-2 py-1 text-xs"
                                    title="Descending"
                                >
                                    <Icon name=IconName::ChevronDown size=12 />
                                    <span>"Descending"</span>
                                </OptionButton>
                            </div>
                        }
                    })
                }}
                {SORTS
                    .iter()
                    .copied()
                    .map(|key| {
                        view! {
                            <MenuItem
                                label=key.label().to_string()
                                selected=Signal::derive(move || sort.get() == key)
                                check=true
                                on_click=move || {
                                    set_view(state, move |v| v.sort = key);
                                }
                            />
                        }
                    })
                    .collect_view()}
            </MenuPopover>
        </div>
    }
}
