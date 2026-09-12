//! The suggestion list under the library's search bar: the books a half-typed
//! query is probably about, best first, with the characters that matched lit up.
//!
//! A panel, not a page: it answers the keystroke the reader just made and gets
//! out of the way of the shelf, which is filtering live behind it — two answers
//! to one query, one for "narrow this shelf" and one for "take me to that
//! book". A row is the second answer: picking one reveals the book
//! (`crate::services::library::reveal_book` — its shelf, then the card itself,
//! scrolled to and lit), which is what a reader who typed a title meant.
//!
//! The ranking and the match spans are `library_core::query`'s, computed by
//! the bar at the keystroke and handed in: this file renders an answer, it does
//! not derive one, so a keystroke costs one pass over the library and not two.
//!
//! Pointer rows answer to POINTERDOWN, not click, and prevent its default: the
//! default of a press on the panel is to move focus out of the input, and the
//! input blurring closes the panel before a click ever lands. One handler
//! therefore does two jobs — it picks the row and it keeps the caret where the
//! reader was typing. The keyboard never visits these rows at all; the input
//! owns the arrows and the listbox is announced through it
//! (`aria-activedescendant`), the pattern a combobox is defined by.

use leptos::prelude::*;

use library_core::query::Suggestion;

use crate::features::library::cover_thumb::CoverThumb;
use crate::state::AppState;

/// Cut `text` at `spans` — sorted, disjoint char ranges — into alternating
/// plain and lit pieces: `("Dune", [(0, 2)])` becomes `["Du", lit] ["ne",
/// plain]`. Spans outside the text are ignored rather than trusted, because a
/// highlight is a courtesy and a courtesy that panics on a stale span takes the
/// bar with it. Pure so the walk is a host test and not a browser hunt.
fn pieces(text: &str, spans: &[(usize, usize)]) -> Vec<(String, bool)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<(String, bool)> = Vec::with_capacity(spans.len() * 2 + 1);
    let mut at = 0usize;
    let mut si = 0usize;
    while at < chars.len() {
        // The next hit that starts exactly here; anything else is a span the
        // text has no room for, and the walk steps over it.
        while si < spans.len() && spans[si].0 < at {
            si += 1;
        }
        if si < spans.len() && spans[si].0 == at && spans[si].1 > at {
            let end = spans[si].1.min(chars.len());
            out.push((chars[at..end].iter().collect(), true));
            at = end;
            si += 1;
        } else {
            let end = spans
                .get(si)
                .map(|s| s.0.max(at + 1))
                .unwrap_or(chars.len())
                .min(chars.len());
            out.push((chars[at..end].iter().collect(), false));
            at = end;
        }
    }
    out
}

/// Paint `text` with `spans` lit: one walk, a plain node between hits and one
/// accent node per hit.
fn highlighted(text: &str, spans: &[(usize, usize)]) -> Vec<AnyView> {
    pieces(text, spans)
        .into_iter()
        .map(|(chunk, lit)| {
            if lit {
                view! { <span class="lib-suggest-hit">{chunk}</span> }.into_any()
            } else {
                chunk.into_any()
            }
        })
        .collect()
}

