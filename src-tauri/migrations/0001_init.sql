-- 0001: the library catalog.
--
-- Three principles decided this shape, and they are the reason half of what used
-- to be one localStorage blob is here and half is not:
--
--   * content is relational and unbounded — books, their resume points, their
--     highlights, their covers, the shelves and the folder ledgers — so it lives
--     where a delete cascades and a query can be indexed;
--   * chrome preference is small, per-device and needed synchronously at first
--     paint, so it stays in localStorage (`reader_core::settings`);
--   * the bytes of a copied book are NOT here. `pdf-engine` opens by path through
--     the asset protocol, which streams; a BLOB would put every whole file through
--     IPC into wasm memory on every open. SQLite owns the address and the
--     fingerprint, the filesystem owns the bytes, and a purge is a `remove_file`
--     rather than a `VACUUM`.
--
-- The fingerprint triple is a UNIQUE constraint, which is the point: the import
-- ledger's "have I seen this content before?" was an application-side hash set
-- that every writer had to remember to consult. Here it is a constraint, so a
-- second copy of one file cannot be inserted by any code path, including one
-- written later by somebody who did not read the ledger.

CREATE TABLE kv (
  key   TEXT NOT NULL PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE books (
  id            TEXT NOT NULL PRIMARY KEY,
  -- The fingerprint: what survives a move, and what makes two addresses holding
  -- one file one book.
  fp_size       INTEGER NOT NULL,
  fp_mtime_ms   INTEGER NOT NULL,
  fp_head_hash  INTEGER NOT NULL,
  -- The fingerprint is a placeholder derived from the address rather than a
  -- measurement of the file: a row migrated from the previous schema, or a book
  -- opened before its first path check. A watched folder must not rescan against
  -- one — a real fingerprint matches no placeholder, so every book the folder
  -- already holds would be added again.
  fp_pending    INTEGER NOT NULL DEFAULT 1,
  title         TEXT,
  author        TEXT,
  -- The file stem, kept beside the title because it is what a book with no
  -- title of its own is searchable by. Indexed by books_fts.
  stem          TEXT NOT NULL DEFAULT '',
  format        TEXT NOT NULL,           -- 'pdf' | 'text' | 'markdown'
  origin        TEXT NOT NULL,           -- 'linked' | 'stored'
  -- THE address: the reader's own path when linked, the app's store copy when
  -- stored. Everything that opens a book reads this column and nothing else.
  path          TEXT NOT NULL,
  src_path      TEXT,                    -- provenance, when the app copied it
  position      INTEGER NOT NULL,        -- manual order in the All view
  added_ms      INTEGER NOT NULL,
  last_read_ms  INTEGER NOT NULL DEFAULT 0,
  last_page     INTEGER NOT NULL DEFAULT 1,
  num_pages     INTEGER NOT NULL DEFAULT 0,
  fraction      REAL,
  missing       INTEGER NOT NULL DEFAULT 0,
  UNIQUE (fp_size, fp_mtime_ms, fp_head_hash)
);
CREATE INDEX idx_books_path ON books (path);
CREATE INDEX idx_books_position ON books (position);

CREATE TABLE shelves (
  id        TEXT NOT NULL PRIMARY KEY,
  name      TEXT NOT NULL,
  kind      TEXT NOT NULL,               -- 'virtual' | 'folder'
  folder_id TEXT,
  rel       TEXT,                        -- subfolder within the folder's root
  position  INTEGER NOT NULL
);
CREATE INDEX idx_shelves_position ON shelves (position);

-- Membership and nothing else: a shelf holds ids, never a path and never a byte,
-- which is what makes filing a read-in-place book unable to touch a file.
CREATE TABLE shelf_books (
  shelf_id TEXT NOT NULL REFERENCES shelves (id) ON DELETE CASCADE,
  book_id  TEXT NOT NULL REFERENCES books (id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  PRIMARY KEY (shelf_id, book_id)
);
CREATE INDEX idx_shelf_books_book ON shelf_books (book_id);

CREATE TABLE folders (
  id             TEXT NOT NULL PRIMARY KEY,
  root           TEXT NOT NULL UNIQUE,
  -- The ledger is one aggregate that is always read and written whole with its
  -- folder, and nothing ever queries across folders, so it is JSON in a column
  -- rather than four more tables. That is a decision about these four values, not
  -- a licence: anything a query needs to see gets a column.
  opts_json      TEXT NOT NULL,          -- FolderOpts
  placed_json    TEXT NOT NULL,          -- [Fingerprint] already filed once
  ignored_json   TEXT NOT NULL,          -- [Tombstone] removed, restorable
  last_seen_json TEXT NOT NULL,          -- [(Fingerprint, path)] latest scan
  shelf_map_json TEXT NOT NULL,          -- { rel: shelf_id }
  scanned_ms     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE gloss_marks (
  id          TEXT NOT NULL PRIMARY KEY,
  book_id     TEXT NOT NULL REFERENCES books (id) ON DELETE CASCADE,
  page        INTEGER NOT NULL,
  word        TEXT NOT NULL,
  context     TEXT NOT NULL,
  anchor_json TEXT NOT NULL,             -- the mark's page-space rect
  created_ms  INTEGER NOT NULL
);
CREATE INDEX idx_gloss_book ON gloss_marks (book_id);

-- The AI's answer, cached against the mark that asked. Persisting it is what makes
-- reopening a highlighted word instant and offline, instead of a second request
-- for a sentence the model has already written.
CREATE TABLE gloss_answers (
  mark_id   TEXT NOT NULL PRIMARY KEY REFERENCES gloss_marks (id) ON DELETE CASCADE,
  info_json TEXT NOT NULL,               -- WordInfo
  saved_ms  INTEGER NOT NULL
);

CREATE TABLE covers (
  book_id  TEXT NOT NULL PRIMARY KEY REFERENCES books (id) ON DELETE CASCADE,
  width    REAL NOT NULL,
  height   REAL NOT NULL,
  -- A data URL, exactly as the engine renders it: migrating these is a text
  -- insert with no transcode, which is the difference between a migration that
  -- takes milliseconds and one that takes a minute.
  data_url TEXT NOT NULL
);

-- Library search, server-side. External content over the catalog, so the index
-- holds no copy of the text it searches and cannot drift from it — the triggers
-- below are the only writers.
CREATE VIRTUAL TABLE books_fts USING fts5(
  title,
  author,
  stem,
  content='books',
  content_rowid='rowid',
  tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER books_fts_i AFTER INSERT ON books BEGIN
  INSERT INTO books_fts (rowid, title, author, stem)
  VALUES (new.rowid, new.title, new.author, new.stem);
END;

CREATE TRIGGER books_fts_d AFTER DELETE ON books BEGIN
  INSERT INTO books_fts (books_fts, rowid, title, author, stem)
  VALUES ('delete', old.rowid, old.title, old.author, old.stem);
END;

CREATE TRIGGER books_fts_u AFTER UPDATE OF title, author, stem ON books BEGIN
  INSERT INTO books_fts (books_fts, rowid, title, author, stem)
  VALUES ('delete', old.rowid, old.title, old.author, old.stem);
  INSERT INTO books_fts (rowid, title, author, stem)
  VALUES (new.rowid, new.title, new.author, new.stem);
END;
