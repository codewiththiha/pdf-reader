//! The library's database: one connection, its pragmas, and the migration runner.
//!
//! SQLite is the catalog's home because the catalog is relational and unbounded —
//! books, resume points, highlights, covers, shelves and folder ledgers all
//! cross-reference each other, and localStorage has a quota measured in
//! megabytes that a cover map alone will find. What is NOT here is deliberate:
//!
//!   * **chrome preference stays in localStorage.** Settings paint the theme
//!     before any IPC round trip resolves, so making them async would buy a white
//!     flash and a race in exchange for nothing.
//!   * **the bytes of a copied book stay on the filesystem.** `pdf-engine` opens by
//!     path through the asset protocol, which streams; a BLOB would put every whole
//!     file through IPC into wasm memory on every open. SQLite owns the address and
//!     the fingerprint, the filesystem owns the bytes, and a purge is a
//!     `remove_file` rather than a `VACUUM`.
//!   * **transient state stays in signals.** An in-flight import, an open overlay
//!     and the text in the search box are all things a restart should forget.
//!
//! ## Concurrency
//!
//! WAL plus one `Mutex<Connection>` is the whole story. Readers never block the
//! import writer, and every command reaches the connection through
//! [`Db::with_conn`] inside a `spawn_blocking`, so the async runtime never holds
//! the lock on-thread. A mutex around one connection rather than a pool because
//! the work is short and serial: a library write is a handful of rows, and a pool
//! would add a second answer to "which connection saw which write".
//!
//! ## Failure
//!
//! [`open`] does not fail. A database that will not open — corrupt, locked by
//! another process, on a full disk — yields a [`Db`] holding nothing, every
//! command answers with [`UNAVAILABLE`], and the reader still reads: settings come
//! from localStorage and a document opens by path, neither of which asks this
//! module anything. The alternative is an app that will not start because its
//! library is unreadable, which trades a missing shelf for a missing reader.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use tauri::Manager;

pub mod repo;

/// What every command answers when there is no database behind it. One string, so
/// the frontend has one thing to match on and the reader sees one sentence.
pub const UNAVAILABLE: &str = "The library database is unavailable.";

/// The schema, in order. Each entry is one transaction and one `user_version`
/// bump, so a migration is either wholly applied or wholly not — there is no
/// half-migrated database for a later run to trip over.
///
/// `include_str!` rather than reading the files at runtime: the SQL travels inside
/// the binary, so an installed app cannot be handed a schema by whoever can write
/// to its data directory.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../migrations/0001_init.sql")),
    (2, include_str!("../../migrations/0002_gloss_fts.sql")),
];

/// The library's connection, or nothing when it could not be opened.
///
/// Cheap to clone (an `Arc`), so a command takes one and hands it to the blocking
/// pool without negotiating ownership.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Option<Connection>>>,
}

impl Db {
    /// A handle over an already-open connection. Test-shaped on purpose: the host
    /// tests build one over `Connection::open_in_memory` and run the real
    /// migrations against it, so the schema in `migrations/` is the schema under
    /// test rather than a file nobody executes.
    pub fn from_connection(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(Some(conn))),
        }
    }

    /// A handle with no database behind it. What [`open`] falls back to.
    pub fn unavailable() -> Self {
        Self {
            conn: Arc::new(Mutex::new(None)),
        }
    }

    /// Whether there is a database to talk to. What the frontend asks before it
    /// decides between "empty library" and "library unavailable".
    pub fn is_available(&self) -> bool {
        self.conn
            .lock()
            .is_ok_and(|guard| guard.is_some())
    }

    /// Run `run` against the connection, holding the lock for the duration.
    ///
    /// The lock is the serialization point: two imports cannot interleave writes,
    /// and a reader cannot see a transaction half-applied. Callers are expected to
    /// be on the blocking pool — see the module docs.
    pub fn with_conn<T>(&self, run: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        let guard = self
            .conn
            .lock()
            .map_err(|_| "the library database lock is poisoned".to_string())?;
        match guard.as_ref() {
            Some(conn) => run(conn),
            None => Err(UNAVAILABLE.to_string()),
        }
    }
}

/// Open (and migrate) the library database, or answer with an empty handle.
///
/// Never fails: see the module docs for why that is the right shape.
pub fn open(app: &tauri::AppHandle) -> Db {
    match connect(app) {
        Ok(db) => db,
        Err(message) => {
            // One console line rather than a toast: this runs before the webview
            // exists, so there is nothing on screen to say it to, and every command
            // that follows will report it to something that can.
            report(&message);
            Db::unavailable()
        }
    }
}

fn connect(app: &tauri::AppHandle) -> Result<Db, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("no app data directory: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {dir:?}: {e}"))?;
    let path = dir.join("library.db");
    let conn = Connection::open(&path).map_err(|e| format!("could not open {path:?}: {e}"))?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(Db::from_connection(conn))
}

/// The pragmas every connection needs. Split from [`connect`] so the host tests
/// can apply the same ones to an in-memory database and be sure they are testing
/// the configuration the app runs with — `foreign_keys` in particular is
/// per-connection, and a cascade that only works in production is not a cascade.
pub fn configure(conn: &Connection) -> Result<(), String> {
    // INCREMENTAL auto-vacuum only takes effect on a database with no pages yet,
    // so it is set exactly once, before the first migration creates a table. A
    // purge can then hand the space back lazily instead of the file only ever
    // growing.
    let fresh: i64 = user_version(conn)?;
    if fresh == 0 {
        conn.pragma_update(None, "auto_vacuum", 2)
            .map_err(|e| format!("auto_vacuum: {e}"))?;
    }
    // WAL is what lets a reader browse the shelf while an import writes to it.
    // On an in-memory database it is a no-op that answers "memory", not an error.
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("journal_mode: {e}"))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| format!("synchronous: {e}"))?;
    // The cascades the purge relies on. Off by default in SQLite, so this line is
    // the difference between "DELETE FROM books" taking a book's highlights,
    // answers, cover and shelf links with it and leaving all four orphaned.
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| format!("foreign_keys: {e}"))?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| format!("busy_timeout: {e}"))?;
    Ok(())
}

