//! The import dock: bottom-left, one card per run, a ring per card.
//!
//! Not a toast, and deliberately not mounted on the toast anchor. A toast is a
//! sentence the app says and takes back; an import is a thing the reader started
//! and wants to keep an eye on while they do something else. So the dock sits in
//! the corner, clear of both the shelf's centre and the app's one toast slot,
//! stacks, and stays until the run is finished and has been looked at.
//!
//! The ring is the only new animated geometry on the page, and it animates one
//! property (`stroke-dashoffset`) that composites without a layout: a two
//! thousand-file import moves a number and nothing else repaints.

use std::time::Duration;

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_chrome::layers::TOAST;

use crate::services::library::dismiss_task;
use crate::state::library::TaskPhase;
use crate::state::AppState;

/// How long a finished card stays up. Long enough to read, short enough that a
/// second import does not stack onto a stale one.
const HOLD_MS: u64 = 1600;

/// The ring's circumference at `r=15`, which is what turns a fraction into a dash
/// offset. Written down once here and once in `styles/library.css`, where the
/// dash array lives; the two have to agree or the ring ends short of full.
const CIRCUMFERENCE: f64 = 94.248;

/// The ring's markup, as a static string.
///
/// `inner_html` rather than `view!` because the app's one other SVG (the icon
/// sprite in `app_chrome::icon`) does the same, and because it is what makes the
/// transition work: the two circles are created once and the percentage is a
/// custom property on the wrapper, so a beat moves a number the browser
/// interpolates instead of replacing the node that was interpolating it.
const RING: &str = "<svg viewBox='0 0 36 36' width='36' height='36' aria-hidden='true'>\
<circle class='import-ring-track' cx='18' cy='18' r='15'/>\
<circle class='import-ring-fill' cx='18' cy='18' r='15'/>\
</svg>";

#[component]
pub(crate) fn ProgressDock(state: AppState) -> impl IntoView {
    view! {
        <div class=format!("import-dock pointer-events-none {TOAST}")>
            <For
                each=move || state.library.tasks.get()
                key=|task| task.id.clone()
                let:task
            >
                <DockCard state=state id=task.id />
            </For>
        </div>
    }
}

/// One card. Reads its task back out of the list by id rather than rendering the
/// row it was handed: `For` keys on the id, so a beat that only changes a count
/// would otherwise never reach the card.
#[component]
fn DockCard(state: AppState, id: String) -> impl IntoView {
    // Cloned before the signal takes the original: three closures below each need
    // the id, and a `Signal::derive` that owns it is the first of them.
    let timer_id = id.clone();
    let close_id = id.clone();
    let task = Signal::derive(move || {
        state
            .library
            .tasks
            .with(|tasks| tasks.iter().find(|t| t.id == id).cloned())
    });

    // A finished card leaves on a timer of its own, re-armed whenever the task
    // changes and cleared when the card unmounts — so a beat that lands after the
    // finish cannot leave two timers racing over one row.
    Effect::new(move |_| {
        let Some(current) = task.get() else {
            return;
        };
        if !current.phase.is_finished() {
            return;
        }
        let finished_id = timer_id.clone();
        let handle = set_timeout_with_handle(
            move || dismiss_task(state, &finished_id),
            Duration::from_millis(HOLD_MS),
        )
        .ok();
        on_cleanup(move || {
            if let Some(handle) = handle {
                handle.clear();
            }
        });
    });

    view! {
        <div
            class="import-card surface-toast pointer-events-auto"
            class=("import-card-failed", move || {
                task.get()
                    .is_some_and(|t| t.phase == TaskPhase::Failed)
            })
        >
            <span class="import-ring-wrap">
                <span
                    class=move || {
                        // One class carries the whole indeterminate story: the
                        // ring spins and stops drawing a fraction, because a scan
                        // has no total to draw one of. Both reduced-motion nets
                        // kill the spin globally, and a frozen ring still reads
                        // as "working" because the label next to it says so in
                        // words.
                        let base = "import-ring";
                        let Some(current) = task.get() else {
                            return base.to_string();
                        };
                        if current.phase == TaskPhase::Scanning {
                            format!("{base} import-ring-spin")
                        } else {
                            base.to_string()
                        }
                    }
                    style=move || {
                        let offset = task
                            .get()
                            .and_then(|t| t.fraction())
                            .map(|f| CIRCUMFERENCE * (1.0 - f))
                            .unwrap_or(CIRCUMFERENCE);
                        format!("--ring-offset:{offset:.2}")
                    }
                    role="progressbar"
                    aria-label="Import progress"
                    aria-valuemin="0"
                    aria-valuemax="100"
                    aria-valuenow=move || {
                        task.get()
                            .and_then(|t| t.percent())
                            .map(|p| p.to_string())
                            .unwrap_or_default()
                    }
                    inner_html=RING
                ></span>
                <span class="import-ring-value">
                    {move || {
                        let current = task.get()?;
                        match current.phase {
                            TaskPhase::Done => {
                                Some(view! { <Icon name=IconName::Check size=15 /> }.into_any())
                            }
                            TaskPhase::Failed => {
                                Some(view! { <Icon name=IconName::Close size=15 /> }.into_any())
                            }
                            _ => current.percent().map(|percent| {
                                view! { <span class="import-ring-percent">{percent}</span> }
                                    .into_any()
                            }),
                        }
                    }}
                </span>
            </span>

            <span class="min-w-0 flex-1">
                <span class="block truncate text-xs font-medium text-ink">
                    {move || {
                        task.get()
                            .map(|t| t.headline())
                            .unwrap_or_else(|| "Importing…".to_string())
                    }}
                </span>
                <span class="block max-w-40 truncate text-[11px] text-muted">
                    {move || {
                        let Some(current) = task.get() else {
                            return String::new();
                        };
                        match current.phase {
                            TaskPhase::Failed => current
                                .error
                                .clone()
                                .unwrap_or_else(|| current.label.clone()),
                            TaskPhase::Done => current.label.clone(),
                            // The file being worked on is the useful line while
                            // a run is going; the folder's name is the fallback
                            // for the beats that carry no file.
                            _ => {
                                if current.name.is_empty() {
                                    current.label.clone()
                                } else {
                                    current.name.clone()
                                }
                            }
                        }
                    }}
                </span>
            </span>

            <button
                class="import-card-close"
                type="button"
                title="Dismiss"
                aria-label="Dismiss this import"
                on:click=move |_| {
                    let Some(current) = task.get_untracked() else {
                        return;
                    };
                    // Only a finished card can be dismissed by hand: cancelling a
                    // walk half way through would leave the library holding half a
                    // shelf, and the shell has no cancel to honour.
                    if current.phase.is_finished() {
                        dismiss_task(state, &close_id);
                    }
                }
            >
                <Icon name=IconName::Close size=11 />
            </button>
        </div>
    }
}
