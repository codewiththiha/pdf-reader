//! The sidebar rail's composition: the `<aside>` and its four slots, written
//! once and mounted from one of two places.
//!
//! The two places are not a stylistic choice. `.reader-bg` is a stacking
//! context (`position: relative` + `z-index: 0`), so everything inside it
//! paints below the title bar's band, which is a SIBLING of `.reader-bg` at
//! `--z-bar`. A docked rail wants that: the bar yields its left inset and the
//! rail keeps the corner. An overlay rail floats, and a floating rail that the
//! bar paints over loses its whole header to the glass — the close, search and
//! More buttons sit in the top 48px, exactly where the band is — so it mounts
//! as a sibling of `.reader-bg` instead, the same escape the settings modal
//! uses, where its own `--z-popover` outranks the bar and it covers the bar's
//! left corner rather than the other way round.
//!
//! Hence one component: the composition must not drift between the two mount
//! points, only the wrapper around it may.

use leptos::prelude::*;

use crate::components::sidebar::Sidebar;
use crate::components::sidebar::document_info::BookInfo;
use crate::components::sidebar::header::SidebarHeader;
use crate::components::sidebar::outline_view::SidebarOutline;
use crate::components::sidebar::shell::{SidebarPaint, request_reveal_active};
use crate::components::sidebar::switcher::PanelSwitcher;
use crate::components::sidebar::thumbnails_view::SidebarThumbs;
use crate::state::AppState;

#[component]
pub(crate) fn ReaderRail(
    state: AppState,
    /// The open/close slide bookkeeping, owned by the page so both mount points
    /// share one set of paint flags.
    paint: SidebarPaint,
    /// Freezes the width tween (Settings → Animations).
    #[prop(into)] no_slide: Signal<bool>,
) -> impl IntoView {
    let vs = state.reader;
    let sidebar = state.ui.sidebar;

    view! {
        <Sidebar
            mode=sidebar
            no_slide=no_slide
            header=move || view! { <SidebarHeader reader=vs sidebar=sidebar /> }
            info_row=move || view! { <BookInfo reader=vs covers=state.library.covers /> }
            panels=move || view! {
                <SidebarOutline
                    state=vs
                    sidebar=sidebar
                    shown=paint.show_outline
                    outro=paint.is_closed
                    intro=paint.intro
                />
                <SidebarThumbs
                    state=vs
                    sidebar=sidebar
                    // Cells mount with the aside. Cached thumbs blit during the
                    // slide; cold cells keep their skeleton until their capped
                    // render completes.
                    live=Signal::derive(move || paint.thumbs_live.get())
                    shown=paint.show_thumbs
                    outro=paint.is_closed
                    intro=paint.intro
                />
            }
            footer=move || view! {
                <PanelSwitcher
                    mode=sidebar
                    thumbs_active=paint.thumbs_active
                    outline_active=paint.outline_active
                    on_reveal=request_reveal_active
                />
            }
        />
    }
}
