-- 0002: search the highlights, not just the catalog.
--
-- The second migration exists to prove the runner before anything depends on it
-- being right: a migration list of one is indistinguishable from a script that
-- runs once, and the first real schema change is not the moment to find out the
-- `user_version` bump was never committed.
--
-- "Find the book by what I highlighted in it" is the question this answers, and
-- it is the one a catalog index cannot: the word is in the mark's context, not in
-- any column of `books`.

CREATE VIRTUAL TABLE gloss_fts USING fts5(
  word,
  context,
  content='gloss_marks',
  content_rowid='rowid',
  tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER gloss_fts_i AFTER INSERT ON gloss_marks BEGIN
  INSERT INTO gloss_fts (rowid, word, context)
  VALUES (new.rowid, new.word, new.context);
END;

CREATE TRIGGER gloss_fts_d AFTER DELETE ON gloss_marks BEGIN
  INSERT INTO gloss_fts (gloss_fts, rowid, word, context)
  VALUES ('delete', old.rowid, old.word, old.context);
END;

CREATE TRIGGER gloss_fts_u AFTER UPDATE OF word, context ON gloss_marks BEGIN
  INSERT INTO gloss_fts (gloss_fts, rowid, word, context)
  VALUES ('delete', old.rowid, old.word, old.context);
  INSERT INTO gloss_fts (rowid, word, context)
  VALUES (new.rowid, new.word, new.context);
END;
