//! The library route (`/`): the shelf, and a title bar that is navigation rather
//! than chrome.
//!
//! The bar keeps the reader's shape — leading cluster, centred slot, trailing
//! cluster, the built-in pin — and changes what each is for. Left is where you
//! are (the breadcrumb), centre is how to narrow it (the search), right is how it
//! looks (the view menu) plus the app's own appearance and settings. There is no
//! Open button and no Import button: adding books is a shelf affordance (the `+`
//! card, the empty state, a drop) rather than a title-bar one, because a bar
//! button that opens a picker is a button that hides the library's real door.
//!
//! No sidebar, no zoom, no mode, no document search: those are reader-only, so
//! the shell controller this page provides is rail-less and the bar keeps the
//! full window width, its gutter and its lights.

use leptos::prelude::*;

use app_chrome::hooks::dom::TOOLBAR_LEADING_ID;
use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;

use crate::components::menus::appearance_menu::AppearanceMenu;
use crate::components::settings::modal::SettingsModal;
use crate::components::shell::controller::ShellController;
use crate::components::shell::titlebar::app_title_bar::AppTitleBar;
use crate::features::library::breadcrumb::Breadcrumb;
use crate::features::library::content::LibraryContent;
use crate::features::library::import_modal::{ImportModal, ImportSheet, drain_sheet_toasts};
use crate::features::library::progress_dock::ProgressDock;
use crate::features::library::titlebar_search::TitlebarSearch;
use crate::features::library::view_menu::ViewMenu;
use crate::state::AppState;

#[component]
pub fn LibraryPage(state: AppState) -> impl IntoView {
    // The shell's layout truth, answering every rail question with "no rail" —
    // the bar and the traffic lights ask it like any page's do.
    let shell = ShellController::titlebar_only(state);
    provide_context(shell);

    // The library is rail-less, so settings open straight from its title bar.
    let settings_open = RwSignal::new(false);
    // The import sheet's handles, provided here so the three surfaces that can
    // open it (the add card, the empty state, a dropped folder) never have to
    // pass two signals through the grid to reach it.
    let sheet = ImportSheet::provide();

    // A picker that failed before the sheet could open has nowhere of its own to
    // put the error, and the page owns the app's one toast slot.
    Effect::new(move |_| drain_sheet_toasts(state, sheet));

    let left = move || {
        view! {
            <div class="flex min-w-0 items-center gap-1">
                <div
                    id=TOOLBAR_LEADING_ID
                    data-tauri-drag-region="true"
                    class="flex min-w-0 shrink-0 items-center gap-1"
                >
                    <Breadcrumb state=state />
                </div>
            </div>
        }
    };
    let center = move || view! { <TitlebarSearch state=state /> };
    let right = move || {
        view! {
            // #toolbar-trailing is owned by the shell's trailing group (this
            // cluster + the pin), so the page only styles its own cluster here.
            <div data-tauri-drag-region="true" class="flex shrink-0 items-center gap-1">
                <ViewMenu state=state />
                <AppearanceMenu state=state />
                <IconButton
                    icon=IconName::Settings
                    title="Reader settings"
                    on_click=move || settings_open.set(true)
                />
            </div>
        }
    };

    view! {
        <AppTitleBar state=state left=left center=center right=right>
            <div class="relative h-full w-full overflow-hidden bg-paper text-ink">
                <LibraryContent state=state />
            </div>
            <SettingsModal state=state open=settings_open />
            <ImportModal state=state sheet=sheet />
            // Fixed, and mounted at the page rather than inside the content: an
            // import outlives the state the shelf is in, and a card that unmounted
            // with an "Opening…" would take its progress with it.
            <ProgressDock state=state />
        </AppTitleBar>
    }
}
