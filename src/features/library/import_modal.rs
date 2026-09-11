//! The import sheet: what a folder import is allowed to be.
//!
//! Five answers, and each one is a `library_core::folder::FolderOpts` field — the
//! sheet writes the options and the shell's walk reads them, so there is no
//! second copy of "what does larger than 30 KB mean" anywhere in the app.
//!
//! The two switches are nested on purpose. "Watch for new books" only means
//! something for a folder the library reads in place: a store copy is the app's
//! own file from the moment it lands, and rescanning the source afterwards would
//! be a second opinion about a book that already exists. The row hides rather
//! than disables, because a switch that cannot be turned on is noise.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use library_core::folder::{FolderOpts, MIN_SIZE_CEIL, MIN_SIZE_FLOOR};
use library_core::scan::selectable_formats;
use reader_core::format::Format;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::controls::option_button::OptionButton;
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::overlay::lanes::{OverlayPolicy, use_overlay_lane};
use crate::components::primitives::overlay::modal_scrim::ModalScrim;
use crate::components::settings::common::Row;
use crate::services::library::{import_folder, pick_folder};
use crate::state::AppState;

/// The sheet's two handles, provided by the library page: whether it is open and
/// the folder it opens onto.
///
/// A context rather than props because three surfaces can open it — the shelf's
/// `+` card, the empty state's button and a folder dropped on the window — and
/// threading two signals through all of them would put the sheet's plumbing in
/// every component between.
#[derive(Clone, Copy)]
pub(crate) struct ImportSheet {
    /// The app's state, carried so the sheet's surfaces can report a failure
    /// through the app's ONE toast slot instead of keeping a queue of their own.
    state: AppState,
    pub open: RwSignal<bool>,
    pub root: RwSignal<Option<String>>,
}

impl ImportSheet {
    /// Create and provide the handles. Called once, by the page.
    pub fn provide(state: AppState) -> Self {
        let sheet = Self {
            state,
            open: RwSignal::new(false),
            root: RwSignal::new(None),
        };
        provide_context(sheet);
        sheet
    }

    /// Open the sheet, onto `root` when the caller already has one.
    pub fn open_on(&self, root: Option<String>) {
        if let Some(root) = root {
            self.root.set(Some(root));
        }
        self.open.set(true);
    }

    /// Report a failure to open the sheet — a picker that could not run, on a
    /// surface that has no toast of its own. The app's slot is the one place a
    /// message goes; a second queue here would be a second opinion about who
    /// shows it.
    pub fn toast(&self, message: String) {
        self.state.toast(message);
    }
}

