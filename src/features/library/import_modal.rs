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
//!
//! One ground the watch switch is NOT the reader's to set, and it disables rather
//! than hides: ground a watched read-at-place tree already covers — the folder
//! itself re-picked, or a rung of it — is watched, and an import of it is the
//! reader asking for its books again rather than asking the library to stop
//! looking. The switch locks on and says why, because the alternative is a sheet
//! whose defaults quietly un-track a folder by importing it. Turning a watch off
//! is the shelf's own right-click, which asks nothing of a walk
//! (`crate::services::library::set_folder_watch`).
//!
//! The lock is the READ-AT-PLACE ground's, and only while the run stays
//! read-at-place: a folder imported as copies is a different mode, whose watch
//! the sheet has already taken off with the switch it hides, and leaving a copy
//! watched would be a folder nothing can turn off any more — the shelf's menu
//! answers for read-at-place ground, because that is the only ground the sheet
//! offers the watch on.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use library_core::folder::{watching_over, FolderOpts, MIN_SIZE_CEIL, MIN_SIZE_FLOOR};
use library_core::scan::selectable_formats;
use reader_core::format::Format;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::controls::option_button::OptionButton;
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter};
use crate::components::primitives::form::row::Row;
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
    pub open: RwSignal<bool>,
    pub root: RwSignal<Option<String>>,
    toasts: RwSignal<Option<String>>,
}

impl ImportSheet {
    /// Create and provide the handles. Called once, by the page.
    pub fn provide() -> Self {
        let sheet = Self {
            open: RwSignal::new(false),
            root: RwSignal::new(None),
            toasts: RwSignal::new(None),
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
    /// surface that has no toast host of its own.
    pub fn toast(&self, message: String) {
        self.toasts.set(Some(message));
    }
}

/// Raise anything the sheet collected while it was closed. Called by the page,
/// which owns the app's one toast slot.
pub(crate) fn drain_sheet_toasts(state: AppState, sheet: ImportSheet) {
    if let Some(message) = sheet.toasts.get_untracked() {
        sheet.toasts.set(None);
        crate::services::library::toast(state, message);
    }
}

#[component]
pub(crate) fn ImportModal(state: AppState, sheet: ImportSheet) -> impl IntoView {
    // The options outlive the sheet being open: a reader who imports a second
    // folder usually wants it imported the same way as the first. The lane
    // arbitration and the Escape rule are the modal shell's (see
    // `crate::components::primitives::overlay::modal_shell`).
    let opts = RwSignal::new(FolderOpts::default());

    let in_place = Signal::derive(move || opts.with(|o| o.in_place));
    let watching = Signal::derive(move || opts.with(|o| o.watch));
    // Ground a watched read-at-place tree already covers, where the watch is the
    // folder's answer and not the sheet's question. Read on the OPEN as well as
    // on the root: the root survives the sheet closing, so a folder whose watch
    // the reader turned off from its own menu in between would be answered from
    // the last look this sheet took at the ledger.
    let watch_locked = Signal::derive(move || {
        sheet.open.get()
            && sheet
                .root
                .get()
                .is_some_and(|root| ground_is_watched(state, &root))
    });
    // What the switch shows, which is the value that lands: a locked watch is on
    // whatever the options signal was left holding by the last folder imported.
    let watching_on = Signal::derive(move || watching.get() || watch_locked.get());
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
        <ModalShell
            open=sheet.open
            aria_label="Import a folder"
            width="min(92vw, 480px)"
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

                    // The heading is this sheet's own — a sentence with no
                    // subtitle and a different rhythm from the six that ask a
                    // question. The body and the button row are everybody's.
                    <SheetBody>
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
                            <Row label="Read at place">
                                <Switch
                                    checked=in_place
                                    on_change=Callback::new(move |on| {
                                        opts.update(|o| {
                                            o.in_place = on;
                                            // Watching a copy is a question the
                                            // sheet does not ask, so turning the
                                            // mode off takes the answer with it —
                                            // a lock included, because the lock is
                                            // about ground the library READS, and
                                            // a run that copies instead is a
                                            // different mode rather than the same
                                            // folder watched harder.
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
                                                {move || {
                                                    if watch_locked.get() {
                                                        "This folder is already watched. Right-click \
                                                         its shelf to stop."
                                                            .to_string()
                                                    } else {
                                                        "Checks for new books when the app opens or \
                                                         you come back to it."
                                                            .to_string()
                                                    }
                                                }}
                                            </span>
                                        </div>
                                        // Rebuilt rather than reactive inside:
                                        // the row's title is a `String` prop, and
                                        // a lock that changed is a switch with a
                                        // different sentence on it.
                                        {move || {
                                            let locked = watch_locked.get();
                                            let title = if locked {
                                                "Already watched — the shelf's own menu turns it off"
                                            } else {
                                                "Watch for new books"
                                            };
                                            view! {
                                                <Switch
                                                    checked=watching_on
                                                    on_change=Callback::new(move |on| {
                                                        opts.update(|o| o.watch = on);
                                                    })
                                                    disabled=watch_locked
                                                    title=title.to_string()
                                                />
                                            }
                                        }}
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
                    </SheetBody>

                    <SheetFooter>
                        <Button
                            on_click=move |_| sheet.open.set(false)
                            variant=ButtonVariant::Ghost
                            title="Close without importing"
                        >
                            <span>"Cancel"</span>
                        </Button>
                        <Button
                            on_click=move |_| {
                                let (Some(root), mut options) = (
                                    sheet.root.get_untracked(),
                                    opts.get_untracked(),
                                ) else {
                                    return;
                                };
                                // The value that lands is the value the switch
                                // showed: on ground a watched tree covers the
                                // switch is locked ON and the options signal may
                                // still be holding the last folder's `false`, so
                                // the lock writes what the reader was looking at.
                                if options.in_place && watch_locked.get_untracked() {
                                    options.watch = true;
                                }
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
                    </SheetFooter>
        </ModalShell>
    }
}

/// Whether ground a watched read-at-place tree already covers is what the sheet
/// is pointed at: the folder itself, or a rung inside one. One question with the
/// folder run's own lock on the other side of it (`import::folder::resolve_folder`),
/// answered by the same function so the sheet and the ledger cannot disagree
/// about which imports are locked.
fn ground_is_watched(state: AppState, root: &str) -> bool {
    state
        .library
        .folders
        .with_untracked(|folders| watching_over(folders, root).is_some())
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
