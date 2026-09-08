-- 0003: shelves nest.
--
-- A shelf can be filed inside another shelf, which the frontend's persisted blob
-- carries as `Shelf::parent` — and a column the blob has and the catalog does not
-- is a value that survives a restart in one store and not the other. The two hold
-- the same library, so they carry the same shape of it.
--
-- No foreign key on `parent`, deliberately, and for the reason `folder_id` has
-- none: shelves are written one row at a time in whatever order the caller has
-- them, so a self-reference would turn restoring a library into an ordering
-- problem. A parent that names no shelf is `library_core::shelf::sanitize`'s to
-- collapse back to the root, and `shelf_delete` lifts the children of a shelf it
-- removes, so the only delete this schema has cannot produce one.

ALTER TABLE shelves ADD COLUMN parent TEXT;
CREATE INDEX idx_shelves_parent ON shelves (parent);
