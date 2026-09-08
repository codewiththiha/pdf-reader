//! The two ways books arrive, as one small popover.
//!
//! Both the shelf's `+` card and the empty state's button open this rather than
//! doing anything themselves: an import has exactly two sources and the reader
//! should see both from either, or the empty state becomes the only way to find
//! one of them. The rows are the same two the drop overlay already accepts, so
//! "drop a file anywhere" and these are one feature with three doors.

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use app_chrome::icon::IconName;

use crate::components::primitives::menu::menu_item::MenuItem;
use crate::components::shell::titlebar::toolbar_popover::MenuPopover;
use crate::features::library::import_modal::ImportSheet;
use crate::services::library::{import_files, pick_documents};
use crate::state::{AppState, Toast};

/// Pick files and import them, read in place.
///
/// A cancel is not an error and raises nothing; anything else is worth a toast,
/// because the reader asked for this and got no books.
fn from_files(state: AppState, target: Option<String>) {
    spawn_local(async move {
        match pick_documents().await {
            Ok(paths) if paths.is_empty() => {}
            Ok(paths) => import_files(state, paths, target),
            Err(message) => state.ui.toast.set(Some(Toast::new(message))),
        }
    });
}

/// Pick a folder and open the import sheet onto it. The sheet is the point: a
/// folder has options (which formats, how small is too small, whether to copy,
/// whether to watch), and importing one on the strength of a picker alone would
/// have to guess all of them.
fn from_directory(sheet: ImportSheet) {
    spawn_local(async move {
        match crate::services::library::pick_folder().await {
            Ok(Some(root)) => sheet.open_on(Some(root)),
            Ok(None) => {}
            Err(message) => sheet.toast(message),
        }
    });
}

#[component]
pub(crate) fn AddMenu(
    state: AppState,
    open: RwSignal<bool>,
    anchor: NodeRef<html::Div>,
    /// The shelf a pick lands on. `None` files onto no shelf, which leaves the
    /// books in "All" — the honest answer for a pick made from the root. Read at
    /// click time rather than at mount: the trigger outlives a drill in or out.
    target: Signal<Option<String>>,
) -> impl IntoView {
    let sheet = use_context::<ImportSheet>().expect("the library page provides the import sheet");

    view! {
        <MenuPopover open=open anchor=anchor width=232 class="p-1".to_string()>
            <MenuItem
                icon=IconName::Open
                label="From Local File"
                on_click=move || {
                    open.set(false);
                    from_files(state, target.get_untracked());
                }
            />
            <MenuItem
                icon=IconName::Library
                label="From Local Directory"
                on_click=move || {
                    open.set(false);
                    from_directory(sheet);
                }
            />
        </MenuPopover>
    }
}
