//! Auto-center the current page in the thumbnail grid: the glide / grace /
//! debounce machinery.
//!
//! Scrolls through the panel's virtualizer — content coordinates,
//! layout-clamped — instead of hand-rolled offset arithmetic.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::state::ReaderState;
use crate::state::ui::SidebarMode;

use super::geometry::{CELL_W, GLIDE_DEBOUNCE_MS, GRACE_MS, THUMB_SCALE};

/// Panel-lifetime state shared between the thumbnail panel's effects and the
/// auto-center machinery.
pub struct AutoCenter {
    /// Last time the user physically drove the thumb panel.
    pub last_user_drive: Rc<Cell<f64>>,
    /// (was-this-panel-open, last-centered page).
    pub centered: StoredValue<(bool, u32), LocalStorage>,
    /// Handle for the debounced auto-center glide.
    pub glide_timer: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    /// The current self-re-arming glide step.
    pub glide_step: StoredValue<Option<Rc<dyn Fn()>>, LocalStorage>,
    /// The panel's virtualizer.
    pub virtualizer: Virtualizer,
}

/// Content-coordinate target that vertically centers `page`'s cell.
fn center_target(v: &Virtualizer, page: u32, aspect: f64, vh: f64) -> Option<f64> {
    if page == 0 || vh <= 0.0 {
        return None;
    }
    let idx = (page - 1) as usize;
    let row_top = v.offset_of(idx);
    let cell_h = CELL_W * aspect;
    Some(row_top + cell_h / 2.0 - vh / 2.0)
}

impl AutoCenter {
    /// Create the bundle around the panel's virtualizer.
    pub fn new(virtualizer: Virtualizer) -> Self {
        Self {
            last_user_drive: Rc::new(Cell::new(f64::NEG_INFINITY)),
            centered: StoredValue::new_local((false, 0u32)),
            glide_timer: StoredValue::new_local(None::<TimeoutHandle>),
            glide_step: StoredValue::new_local(None::<Rc<dyn Fn()>>),
            virtualizer,
        }
    }

    /// Install the auto-center effects.
    pub fn install(self, state: ReaderState, sidebar: RwSignal<SidebarMode>) {
        let auto = self;

        {
            let reveal_drive = auto.last_user_drive.clone();
            let v = auto.virtualizer.clone();
            Effect::new(move |_| {
                let reveal_drive = reveal_drive.clone();
                let v = v.clone();
                let handle = window_event_listener(
                    leptos::ev::Custom::new("pdfreader:reveal-active"),
                    move |_: web_sys::CustomEvent| {
                        if sidebar.get_untracked() != SidebarMode::Thumbs {
                            return;
                        }
                        let vh = v.viewport().get_untracked().main;
                        let page = state.viewer.page.get_untracked();
                        let Some(target) = center_target(&v, page, aspect(state), vh) else {
                            return;
                        };
                        reveal_drive.set(f64::NEG_INFINITY);
                        v.scroll_to_offset(target, ScrollMode::Smooth);
                    },
                );
                on_cleanup(move || handle.remove());
            });
        }

        Effect::new(move |_| {
            let in_thumbs = sidebar.get() == SidebarMode::Thumbs;
            let page = state.viewer.page.get();
            let vh = auto.virtualizer.viewport().get().main;
            let (was_open, _prev_page) = auto.centered.get_value();
            if !in_thumbs {
                auto.centered.set_value((false, 0));
                return;
            }
            if vh <= 0.0 || page == 0 {
                return;
            }

            let just_opened = !was_open;
            auto.centered.set_value((true, page));

            let Some(target) = center_target(&auto.virtualizer, page, aspect(state), vh) else {
                return;
            };
            let cur = auto.virtualizer.scroll_offset().get_untracked();
            if (target - cur).abs() <= 1.0 {
                if let Some(handle) = auto.glide_timer.get_value() {
                    handle.clear();
                    auto.glide_timer.set_value(None);
                }
                return;
            }

            let in_grace =
                !just_opened && js_sys::Date::now() - auto.last_user_drive.get() < GRACE_MS;
            let behavior = if just_opened {
                ScrollMode::Instant
            } else {
                ScrollMode::Auto
            };

            let step_handle = auto.glide_step;
            let step_state = state;
            let step_sidebar = sidebar;
            let step_v = auto.virtualizer.clone();
            let step_drive = auto.last_user_drive.clone();
            let step_timer = auto.glide_timer;
            let step_page = page;
            let step_aspect = aspect(state);
            let step: Rc<dyn Fn()> = Rc::new(move || {
                let now = js_sys::Date::now();
                let elapsed = now - step_drive.get();
                let in_thumbs_now = step_sidebar.get_untracked() == SidebarMode::Thumbs;
                let page_now = step_state.viewer.page.get_untracked();
                let vh_now = step_v.viewport().get_untracked().main;
                let cur_now = step_v.scroll_offset().get_untracked();
                let target_now = center_target(&step_v, step_page, step_aspect, vh_now);
                if !in_thumbs_now
                    || page_now != step_page
                    || target_now.is_none_or(|t| (t - cur_now).abs() <= 1.0)
                {
                    step_timer.set_value(None);
                    return;
                }
                let target_now = target_now.unwrap();
                if elapsed < GRACE_MS {
                    let next = step_handle.get_value();
                    let handle = next.and_then(|next| {
                        set_timeout_with_handle(
                            move || next(),
                            Duration::from_millis((GRACE_MS - elapsed + 50.0) as u64),
                        )
                        .ok()
                    });
                    step_timer.set_value(handle);
                    return;
                }
                step_v.scroll_to_offset(target_now, behavior);
                step_timer.set_value(None);
                let prefetch_page = step_page;
                leptos::task::spawn_local(async move {
                    for page in prefetch_page.saturating_sub(2)..=prefetch_page + 8 {
                        pdf_engine::api::prefetch_thumb(page, THUMB_SCALE).await;
                    }
                });
            });
            auto.glide_step.set_value(Some(step.clone()));

            if let Some(handle) = auto.glide_timer.get_value() {
                handle.clear();
                auto.glide_timer.set_value(None);
            }
            let delay = if just_opened {
                0
            } else if in_grace {
                (GRACE_MS - (js_sys::Date::now() - auto.last_user_drive.get()) + 60.0) as u64
            } else {
                GLIDE_DEBOUNCE_MS
            };
            let fire = step.clone();
            let handle = set_timeout_with_handle(move || fire(), Duration::from_millis(delay)).ok();
            let glide_timer = auto.glide_timer;
            let glide_step = auto.glide_step;
            auto.glide_timer.set_value(handle);
            on_cleanup(move || {
                if let Some(handle) = glide_timer.get_value() {
                    handle.clear();
                    glide_timer.set_value(None);
                }
                glide_step.set_value(None);
            });
        });
    }
}

fn aspect(state: ReaderState) -> f64 {
    state
        .document
        .page1_size
        .get()
        .map(|size| {
            if size.width > 0.0 {
                size.height / size.width
            } else {
                0.75
            }
        })
        .unwrap_or(0.75)
}