/// Apply every migration above the database's own version.
///
/// Idempotent by construction, and the idempotence is tested rather than assumed:
/// running it twice on one connection must change nothing, because the second run
/// happens every time the app starts.
pub fn migrate(conn: &Connection) -> Result<(), String> {
    let version = user_version(conn)?;
    for (next, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > version) {
        // `unchecked_transaction` because the runner only ever has a
        // `&Connection`: the whole point is that a migration is one atomic step,
        // and a step that cannot be rolled back is a step that can be half-done.
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("could not begin migration {next}: {e}"))?;
        tx.execute_batch(sql)
            .map_err(|e| format!("migration {next} failed: {e}"))?;
        tx.pragma_update(None, "user_version", next)
            .map_err(|e| format!("migration {next} could not record its version: {e}"))?;
        tx.commit()
            .map_err(|e| format!("migration {next} could not commit: {e}"))?;
    }
    Ok(())
}

/// The schema version this database is at.
pub fn user_version(conn: &Connection) -> Result<i64, String> {
    conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|e| format!("could not read user_version: {e}"))
}

/// The version the newest migration would bring a database to. What a test
/// compares `user_version` against, so a migration added without its version
/// being reachable fails here rather than in somebody's data directory.
pub fn schema_version() -> i64 {
    MIGRATIONS.last().map(|(v, _)| *v).unwrap_or(0)
}

/// One console line, because the shell crate has no logger and the one thing a
/// boot failure must not do is panic while the app is starting.
fn report(message: &str) {
    eprintln!("[library] database unavailable: {message}");
}

#[cfg(test)]
mod tests {
    use super::{configure, migrate, schema_version, user_version, Db, MIGRATIONS, UNAVAILABLE};
    use rusqlite::Connection;

    /// An in-memory database running the real migrations, configured the way the
    /// app configures itself.
    fn db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        configure(&conn).expect("pragmas");
        conn
    }

    #[test]
    fn migrations_run_once_and_then_do_nothing() {
        let conn = db();
        assert_eq!(user_version(&conn).unwrap(), 0);
        migrate(&conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), schema_version());
        // The second run is every subsequent launch. A runner that re-applied
        // would fail on CREATE TABLE, and a runner that silently skipped would
        // leave the version lying.
        migrate(&conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), schema_version());
    }

    #[test]
    fn every_migration_is_numbered_and_in_order() {
        // A gap or a repeat in this list is a database that can never reach its
        // own schema, and the failure would land in somebody's data directory
        // rather than in a test.
        let mut previous = 0i64;
        for (version, sql) in MIGRATIONS {
            assert!(*version > previous, "migration {version} does not advance");
            assert!(!sql.trim().is_empty(), "migration {version} is empty");
            previous = *version;
        }
        assert_eq!(previous, schema_version());
    }

    #[test]
    fn the_schema_the_migrations_build_is_the_one_queried_below() {
        let conn = db();
        migrate(&conn).unwrap();
        for table in [
            "kv",
            "books",
            "shelves",
            "shelf_books",
            "folders",
            "gloss_marks",
            "gloss_answers",
            "covers",
            "books_fts",
            "gloss_fts",
        ] {
            let found: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type IN ('table','view') AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(found, 1, "{table} is missing from the migrated schema");
        }
    }

    #[test]
    fn fts5_is_compiled_into_the_bundled_sqlite() {
        // The whole search design rests on this. `bundled` builds SQLite from
        // source with its own flag list, and a flag that stops being set is a
        // migration that fails on every machine at once.
        let conn = db();
        migrate(&conn).unwrap();
        conn.execute_batch("INSERT INTO books (id, fp_size, fp_mtime_ms, fp_head_hash, stem, format, origin, path, position, added_ms) VALUES ('b1', 1, 1, 1, 'dune', 'pdf', 'linked', '/books/dune.pdf', 0, 0);")
            .unwrap();
        let hits: i64 = conn
            .query_row(
                "SELECT count(*) FROM books_fts WHERE books_fts MATCH '\"dune\"*'",
                [],
                |row| row.get(0),
            )
            .expect("fts5 must be available");
        assert_eq!(hits, 1);
    }

    #[test]
    fn the_pragmas_the_cascades_depend_on_are_set() {
        let conn = db();
        migrate(&conn).unwrap();
        let foreign_keys: i64 = conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1, "a cascade with foreign_keys off is a no-op");
        let vacuum: i64 = conn
            .pragma_query_value(None, "auto_vacuum", |row| row.get(0))
            .unwrap();
        assert_eq!(vacuum, 2, "incremental, so a purge can hand the space back");
    }

    #[test]
    fn a_database_that_was_never_opened_says_so_instead_of_panicking() {
        let db = Db::unavailable();
        assert!(!db.is_available());
        let answer = db.with_conn(|conn| {
            let _: &Connection = conn;
            Ok(())
        });
        assert_eq!(answer.unwrap_err(), UNAVAILABLE);
    }

    #[test]
    fn a_handle_over_a_real_connection_reports_itself() {
        let conn = db();
        migrate(&conn).unwrap();
        let handle = Db::from_connection(conn);
        assert!(handle.is_available());
        let version = handle
            .with_conn(|conn| user_version(conn))
            .expect("reachable");
        assert_eq!(version, schema_version());
    }
}
