//! Row 2 of the sidebar: book identity (cover + title + author + info).
//! Always visible while the sidebar is open and a document is Ready.

use leptos::prelude::*;

use pdf_engine::types::DocStatus;
use crate::components::{Icon, IconName};
use crate::state::AppState;

#[component]
pub(crate) fn BookInfo(state: AppState) -> impl IntoView {
    view! {
        <Show when=move || state.reader.document.status.get() == DocStatus::Ready>
            <div
                class="flex items-center gap-3 border-b border-line px-3 pb-3"
                data-tauri-drag-region="true"
            >
                {move || {
                    let path = state.reader.document.path.get().unwrap_or_default();
                    let cover = state
                        .library
                        .covers
                        .with(|covers| covers.get(&path).cloned());
                    match cover {
                        Some(c) => view! {
                            <img
                                class="h-12 w-10 rounded-sm border border-line/60 object-cover"
                                src=c.data_url
                                alt="Cover"
                                loading="lazy"
                            />
                        }
                        .into_any(),
                        None => view! {
                            <div class="flex h-12 w-10 items-center justify-center rounded-sm border border-line bg-surface">
                                <Icon name=IconName::Open size=14 />
                            </div>
                        }
                        .into_any(),
                    }
                }}
                <div class="min-w-0 flex-1" data-tauri-drag-region="true">
                    <p
                        class="truncate text-sm font-semibold text-ink"
                        data-tauri-drag-region="true"
                    >
                        {move || pdf_core::filename::display_name(
                            state.reader.document.title.get().as_deref(),
                            state.reader.document.path.get().as_deref(),
                        )
                        .unwrap_or_else(|| "No document".to_string())}
                    </p>
                    <p class="truncate text-xs text-muted">
                        {move || state.reader.document.author.get().unwrap_or_default()}
                    </p>
                </div>
                // Info: native tooltip with the full path is enough.
                <span
                    title=move || state.reader.document.path.get().unwrap_or_default()
                    class="text-muted"
                >
                    <Icon name=IconName::More size=14 />
                </span>
            </div>
        </Show>
    }
}