#[component]
pub(crate) fn ImportModal(state: AppState, sheet: ImportSheet) -> impl IntoView {
    // One modal at a time, and a menu replaces it rather than stacking under it
    // — the same arbitration the reader's settings modal joins.
    use_overlay_lane(sheet.open, OverlayPolicy::MODAL);

    // The options outlive the sheet being open: a reader who imports a second
    // folder usually wants it imported the same way as the first.
    let opts = RwSignal::new(FolderOpts::default());

    // (Escape is the scrim's rule: see `crate::components::primitives::overlay::modal_scrim`.)

    let in_place = Signal::derive(move || opts.with(|o| o.in_place));
    let watching = Signal::derive(move || opts.with(|o| o.watch));
    let include = Signal::derive(move || opts.with(|o| o.include_selected));
    let grouped = Signal::derive(move || opts.with(|o| o.groups));
    let min_size = Signal::derive(move || opts.with(|o| o.min_size));
    let label = Signal::derive(move || opts.with(|o| o.min_size_label()));
    // The floor is zero, so "at the floor" and "is zero" are one question.
    let at_floor = Signal::derive(move || min_size.get() == MIN_SIZE_FLOOR);
    let at_ceil = Signal::derive(move || min_size.get() >= MIN_SIZE_CEIL);
    let chosen = Signal::derive(move || sheet.root.with(|r| r.is_some()));
    // The path row truncates to one line, and a deep folder is exactly the
    // path a reader needs to READ before trusting the import with it — so
    // the truncation has an adjuster: one control that unfolds the row to the
    // whole path, wrapped, and folds it back.
    let path_open = RwSignal::new(false);

    view! {
        <ModalScrim open=sheet.open>
            <div
                    class="flex max-h-[86vh] w-full flex-col overflow-hidden rounded-2xl border border-line bg-surface shadow-2xl"
                    style="width:min(92vw, 480px)"
                    on:click=move |ev| ev.stop_propagation()
                    role="dialog"
                    aria-label="Import a folder"
                >
                    <header class="flex shrink-0 items-center gap-2 px-4 pb-2 pt-4">
                        <h2 class="text-sm font-semibold text-ink">"Import books"</h2>
                        <div class="ml-auto">
                            <IconButton
                                icon=IconName::Close
                                title="Close"
                                class="rounded-full bg-line/60 hover:bg-line".to_string()
                                on_click=move || sheet.open.set(false)
                            />
                        </div>
                    </header>

                    <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
                        // --- the folder ------------------------------------
                        <SectionLabel text="Folder" />
                        <div class="mb-4 flex items-start gap-2 rounded-xl border border-line px-3 py-2.5">
                            <Icon name=IconName::Open size=16 class="mt-0.5 shrink-0 text-muted" />
                            <span
                                class=move || {
                                    if path_open.get() {
                                        "min-w-0 flex-1 break-all text-sm text-ink"
                                    } else {
                                        "min-w-0 flex-1 truncate text-sm text-ink"
                                    }
                                }
                                title=move || sheet.root.get().unwrap_or_default()
                            >
                                {move || {
                                    sheet
                                        .root
                                        .get()
                                        .unwrap_or_else(|| "Choose a folder…".to_string())
                                }}
                            </span>
                            {move || {
                                chosen.get().then(|| {
                                    view! {
                                        <IconButton
                                            icon=if path_open.get() {
                                                IconName::ChevronUp
                                            } else {
                                                IconName::ChevronDown
                                            }
                                            title=if path_open.get() {
                                                "Show one line"
                                            } else {
                                                "Show the full path"
                                            }
                                            class="rounded-full bg-line/60 hover:bg-line".to_string()
                                            on_click=move || path_open.set(!path_open.get())
                                        />
                                    }
                                })
                            }}
                            <Button
                                on_click=move |_| {
                                    wasm_bindgen_futures::spawn_local(async move {
                                        match pick_folder().await {
                                            Ok(Some(root)) => sheet.root.set(Some(root)),
                                            Ok(None) => {}
                                            Err(message) => sheet.toast(message),
                                        }
                                    });
                                }
                                variant=ButtonVariant::Ghost
                                compact=true
                                title="Choose another folder"
                            >
                                <span>"Change…"</span>
                            </Button>
                        </div>

                        // --- formats ---------------------------------------
                        <SectionLabel text="Formats" />
                        <div class="mb-2 flex gap-1.5">
                            <OptionButton
                                selected=Signal::derive(move || include.get())
                                on_click=move || opts.update(|o| o.include_selected = true)
                                variant_class="flex-1 px-2 py-1.5 text-xs"
                            >
                                <span>"Include selected"</span>
                            </OptionButton>
                            <OptionButton
                                selected=Signal::derive(move || !include.get())
                                on_click=move || opts.update(|o| o.include_selected = false)
                                variant_class="flex-1 px-2 py-1.5 text-xs"
                            >
                                <span>"Exclude selected"</span>
                            </OptionButton>
                        </div>
                        <div class="mb-4 grid grid-cols-2 gap-1.5">
                            {selectable_formats()
                                .into_iter()
                                .map(|format| {
                                    view! {
                                        <FormatRow opts=opts format=format />
                                    }
                                })
                                .collect_view()}
                        </div>

                        // --- size ------------------------------------------
                        <SectionLabel text="File size larger than" />
                        <div class="mb-4 flex items-center justify-between gap-3 rounded-xl border border-line px-3 py-2">
                            <span class="rounded-md bg-line/60 px-2 py-0.5 text-xs tabular-nums text-ink">
                                {move || label.get()}
                            </span>
                            <div class="flex items-center gap-1">
                                <IconButton
                                    icon=IconName::Minus
                                    size=14
                                    title="Smaller files count too"
                                    class="rounded-full bg-line/60 hover:bg-line".to_string()
                                    disabled=at_floor
                                    on_click=move || opts.update(|o| o.step_min_size(-1))
                                />
                                <IconButton
                                    icon=IconName::Plus
                                    size=14
                                    title="Only larger files"
                                    class="rounded-full bg-line/60 hover:bg-line".to_string()
                                    disabled=at_ceil
                                    on_click=move || opts.update(|o| o.step_min_size(1))
                                />
                            </div>
                        </div>

                        // --- how the books are held -------------------------
                        <SectionLabel text="Books" />
                        <div class="divide-y divide-line rounded-xl border border-line">
                            <Row label="Keep books where they are">
                                <Switch
                                    checked=in_place
                                    on_change=Callback::new(move |on| {
                                        opts.update(|o| {
                                            o.in_place = on;
                                            // Watching a copy is a question the
                                            // sheet does not ask, so turning the
                                            // mode off takes the answer with it.
                                            if !on {
                                                o.watch = false;
                                            }
                                        });
                                    })
                                    title="Read the books from their own folders, without copying".to_string()
                                />
                            </Row>
                            <Show when=move || in_place.get() fallback=|| ()>
                                <div class="px-4 py-3.5 pl-8">
                                    <div class="flex items-center justify-between gap-3">
                                        <div class="min-w-0">
                                            <span class="block text-sm text-ink">
                                                "Watch for new books"
                                            </span>
                                            <span class="mt-0.5 block text-xs text-muted">
                                                "Checks for new books when the app opens or you come back to it."
                                            </span>
                                        </div>
                                        <Switch
                                            checked=watching
                                            on_change=Callback::new(move |on| {
                                                opts.update(|o| o.watch = on);
                                            })
                                            title="Watch for new books".to_string()
                                        />
                                    </div>
                                </div>
                            </Show>
                            <div class="px-4 py-3.5">
                                <span class="mb-2 block text-sm text-ink">"Folder structure"</span>
                                <div class="flex flex-col gap-1.5">
                                    <OptionButton
                                        selected=Signal::derive(move || grouped.get())
                                        on_click=move || opts.update(|o| o.groups = true)
                                        variant_class="flex items-center gap-2 px-2.5 py-1.5 text-xs"
                                    >
                                        <Dot on=Signal::derive(move || grouped.get()) />
                                        <span>"A shelf for each folder"</span>
                                    </OptionButton>
                                    <OptionButton
                                        selected=Signal::derive(move || !grouped.get())
                                        on_click=move || opts.update(|o| o.groups = false)
                                        variant_class="flex items-center gap-2 px-2.5 py-1.5 text-xs"
                                    >
                                        <Dot on=Signal::derive(move || !grouped.get()) />
                                        <span>"One shelf for everything"</span>
                                    </OptionButton>
                                </div>
                            </div>
                        </div>

                        <p class="mt-3 text-xs text-muted">
                            {move || {
                                if in_place.get() {
                                    "Books stay where they are — the library just remembers \
                                     where they live."
                                        .to_string()
                                } else {
                                    "Books are copied into the app's own files, so they keep \
                                     working even if the folder moves or is deleted."
                                        .to_string()
                                }
                            }}
                        </p>
                    </div>

                    <footer class="flex shrink-0 items-center justify-end gap-2 border-t border-line px-4 py-3">
                        <Button
                            on_click=move |_| sheet.open.set(false)
                            variant=ButtonVariant::Ghost
                            title="Close without importing"
                        >
                            <span>"Cancel"</span>
                        </Button>
                        <Button
                            on_click=move |_| {
                                let (Some(root), options) = (
                                    sheet.root.get_untracked(),
                                    opts.get_untracked(),
                                ) else {
                                    return;
                                };
                                sheet.open.set(false);
                                import_folder(state, root, options);
                            }
                            variant=ButtonVariant::Primary
                            disabled=Signal::derive(move || !chosen.get())
                            title="Import this folder"
                        >
                            <Icon name=IconName::Drop size=16 />
                            <span>"Import"</span>
                        </Button>
                    </footer>
                </div>
        </ModalScrim>
    }
}

