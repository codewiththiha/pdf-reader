-- 0004: a hand-moved shelf keeps its move.
--
-- A shelf cut from a watched folder starts on the rung its directory has on
-- disk, and the frontend's rescan re-hangs it there — until the reader moves
-- it by hand. `library_core::shelf::reparent` accepts that move and marks the
-- row, and the re-hang passes a marked shelf by: the reader's arrangement
-- beats the filesystem's shape, while the shelf's `rel` keeps routing newly
-- scanned files into it wherever it now hangs.
--
-- A column the blob has and the catalog did not is a value that survives a
-- restart in one store and not the other — the same sentence migration 0003
-- was written under. Default 0: every shelf that predates the mark is where
-- the last scan put it, which is exactly what the default says.

ALTER TABLE shelves ADD COLUMN manual_parent INTEGER NOT NULL DEFAULT 0;
