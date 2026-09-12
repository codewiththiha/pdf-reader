#[cfg(test)]
mod tests {
    use super::moves::{insert_many, place_many, reorder_root};
    use super::shelf_departure::{departing_book_ids, departing_sets, return_path, target_is_family};
    use super::*;
    use std::collections::{BTreeMap, HashSet};

    use library_core::book::Row;
    use library_core::shelf::{Shelf, ALL_SHELF};

    use library_core::book::{Book, Fingerprint, Origin};
    use library_core::folder::{FolderOpts, WatchedFolder};
    use library_core::shelf::ShelfKind;
    use reader_core::format::Format;

    /// A linked row. Markdown rather than PDF so nothing that reads these lists
    /// ever asks the cover queue to render one — a host test has no engine.
    fn row(id: &str) -> Row {
        library_core::testkit::markdown_row(id)
    }

    fn list() -> Vec<Row> {
        vec![row("a"), row("b"), row("c"), row("d")]
    }

    fn ids(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.id()).collect()
    }

    fn owned(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    // -----------------------------------------------------------------------
    // reorder_root — "All" IS the library's own list, so a drop on the root
    // crumb re-orders rows rather than filing them anywhere.
    // -----------------------------------------------------------------------

    #[test]
    fn a_drop_on_the_root_puts_one_row_where_the_reader_pointed() {
        let mut rows = list();
        reorder_root(&mut rows, &owned(&["d"]), Some(1));
        assert_eq!(ids(&rows), vec!["a", "d", "b", "c"]);
    }

    #[test]
    fn the_index_counts_the_list_as_it_was_before_the_lift() {
        // The whole reason `insert_many` takes a `shift`. "a" and "b" sat at 0
        // and 1, so lifting them moves "d" from index 3 to index 1 — and the
        // reader pointed at the slot "d" occupied while they were holding the
        // two. That is the slot they land in, not two further down the list the
        // lift just shortened.
        let mut rows = list();
        reorder_root(&mut rows, &owned(&["a", "b"]), Some(3));
        assert_eq!(ids(&rows), vec!["c", "a", "b", "d"]);
    }

    #[test]
    fn a_drop_past_the_end_appends() {
        let mut rows = list();
        reorder_root(&mut rows, &owned(&["a"]), Some(99));
        assert_eq!(ids(&rows), vec!["b", "c", "d", "a"]);
    }

    #[test]
    fn an_append_keeps_the_payload_s_order_not_the_list_s() {
        // A set has no order, so the payload is sorted into the level's own
        // order on the way out — and the payload's order IS the reader's, which
        // is why the sort is by position in `row_ids` and not by the position
        // each row used to hold. Putting them back in the list's order would be
        // a drop that quietly shuffled the hand.
        let mut rows = list();
        reorder_root(&mut rows, &owned(&["c", "a"]), None);
        assert_eq!(ids(&rows), vec!["b", "d", "c", "a"]);
    }

    #[test]
    fn a_row_the_list_does_not_hold_is_not_invented() {
        // A drag can outlive a row: a focus rescan or another surface's removal
        // can take it between the lift and the drop. A hole in the grid would be
        // worse than an id quietly dropped.
        let mut rows = list();
        reorder_root(&mut rows, &owned(&["gone", "b"]), Some(0));
        assert_eq!(ids(&rows), vec!["b", "a", "c", "d"]);
    }

    #[test]
    fn a_link_is_reordered_by_its_own_id_like_any_other_row() {
        // "All" holds links as well as books, and a drag of one is a question
        // about a position rather than about content.
        let mut rows = vec![
            row("a"),
            Row::link("l1".into(), "Dune".into(), "a".into(), 1),
            row("b"),
        ];
        reorder_root(&mut rows, &owned(&["l1"]), Some(0));
        assert_eq!(ids(&rows), vec!["l1", "a", "b"]);
    }

    #[test]
    fn an_empty_set_leaves_the_list_alone() {
        let mut rows = list();
        reorder_root(&mut rows, &[], Some(0));
        assert_eq!(ids(&rows), vec!["a", "b", "c", "d"]);
    }

    // -----------------------------------------------------------------------
    // place_many — a shelf's member list, which is ids and nothing else.
    // -----------------------------------------------------------------------

    #[test]
    fn a_book_already_on_the_shelf_is_moved_not_duplicated() {
        let mut members = owned(&["a", "b", "c"]);
        place_many(&mut members, &owned(&["a"]), Some(2));
        assert_eq!(
            members,
            vec!["b", "a", "c"],
            "one membership, in the slot the drop named"
        );
    }

    #[test]
    fn the_shift_is_counted_per_book_rather_than_for_the_batch() {
        // "a" and "c" were at 0 and 2, both below the drop's index 3, so the
        // lift takes two off it. "b" was not on the shelf at all and adds
        // nothing to the count — counting the batch instead of the members
        // would have landed the three one slot early.
        let mut members = owned(&["a", "x", "c", "y"]);
        place_many(&mut members, &owned(&["a", "b", "c"]), Some(3));
        assert_eq!(members, vec!["x", "a", "b", "c", "y"]);
    }

    #[test]
    fn filing_with_no_index_appends_in_order() {
        let mut members = owned(&["a"]);
        place_many(&mut members, &owned(&["b", "c"]), None);
        assert_eq!(members, vec!["a", "b", "c"]);
    }

    // -----------------------------------------------------------------------
    // insert_many — the step both of the above land on.
    // -----------------------------------------------------------------------

    #[test]
    fn each_item_lands_after_the_last_rather_than_all_at_one_place() {
        let mut list: Vec<&str> = vec!["x", "y"];
        insert_many(&mut list, ["a", "b", "c"].into_iter(), Some(1), 0);
        assert_eq!(list, vec!["x", "a", "b", "c", "y"], "not reversed");
    }

    #[test]
    fn an_index_past_the_end_clamps_per_item() {
        let mut list: Vec<&str> = vec!["x"];
        insert_many(&mut list, ["a", "b"].into_iter(), Some(99), 0);
        assert_eq!(list, vec!["x", "a", "b"]);
    }

    #[test]
    fn a_shift_larger_than_the_index_lands_at_the_front() {
        let mut list: Vec<&str> = vec!["x", "y"];
        insert_many(&mut list, ["a"].into_iter(), Some(1), 4);
        assert_eq!(list, vec!["a", "x", "y"]);
    }

    #[test]
    fn a_removal_deletes_the_app_s_own_copy_by_default() {
        // A copy the app made for a book that is no longer in the library is a
        // file nothing will ever read again; the sheet is where a reader says
        // otherwise, and it is the only place that does.
        assert!(PurgeOpts::default().delete_store_copy);
    }

    // -----------------------------------------------------------------------
    // converts_on_move_to — the departure, and the ground it measures against.
    // -----------------------------------------------------------------------

    fn fp(n: u32) -> Fingerprint {
        Fingerprint {
            size: u64::from(n),
            mtime_ms: u64::from(n),
            head_hash: n,
        }
    }

    fn linked_at(id: &str, path: &str, n: u32) -> Row {
        Row::Book(Book::new(
            id.to_string(),
            fp(n),
            Format::Markdown,
            Origin::Linked {
                src: path.to_string(),
            },
            0,
        ))
    }

    fn stored_at(id: &str, src: &str, store: &str, n: u32) -> Row {
        Row::Book(Book::new(
            id.to_string(),
            fp(n),
            Format::Markdown,
            Origin::Stored {
                src: Some(src.to_string()),
                store: store.to_string(),
            },
            0,
        ))
    }

    /// `/books` cut into `Fiction` cut into `Fiction/SciFi`, read in place and
    /// holding fingerprint `n` — the three-level shelf the departure rule was
    /// wrong about.
    fn nested(n: u32) -> WatchedFolder {
        WatchedFolder {
            id: "f1".into(),
            root: "/books".into(),
            opts: FolderOpts::default(),
            placed: HashSet::from([fp(n)]),
            ignored: Vec::new(),
            last_seen: Vec::new(),
            shelf_map: BTreeMap::from([
                (String::new(), "shelf1".to_string()),
                ("Fiction".to_string(), "shelf2".to_string()),
                ("Fiction/SciFi".to_string(), "shelf3".to_string()),
            ]),
            scanned_ms: 0,
        }
    }

    /// The reported shape, written into a fresh state: one read-in-place
    /// folder, three rungs, and the linked book the deepest one placed.
    ///
    /// Takes the state rather than making one because the `Owner` a signal
    /// needs has to outlive the write, and a helper that minted its own would
    /// drop it on the way out.
    fn set_nested(state: AppState) {
        state.library.folders.set(vec![nested(7)]);
        state
            .library
            .books
            .set(vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)]);
    }

    #[test]
    fn a_drag_to_another_rung_of_the_same_folder_is_a_departure() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        set_nested(state);
        // shelf3 is the rung the folder's own tree names for this address, so a
        // drag UP to shelf2 leaves the ground even though shelf2 is a rung of
        // the very folder that placed the book. Reading the tie as the folder's
        // shelf tree instead — "any shelf this folder owns" — left the row
        // linked at an address it had been dragged off, and the next import of
        // that file found a living row there, asked the reader to choose between
        // a collision and a highlight, and lit up the row that had moved rather
        // than bringing the file home to the rung it belongs on.
        assert!(converts_on_move_to(state, "b1", "shelf2"));
        assert!(converts_on_move_to(state, "b1", "shelf1"));
        // And a shelf the folder's map does not name for this address at all is
        // ground the book has left whatever it is.
        assert!(converts_on_move_to(state, "b1", "elsewhere"));
    }

    #[test]
    fn a_reorder_on_the_book_s_own_rung_copies_nothing() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        set_nested(state);
        // The cheapest drag in the library must stay the cheapest: re-ordering
        // the books a folder placed, on the rung it placed them on, is the
        // folder's own business. Copying here would spend a reader's disk on a
        // move that changed no ground at all.
        assert!(!converts_on_move_to(state, "b1", "shelf3"));
    }

    #[test]
    fn the_root_and_the_reader_s_own_shelves_are_nobody_s_ground() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        set_nested(state);
        // "All" is the library's own list rather than a shelf, so no folder's
        // map can name it — this is the departure the rule always had.
        assert!(converts_on_move_to(state, "b1", ALL_SHELF));
        // And a virtual shelf is the reader's own by construction.
        assert!(converts_on_move_to(state, "b1", "mine"));
    }

    #[test]
    fn a_rung_the_reader_deleted_is_ground_the_book_has_left() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let mut folder = nested(7);
        // The shelf is gone, so the map no longer names a rung for the address.
        // The ledger is still waiting on the fingerprint, which is what makes
        // this a departure rather than a book nobody answers for: an empty map
        // answer and an empty list of folders are not the same fact.
        folder.shelf_map.remove("Fiction/SciFi");
        state.library.folders.set(vec![folder]);
        state
            .library
            .books
            .set(vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)]);
        assert!(converts_on_move_to(state, "b1", "shelf2"));
        assert!(converts_on_move_to(state, "b1", "shelf3"));
    }

    #[test]
    fn a_folder_that_does_not_group_has_one_ground_for_every_file() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let mut folder = nested(7);
        folder.opts.groups = false;
        // Everything lands flat, so the root rung is the ground for the whole
        // tree and a drag between the folder's own shelves is a re-order.
        folder.shelf_map = BTreeMap::from([(String::new(), "flat".to_string())]);
        state.library.folders.set(vec![folder]);
        state
            .library
            .books
            .set(vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)]);
        assert!(!converts_on_move_to(state, "b1", "flat"));
        assert!(converts_on_move_to(state, "b1", "shelf2"));
    }

    #[test]
    fn only_a_linked_book_of_a_reading_folder_owes_the_copy() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        let mut copying = nested(7);
        copying.id = "f2".into();
        copying.opts.in_place = false;
        state.library.folders.set(vec![nested(7), copying]);
        state.library.books.set(vec![
            linked_at("b1", "/books/Fiction/SciFi/dune.md", 7),
            // The copy a departure made: the library's own, so it simply moves.
            stored_at("b2", "/books/Fiction/SciFi/dune.md", "/store/b2.md", 9),
            // A loose file the reader dropped: no ledger is waiting on it.
            linked_at("b3", "/elsewhere/loose.md", 11),
        ]);

        assert!(converts_on_move_to(state, "b1", "shelf2"));
        assert!(!converts_on_move_to(state, "b2", "shelf2"));
        assert!(!converts_on_move_to(state, "b3", "shelf2"));
        // A row the library does not hold owes nothing at all.
        assert!(!converts_on_move_to(state, "gone", "shelf2"));
    }

    /// A shelf cut from a watched folder's tree, which is what makes a landing
    /// on it look like a return.
    fn folder_shelf(id: &str, folder_id: &str, rel: &str) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: ShelfKind::Folder {
                folder_id: folder_id.to_string(),
                rel: Some(rel.to_string()),
            },
            books: Vec::new(),
            parent: None,
            manual_parent: false,
        }
    }

    /// `f1` read in place, holding fingerprint 7, with the moved-out log a
    /// departure of that file writes: the name the shelf showed, the rung it was
    /// filed on, and no row bound to it yet.
    fn folder_with_moved_log() -> WatchedFolder {
        WatchedFolder {
            id: "f1".into(),
            root: "/books".into(),
            opts: FolderOpts::default(),
            placed: HashSet::from([fp(7)]),
            ignored: vec![Tombstone {
                fp: fp(7),
                title: Some("Dune".to_string()),
                format: Format::Markdown,
                last_path: "/books/Fiction/SciFi/dune.md".to_string(),
                shelf_id: Some("shelf3".to_string()),
                removed_ms: 5,
                moved: true,
                returned_row: None,
            }],
            last_seen: Vec::new(),
            shelf_map: BTreeMap::from([
                ("Fiction".to_string(), "shelf2".to_string()),
                ("Fiction/SciFi".to_string(), "shelf3".to_string()),
            ]),
            scanned_ms: 0,
        }
    }

    #[test]
    fn a_departure_s_landing_does_not_bind_the_log_it_just_wrote() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        // The row a departure leaves behind: the library's own copy, still
        // wearing the name the folder's log remembers.
        let mut rows = vec![stored_at("b1", "/books/Fiction/SciFi/dune.md", "/store/b1.md", 9)];
        find_book_mut(&mut rows, "b1").unwrap().title = Some("Dune".to_string());
        state.library.books.set(rows);
        state
            .library
            .shelves
            .set(vec![folder_shelf("shelf2", "f1", "Fiction")]);
        state.library.folders.set(vec![folder_with_moved_log()]);
        let bound = |state: AppState| {
            state.library.folders.with_untracked(|folders| {
                folders[0].ignored[0].returned_row.is_some()
            })
        };

        // The gesture that made the copy lands it on ANOTHER rung of the same
        // folder — which is the shape of a return without being one. Binding
        // here would spend the log on the row that just left, and from then on
        // every import of the OS file would light the copy up instead of
        // bringing the linked book home to the rung it belongs on.
        move_row(state, "b1", "shelf2", None, true);
        assert!(!bound(state), "a departure is not a return");
        let filed = state.library.shelves.with_untracked(|shelves| {
            shelves[0].books.iter().any(|id| id == "b1")
        });
        assert!(filed, "and the move itself still happened");

        // The NEXT drag of the same row back is the return the bind exists for,
        // and it binds exactly as it always did.
        move_row(state, "b1", "shelf2", None, false);
        assert!(bound(state), "a later gesture binds the log to the row by name");
    }

    // -----------------------------------------------------------------------
    // The shelf's departure: the same rule the book's rides, read one level up.
    // -----------------------------------------------------------------------

    /// `/books` read in place and cut into three rungs, with four fingerprints
    /// placed: the deep book, one on the middle rung, one on the root, and one
    /// the reader also showed on a rung that is not its own.
    fn reading_folder() -> WatchedFolder {
        WatchedFolder {
            id: "f1".into(),
            root: "/books".into(),
            opts: FolderOpts::default(),
            placed: HashSet::from([fp(7), fp(8), fp(9), fp(14)]),
            ignored: Vec::new(),
            last_seen: Vec::new(),
            shelf_map: BTreeMap::from([
                (String::new(), "r".to_string()),
                ("Fiction".to_string(), "fic".to_string()),
                ("Fiction/SciFi".to_string(), "sf".to_string()),
            ]),
            scanned_ms: 0,
        }
    }

    fn own(id: &str, name: &str, parent: Option<&str>, books: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: name.to_string(),
            kind: ShelfKind::Virtual,
            books: books.iter().map(|b| b.to_string()).collect(),
            parent: parent.map(str::to_string),
            manual_parent: false,
        }
    }

    fn rung(id: &str, folder_id: &str, rel: Option<&str>, parent: Option<&str>, books: &[&str]) -> Shelf {
        Shelf {
            kind: ShelfKind::Folder {
                folder_id: folder_id.to_string(),
                rel: rel.map(str::to_string),
            },
            ..own(id, id, parent, books)
        }
    }

    /// The tree the questions below ask of: root "r", "Fiction", "Fiction/SciFi",
    /// one of the reader's own shelves filed inside the middle rung, and one
    /// standing outside the tree.
    fn tree() -> Vec<Shelf> {
        vec![
            rung("r", "f1", None, None, &["top", "shown2"]),
            rung("fic", "f1", Some("Fiction"), Some("r"), &["mid"]),
            rung("sf", "f1", Some("Fiction/SciFi"), Some("fic"), &["deep", "shown2", "loose", "kept"]),
            own("mine", "Mine", Some("fic"), &[]),
            own("elsewhere", "Elsewhere", None, &[]),
        ]
    }

    fn rows() -> Vec<Row> {
        vec![
            linked_at("top", "/books/top.md", 9),
            linked_at("mid", "/books/Fiction/other.md", 8),
            linked_at("deep", "/books/Fiction/SciFi/dune.md", 7),
            // A second membership of a root-rung book on the deep rung: its
            // ground is the root, so the deep rung's departure does not take it.
            linked_at("shown2", "/books/top2.md", 14),
            // A loose file the reader filed onto the rung: no folder placed it.
            linked_at("loose", "/loose/x.md", 12),
            // A stored book: the library's own already, so it simply rides.
            stored_at("kept", "/books/Fiction/SciFi/old.md", "/store/kept.md", 13),
        ]
    }

    #[test]
    fn a_departing_rung_carries_the_books_standing_on_the_rungs_it_takes() {
        let shelves = tree();
        let folder = reading_folder();
        // "sf" departs: its subtree is itself, and the folder's own rung for
        // this file's address is the rung the map names.
        let (subtree, rungs) = departing_sets(&shelves, "f1", "sf");
        assert!(subtree.contains("sf"));
        assert!(rungs.contains("sf"));
        let ids = departing_book_ids(&rows(), &shelves, &folder, &rungs, &subtree);
        assert_eq!(ids, vec!["deep".to_string()], "only the book whose OWN rung is the one leaving");
    }

    #[test]
    fn a_book_shown_on_a_departing_rung_keeps_its_link_when_its_ground_stays() {
        let shelves = tree();
        let folder = reading_folder();
        // "fic" departs, and "shown2" is a member of "sf" below it — but its
        // address stands on the ROOT rung, which is not departing, so the
        // ledger still answers for it and it keeps reading its file at its place.
        let (subtree, rungs) = departing_sets(&shelves, "f1", "fic");
        assert!(subtree.contains("sf"), "the subtree rides with the shelf the hand named");
        assert!(subtree.contains("mine"), "and the reader's own shelf inside it rides too");
        assert!(!subtree.contains("r"), "the rung above is not part of the ride");
        let ids = departing_book_ids(&rows(), &shelves, &folder, &rungs, &subtree);
        assert_eq!(
            ids,
            vec!["mid".to_string(), "deep".to_string()],
            "the middle rung's own book and the deep one; not the shown one, not the loose one, not the stored one"
        );
        assert!(!ids.iter().any(|id| id == "shown2"));
        assert!(!ids.iter().any(|id| id == "loose"));
        assert!(!ids.iter().any(|id| id == "kept"));
    }

    #[test]
    fn the_ask_names_the_copies_the_level_s_next_free_names() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        // Two rungs of two trees, both named "Fiction", landing on a level
        // that already holds a "Fiction" of the reader's own: the counter
        // starts at one and the second promise counts the first.
        let mut other = reading_folder();
        other.id = "f2".into();
        other.root = "/more".into();
        other.placed = HashSet::new();
        other.shelf_map = BTreeMap::from([
            (String::new(), "r2".to_string()),
            ("Fiction".to_string(), "fic2".to_string()),
        ]);
        state.library.folders.set(vec![reading_folder(), other]);
        let mut tree = tree();
        tree[1].name = "Fiction".to_string();
        let mut shelves = vec![
            own("to", "To", None, &[]),
            own("held", "Fiction", Some("to"), &[]),
        ];
        shelves.extend(tree);
        shelves.push(Shelf {
            id: "fic2".to_string(),
            name: "Fiction".to_string(),
            kind: ShelfKind::Folder {
                folder_id: "f2".to_string(),
                rel: Some("Fiction".to_string()),
            },
            books: Vec::new(),
            parent: None,
            manual_parent: false,
        });
        state.library.shelves.set(shelves);
        state.library.books.set(rows());

        let ask = ShelfDepartureAsk::of(
            state,
            vec!["fic".to_string(), "fic2".to_string()],
            Some("to".to_string()),
            None,
        )
        .expect("two departing shelves are a question");
        assert_eq!(ask.departing.len(), 2);
        assert_eq!(ask.departing[0].copy_name, "Fiction_1");
        assert_eq!(ask.departing[1].copy_name, "Fiction_2");
        assert_eq!(ask.departing[0].folder_name, "books");
        assert_eq!(ask.departing[1].folder_name, "more");
        // "mid" stands on "fic", and "deep" stands below it inside the ride:
        // both become the library's copies, and the second tree carries none.
        assert_eq!(ask.departing[0].books, 2);
        assert_eq!(ask.departing[1].books, 0);
        assert!(
            ask.returns.is_empty(),
            "a reader's own shelf is nobody's family, so the drop owes no way home"
        );
    }

    /// f1's tree with a displaced member: "f3" reading "/books/Fiction/SciFi"
    /// on its own, its root shelf "s3" at the top level, while f1's tree holds
    /// the root and "Fiction" but no rung for the deep directory — the shape
    /// a removed rung and a subfolder imported on its own leave behind.
    fn family_state() -> (Vec<Shelf>, Vec<WatchedFolder>) {
        let mut tree = reading_folder();
        tree.shelf_map.remove("Fiction/SciFi");
        let mut member = reading_folder();
        member.id = "f3".into();
        member.root = "/books/Fiction/SciFi".into();
        member.shelf_map = BTreeMap::from([(String::new(), "s3".to_string())]);
        let shelves = vec![
            rung("r", "f1", None, None, &[]),
            rung("fic", "f1", Some("Fiction"), Some("r"), &[]),
            rung("s3", "f3", None, None, &["deep"]),
            own("mine", "Mine", None, &[]),
        ];
        (shelves, vec![tree, member])
    }

    #[test]
    fn a_displaced_folder_s_root_shelf_goes_home_by_the_fold() {
        let (shelves, folders) = family_state();
        match return_path(&shelves, &folders, "s3") {
            Some(ReturnPath::Reclaim { tree, gone, rel, .. }) => {
                assert_eq!(tree, "f1", "the family the ground belongs to");
                assert_eq!(gone, "f3", "the folder that was reading it on its own");
                assert_eq!(rel, "Fiction/SciFi", "the rung its directory names");
            }
            _ => panic!("the fold is a displaced root shelf's way home"),
        }
        // And the drop that offers it is a drop inside the family: f1's rungs
        // cover the ground s3 stands on.
        assert!(target_is_family(&shelves, &folders, Some("fic"), "/books/Fiction/SciFi"));
        assert!(target_is_family(&shelves, &folders, Some("r"), "/books/Fiction/SciFi"));
        // A reader's own shelf, the root level and a shelf that is gone are
        // not family: the copy is the only answer they get.
        assert!(!target_is_family(&shelves, &folders, Some("mine"), "/books/Fiction/SciFi"));
        assert!(!target_is_family(&shelves, &folders, None, "/books/Fiction/SciFi"));
        assert!(!target_is_family(&shelves, &folders, Some("gone"), "/books/Fiction/SciFi"));
    }

    #[test]
    fn an_off_seat_rung_goes_home_by_the_reseat_and_a_seated_one_is_home() {
        let (shelves, folders) = family_state();
        // "fic" sits where f1's map names for it: home already, so the sheet
        // has no way home to offer — the copy or the cancel is the question.
        assert!(return_path(&shelves, &folders, "fic").is_none());
        // Off its seat — a hand of an older build carried it onto "mine" —
        // the way home is the reseat, and the seat is the ledger's own answer.
        let mut off = shelves.clone();
        off.iter_mut()
            .find(|s| s.id == "fic")
            .unwrap()
            .parent = Some("mine".to_string());
        match return_path(&off, &folders, "fic") {
            Some(ReturnPath::Reseat { seat, .. }) => assert_eq!(seat.as_deref(), Some("r")),
            _ => panic!("the reseat is an off-seat rung's way home"),
        }
        // The root rung dragged into a shelf is the same shape, with the
        // library's own level for its seat.
        let mut lifted = shelves.clone();
        lifted
            .iter_mut()
            .find(|s| s.id == "r")
            .unwrap()
            .parent = Some("mine".to_string());
        match return_path(&lifted, &folders, "r") {
            Some(ReturnPath::Reseat { seat, .. }) => assert_eq!(seat, None),
            _ => panic!("the root's seat is the library's own level"),
        }
    }
}