/// One format's checkbox row. Its own component because the rows are built from
/// the registry rather than typed out, and a closure over a loop variable would
/// have to clone the signal handle per row anyway.
#[component]
fn FormatRow(opts: RwSignal<FolderOpts>, format: Format) -> impl IntoView {
    let on = Signal::derive(move || opts.with(|o| o.formats.contains(&format)));
    view! {
        <OptionButton
            selected=on
            on_click=move || {
                opts.update(|o| {
                    if o.formats.contains(&format) {
                        o.formats.remove(&format);
                    } else {
                        o.formats.insert(format);
                    }
                });
            }
            variant_class="flex items-center gap-2 px-2.5 py-1.5 text-xs"
            title=format.label().to_string()
        >
            <Dot on=on />
            <span>{format.label()}</span>
        </OptionButton>
    }
}

/// The radio dot inside an option row: filled when the row is the chosen one.
#[component]
fn Dot(on: Signal<bool>) -> impl IntoView {
    view! {
        <span
            class=move || {
                let base = "flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-full border";
                if on.get() {
                    format!("{base} border-accent")
                } else {
                    format!("{base} border-line")
                }
            }
        >
            {move || {
                on.get().then(|| {
                    view! { <span class="h-1.5 w-1.5 rounded-full bg-accent"></span> }
                })
            }}
        </span>
    }
}