/// The suggestions, and the one line under them that says what the panel
/// takes: the arrows choose, Enter goes to the book, Escape leaves.
#[component]
pub(crate) fn SearchSuggestions(
    state: AppState,
    suggestions: ReadSignal<Vec<Suggestion>>,
    /// The row the keyboard is on. Owned by the bar — the input answers the
    /// arrows — and written by the pointer here, so both hands move one cursor.
    active: RwSignal<usize>,
    /// Take the reader to a book: its id, on a press of its row.
    pick: Callback<String>,
) -> impl IntoView {
    view! {
        <div>
            <div id="lib-suggest-list" role="listbox" aria-label="Search suggestions" class="lib-suggest-list">
                <For
                    each=move || {
                        suggestions.get().into_iter().enumerate().collect::<Vec<_>>()
                    }
                    // Keyed by the id AND its spans: the id alone would keep a
                    // row alive across keystrokes wearing the highlight it was
                    // born with, and a suggestion whose lit characters never
                    // grow as the query does is a row that stopped listening.
                    key=|(_, s)| {
                        (
                            s.book.id.clone(),
                            s.title_spans.clone(),
                            s.author_spans.clone(),
                            s.path_spans.clone(),
                        )
                    }
                    children=move |(at, s)| {
                        let path = s.book.path().to_string();
                        let title = s.book.title();
                        let fallback_letter = title.chars().next().unwrap_or('?').to_string();
                        let author = s.book.author();
                        // The second line: the author when the book has one;
                        // the address when the address is what the query hit (a
                        // folder name, a file stem the document never carried);
                        // nothing otherwise. A row whose title IS its stem says
                        // the stem twice rather than saying nothing new.
                        let (sub_text, sub_spans) = match (&author, !s.path_spans.is_empty()) {
                            (Some(a), _) => (a.clone(), s.author_spans.clone()),
                            (None, true) => (path.clone(), s.path_spans.clone()),
                            (None, false) => (String::new(), Vec::new()),
                        };
                        let pick_id = s.book.id.clone();
                        let row_id = format!("lib-sug-{at}");

                        view! {
                            <div
                                id=row_id
                                role="option"
                                aria-selected=move || active.get() == at
                                title=path
                                class="lib-suggest-row"
                                class=("lib-suggest-row-active", move || active.get() == at)
                                // Pointerdown, and its default prevented: the
                                // pick happens before the input can blur, and
                                // the caret stays where the reader was typing.
                                on:pointerdown=move |ev: leptos::ev::PointerEvent| {
                                    ev.prevent_default();
                                    pick.run(pick_id.clone());
                                }
                                on:pointerenter=move |_| active.set(at)
                            >
                                <span class="lib-suggest-thumb" aria-hidden="true">
                                    <CoverThumb
                                        state=state
                                        path=Signal::stored(path.clone())
                                        alt=Signal::stored(String::new())
                                        img_class=""
                                        fallback=move || {
                                            view! { <span>{fallback_letter.clone()}</span> }
                                                .into_any()
                                        }
                                    />
                                </span>
                                <span class="lib-suggest-text">
                                    <span class="lib-suggest-title">
                                        {highlighted(&title, &s.title_spans)}
                                    </span>
                                    {(!sub_text.is_empty()).then(|| {
                                        view! {
                                            <span class="lib-suggest-sub">
                                                {highlighted(&sub_text, &sub_spans)}
                                            </span>
                                        }
                                    })}
                                </span>
                            </div>
                        }
                    }
                />
            </div>
            <div class="lib-suggest-foot" aria-hidden="true">
                "Choose with ↑ ↓ · Enter goes to the book · Esc closes"
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::pieces;

    #[test]
    fn an_unlit_text_is_one_plain_piece() {
        assert_eq!(pieces("Dune", &[]), vec![("Dune".to_string(), false)]);
    }

    #[test]
    fn hits_split_the_text_and_keep_every_character() {
        let cut = pieces("Mathematical Proofs", &[(0, 1), (2, 4), (13, 14)]);
        assert_eq!(
            cut,
            vec![
                ("M".to_string(), true),
                ("a".to_string(), false),
                ("th".to_string(), true),
                ("ematical ".to_string(), false),
                ("P".to_string(), true),
                ("roofs".to_string(), false),
            ]
        );
        let rejoined: String = cut.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(rejoined, "Mathematical Proofs", "nothing is lost or doubled");
    }

    #[test]
    fn spans_the_text_has_no_room_for_are_ignored() {
        // A highlight is a courtesy: a stale span past the end, or one that
        // starts before the walk has reached, cannot panic it or invent text.
        let cut = pieces("Dune", &[(2, 99)]);
        assert_eq!(
            cut,
            vec![("Du".to_string(), false), ("ne".to_string(), true)]
        );
        assert_eq!(pieces("Dune", &[(9, 12)]), vec![("Dune".to_string(), false)]);
        let joined: String = pieces("ab", &[(0, 1), (0, 2)])
            .iter()
            .map(|(s, _)| s.as_str())
            .collect();
        assert_eq!(joined, "ab");
    }
}
