//! SQLite databases (.sqlite, .sqlite3, .db), read only: each table is a sheet, its column
//! names the header row; views follow the tables when they can be read quickly. Nobody types SQL, and the file is never
//! written:
//! - opened `immutable`: SQLite takes no lock and writes nothing beside the file, not even a journal;
//! - unless a `-wal` or `-journal` file sits beside it (the program that owns it is running, or stopped halfway): then
//!   its last changes live partly in that file, so the database and that file are copied to a temporary folder and the
//!   copy is opened instead, and removed afterwards;
//! - triggers off, the schema not trusted (functions in views and defaults can't do anything a plain read can't),
//!   defensive mode on, extension loading never switched on;
//! - a read that runs too long (a view over a huge join, a recursive one) is interrupted and that view left out.
//!
//! A database can be far larger than a window holds, so what is read is capped (`Limits`): for Sheets (`VIEW`) 100,000
//! rows of a table at most and 2,000,000 cells across the file, each table getting at least 1,000 rows; for the
//! converter (`CONVERT`) as many rows as an XLSX sheet holds and 5,000,000 cells, and a database that doesn't fit is
//! refused as too large rather than converted in part. A cell holds at most Excel's 32,767 characters. A sheet that was
//! cut says so in the model (`cut`: the rows it has in the file). A blob that isn't text shows as the start of its bytes
//! in hex (0x89504E47…).

use crate::dbf::capped;
use rusqlite::config::DbConfig;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// How much of a database is read: at most `rows` of a table, `cells` across the file, each table at least `min_rows`.
#[derive(Clone, Copy)]
pub struct Limits {
    pub rows: usize,
    pub cells: usize,
    pub min_rows: usize,
}

/// What Sheets shows.
pub const VIEW: Limits = Limits { rows: 100_000, cells: 2_000_000, min_rows: 1_000 };
/// What the converter writes: an XLSX sheet's rows, below its header row.
pub const CONVERT: Limits = Limits { rows: 1_048_575, cells: 5_000_000, min_rows: 1_000 };
/// How long a table may take to read, and a view (views are a convenience: one that is slow is left out).
const TABLE_SECONDS: u64 = 60;
const VIEW_SECONDS: u64 = 5;
const BLOB_BYTES: usize = 32;

/// Whether these bytes start a SQLite database. An encrypted one (SQLCipher, SEE) doesn't: it opens as damaged.
pub fn looks(head: &[u8]) -> bool {
    head.starts_with(b"SQLite format 3\0")
}

/// The database as a URI SQLite opens without locking or writing anything (`immutable`).
fn immutable_uri(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    // Windows' extended paths: \\?\C:\… and \\?\UNC\server\share\…
    let s = match (s.strip_prefix("//?/UNC/"), s.strip_prefix("//?/")) {
        (Some(unc), _) => format!("//{unc}"),
        (None, Some(local)) => local.to_string(),
        _ => s,
    };
    // C:/x → file:///C:/x; /x → file:///x; //server/share/x → file:////server/share/x
    let mut uri = String::from(if s.starts_with('/') { "file://" } else { "file:///" });
    for c in s.chars() {
        match c {
            '%' => uri.push_str("%25"),
            '?' => uri.push_str("%3F"),
            '#' => uri.push_str("%23"),
            c => uri.push(c),
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    uri
}

fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(suffix);
    PathBuf::from(p)
}

fn nonempty(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0)
}

/// A temporary folder, removed when this is dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The database to read: the file itself, or a copy with its journal when it has one (see the top of this file).
fn open(path: &Path) -> Result<(Connection, Option<Scratch>), String> {
    let wal = beside(path, "-wal");
    let journal = beside(path, "-journal");
    if nonempty(&wal) || nonempty(&journal) {
        let stamp =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("dotany-sqlite-{}-{stamp}", std::process::id()));
        let scratch = Scratch(dir.clone());
        let copied = std::fs::create_dir_all(&dir).is_ok()
            && std::fs::copy(path, dir.join("db")).is_ok()
            && (!nonempty(&wal) || std::fs::copy(&wal, dir.join("db-wal")).is_ok())
            && (!nonempty(&journal) || std::fs::copy(&journal, dir.join("db-journal")).is_ok());
        if copied {
            // the copy is private: SQLite may roll its journal back or read its WAL in, as the program that owns the file would
            let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
            if let Ok(c) = Connection::open_with_flags(dir.join("db"), flags) {
                return Ok((c, Some(scratch)));
            }
        }
        // no room for a copy: the file as it is, without what its journal holds
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    Connection::open_with_flags(immutable_uri(path), flags).map(|c| (c, None)).map_err(|e| e.to_string())
}

fn harden(c: &Connection) -> Result<(), String> {
    let e = |e: rusqlite::Error| e.to_string();
    c.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false).map_err(e)?;
    c.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false).map_err(e)?;
    c.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true).map_err(e)?;
    c.pragma_update(None, "query_only", true).map_err(e)?;
    Ok(())
}

/// `f`, interrupted if it takes longer than `secs`.
fn within<T>(c: &Connection, secs: u64, f: impl FnOnce() -> T) -> T {
    let handle = c.get_interrupt_handle();
    let (done, wait) = mpsc::channel::<()>();
    let watch = std::thread::spawn(move || {
        if let Err(mpsc::RecvTimeoutError::Timeout) = wait.recv_timeout(Duration::from_secs(secs)) {
            handle.interrupt();
        }
    });
    let out = f();
    drop(done);
    let _ = watch.join();
    out
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn cell(v: ValueRef) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        // beyond 2^53 a number would lose digits: such an id is shown as the text it is
        ValueRef::Integer(i) if i.unsigned_abs() <= 1 << 53 => json!({ "n": i }),
        ValueRef::Integer(i) => json!(i.to_string()),
        ValueRef::Real(f) if f.is_finite() => json!({ "n": f }),
        ValueRef::Real(f) => json!(f.to_string()),
        ValueRef::Text(t) => json!(capped(String::from_utf8_lossy(t).into_owned())),
        ValueRef::Blob(b) => match std::str::from_utf8(b) {
            Ok(s) if !s.chars().any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r')) => {
                json!(capped(s.to_string()))
            }
            _ => {
                let hex: String = b.iter().take(BLOB_BYTES).map(|x| format!("{x:02X}")).collect();
                json!(format!("0x{hex}{}", if b.len() > BLOB_BYTES { "…" } else { "" }))
            }
        },
    }
}

/// A name a sheet can have (and keep in an XLSX): none of []:*?/\, at most 31 characters, none twice in any case.
fn sheet_name(name: &str, used: &mut HashSet<String>) -> String {
    let clean: String = name.chars().map(|c| if "[]:*?/\\".contains(c) || c.is_control() { ' ' } else { c }).collect();
    let base: String = clean.trim().trim_matches('\'').chars().take(31).collect::<String>().trim().to_string();
    let base = if base.is_empty() { "Sheet".to_string() } else { base };
    let (mut s, mut k) = (base.clone(), 1);
    while used.contains(&s.to_lowercase()) {
        k += 1;
        let tail = format!(" ({k})");
        s = format!("{}{tail}", base.chars().take(31 - tail.chars().count()).collect::<String>());
    }
    used.insert(s.to_lowercase());
    s
}

struct Object {
    name: String,
    view: bool,
}

/// The tables, then the views, in the order they were made. SQLite's own tables and the tables a full-text or R-tree
/// index keeps its workings in are left out: they are the index's, not the data's.
fn objects(c: &Connection) -> Result<Vec<Object>, String> {
    let e = |e: rusqlite::Error| e.to_string();
    let mut st = c
        .prepare(
            "SELECT type, name, coalesce(sql, '') FROM sqlite_schema WHERE type IN ('table', 'view') ORDER BY rowid",
        )
        .map_err(e)?;
    let all: Vec<(String, String, String)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
    let virtuals: Vec<&str> = all
        .iter()
        .filter(|(_, _, sql)| {
            sql.trim_start().get(..20).is_some_and(|h| h.eq_ignore_ascii_case("create virtual table"))
        })
        .map(|(_, n, _)| n.as_str())
        .collect();
    const SHADOW: [&str; 12] =
        ["content", "segments", "segdir", "docsize", "stat", "data", "idx", "config", "node", "parent", "rowid", "aux"];
    let shadow = |n: &str| {
        virtuals.iter().any(|v| {
            n.get(..v.len()).is_some_and(|head| head.eq_ignore_ascii_case(v))
                && n.as_bytes().get(v.len()) == Some(&b'_')
                && n.get(v.len() + 1..).is_some_and(|tail| SHADOW.iter().any(|s| tail.eq_ignore_ascii_case(s)))
        })
    };
    let mut out: Vec<Object> = Vec::new();
    for view in [false, true] {
        for (kind, name, _) in &all {
            if (kind == "view") == view && !name.to_ascii_lowercase().starts_with("sqlite_") && !shadow(name) {
                out.push(Object { name: name.clone(), view });
            }
        }
    }
    Ok(out)
}

/// One table or view: its column names, then up to `limit` rows; and how many rows it has, when it has more.
fn rows_of(c: &Connection, o: &Object, limit: usize) -> Result<(Vec<Vec<Value>>, Option<u64>), rusqlite::Error> {
    let mut st = c.prepare(&format!("SELECT * FROM {} LIMIT {}", quoted(&o.name), limit + 1))?;
    let names: Vec<Value> = st.column_names().into_iter().map(|n| json!(n)).collect();
    let cols = names.len();
    let mut rows = vec![names];
    let mut q = st.query([])?;
    let mut more = false;
    while let Some(r) = q.next()? {
        if rows.len() > limit {
            more = true;
            break;
        }
        let mut row: Vec<Value> = (0..cols).map(|i| r.get_ref(i).map(cell).unwrap_or(Value::Null)).collect();
        while row.last().is_some_and(Value::is_null) {
            row.pop();
        }
        rows.push(row);
    }
    drop(q);
    // the whole count only when it was cut: a table's is quick, a view's might not be
    let all = if more && !o.view {
        c.query_row(&format!("SELECT count(*) FROM {}", quoted(&o.name)), [], |r| r.get::<_, i64>(0))
            .ok()
            .map(|n| n as u64)
    } else {
        None
    };
    Ok((rows, if more { Some(all.unwrap_or(0)) } else { None }))
}

/// The database at `path` as the page's cells: a sheet for each table (and view), each with its header row.
pub fn workbook(path: &Path, limits: Limits) -> Result<Value, String> {
    // a damaged file must never take the window down with it
    std::panic::catch_unwind(|| read(path, limits)).unwrap_or_else(|_| Err("panicked".into()))
}

fn read(path: &Path, limits: Limits) -> Result<Value, String> {
    let mut head = [0u8; 16];
    use std::io::Read;
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).map_err(|e| e.to_string())?;
    if !looks(&head) {
        return Err("not a SQLite database".into());
    }
    // declared before the connection, so the copy's folder goes after it has closed, on every way out of here
    let opened = open(path)?;
    let scratch = opened.1;
    let c = opened.0;
    harden(&c)?;
    // a damaged or encrypted file fails here, on its first page
    let list = within(&c, TABLE_SECONDS, || objects(&c))?;
    let mut used = HashSet::new();
    let mut sheets = Vec::new();
    let mut budget = limits.cells;
    for o in &list {
        let cols =
            c.prepare(&format!("SELECT * FROM {}", quoted(&o.name))).map(|s| s.column_count()).unwrap_or(0).max(1);
        let limit = (budget / cols).clamp(limits.min_rows, limits.rows);
        let got = within(&c, if o.view { VIEW_SECONDS } else { TABLE_SECONDS }, || rows_of(&c, o, limit));
        let (rows, cut) = match got {
            Ok(x) => x,
            // a view that is slow, or names a function or a table that isn't there, is left out
            Err(_) if o.view => continue,
            // a virtual table whose module this build doesn't have (an add-on's), likewise
            Err(rusqlite::Error::SqliteFailure(_, Some(m))) if m.starts_with("no such module") => continue,
            Err(e) => return Err(e.to_string()),
        };
        budget = budget.saturating_sub((rows.len() - 1) * cols);
        let mut sheet = json!({ "name": sheet_name(&o.name, &mut used), "rows": rows, "header": true });
        if let Some(all) = cut {
            sheet["cut"] = json!(all);
        }
        sheets.push(sheet);
    }
    drop(c);
    drop(scratch);
    Ok(json!({ "sheets": sheets, "names": [] }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "dotany-table-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn make(path: &Path, sql: &str) {
        let _ = std::fs::remove_file(path);
        let c = Connection::open(path).unwrap();
        c.execute_batch(sql).unwrap();
    }

    #[test]
    fn uris_for_every_kind_of_path() {
        assert_eq!(immutable_uri(Path::new(r"C:\Data\a b#1.db")), "file:///C:/Data/a b%231.db?mode=ro&immutable=1");
        assert_eq!(
            immutable_uri(Path::new(r"\\server\share\x?.db")),
            "file:////server/share/x%3F.db?mode=ro&immutable=1"
        );
        assert_eq!(immutable_uri(Path::new(r"\\?\C:\x.db")), "file:///C:/x.db?mode=ro&immutable=1");
        assert_eq!(immutable_uri(Path::new("/home/me/100%.db")), "file:///home/me/100%25.db?mode=ro&immutable=1");
    }

    #[test]
    fn names_fit_a_workbook() {
        let mut used = HashSet::new();
        assert_eq!(sheet_name("orders", &mut used), "orders");
        assert_eq!(sheet_name("Orders", &mut used), "Orders (2)");
        assert_eq!(sheet_name("a/b:c", &mut used), "a b c");
        assert_eq!(sheet_name(&"x".repeat(40), &mut used).chars().count(), 31);
        assert_eq!(sheet_name(&"x".repeat(40), &mut used), format!("{} (2)", "x".repeat(27)));
    }

    #[test]
    fn tables_and_views_as_sheets_and_the_file_untouched() {
        let d = dir();
        let p = d.join("shop.sqlite");
        make(&p, "CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT, price REAL, photo BLOB, note);
                  INSERT INTO items VALUES (1, 'Kettle', 12.5, x'89504E470D0A1A0A0000000D49484452000000100000001008060000001FF3FF61', NULL),
                                           (2, '北京', 3, CAST('plain' AS BLOB), 9007199254740993);
                  CREATE VIEW cheap AS SELECT name FROM items WHERE price < 10;
                  CREATE TABLE log(t);
                  CREATE TRIGGER no_read AFTER INSERT ON log BEGIN SELECT 1; END;
                  CREATE VIRTUAL TABLE docs USING fts5(body);
                  INSERT INTO docs VALUES ('hello world');");
        let before = std::fs::read(&p).unwrap();
        let modified = std::fs::metadata(&p).unwrap().modified().unwrap();
        let m = workbook(&p, VIEW).unwrap();
        let names: Vec<&str> = m["sheets"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["items", "log", "docs", "cheap"]);
        let items = &m["sheets"][0];
        assert_eq!(items["header"], true);
        assert_eq!(items["rows"][0], json!(["id", "name", "price", "photo", "note"]));
        assert_eq!(items["rows"][1][1], "Kettle");
        assert_eq!(items["rows"][1][2]["n"], 12.5);
        assert_eq!(items["rows"][1][3], "0x89504E470D0A1A0A0000000D49484452000000100000001008060000001FF3FF…");
        assert_eq!(items["rows"][2], json!([{ "n": 2 }, "北京", { "n": 3.0 }, "plain", "9007199254740993"]));
        assert_eq!(m["sheets"][2]["rows"][1][0], "hello world");
        assert_eq!(m["sheets"][3]["rows"], json!([["name"], ["北京"]]));
        // nothing written: the same bytes, the same time, and nothing beside it
        assert_eq!(std::fs::read(&p).unwrap(), before);
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), modified);
        let beside: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("shop.sqlite"))
            .collect();
        assert_eq!(beside, ["shop.sqlite"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn large_tables_are_cut_and_say_so() {
        let d = dir();
        let p = d.join("big.db");
        make(&p, "CREATE TABLE n(i, j); WITH RECURSIVE k(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM k WHERE x < 120000) INSERT INTO n SELECT x, x * 2 FROM k;
                  CREATE TABLE small(a); INSERT INTO small VALUES (1), (2);
                  CREATE VIEW forever AS WITH RECURSIVE k(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM k) SELECT count(*) FROM k;");
        let m = workbook(&p, VIEW).unwrap();
        let n = &m["sheets"][0];
        assert_eq!(n["rows"].as_array().unwrap().len(), VIEW.rows + 1);
        assert_eq!(n["cut"], 120000);
        assert!(m["sheets"][1].get("cut").is_none());
        // a view that never ends is interrupted and left out
        assert_eq!(m["sheets"].as_array().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_journal_beside_it_is_read_from_a_copy() {
        let d = dir();
        let p = d.join("live.db");
        let _ = std::fs::remove_file(beside(&p, "-wal"));
        let _ = std::fs::remove_file(&p);
        let owner = Connection::open(&p).unwrap();
        owner.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE t(a); INSERT INTO t VALUES ('in the wal');").unwrap();
        // the connection that wrote it is still open: the row is only in the -wal file
        assert!(nonempty(&beside(&p, "-wal")));
        let wal = std::fs::read(beside(&p, "-wal")).unwrap();
        let main = std::fs::read(&p).unwrap();
        let m = workbook(&p, VIEW).unwrap();
        assert_eq!(m["sheets"][0]["rows"][1][0], "in the wal");
        assert_eq!(std::fs::read(beside(&p, "-wal")).unwrap(), wal);
        assert_eq!(std::fs::read(&p).unwrap(), main);
        drop(owner);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sheets_sample_file() {
        // a database written by Python's sqlite3: its tables as sheets, and the file left as it was
        let Some(p) = crate::real_file("../../sheets/test/files/sqlite-shop.sqlite") else { return };
        let before = std::fs::read(&p).unwrap();
        let m = workbook(&p, VIEW).unwrap();
        let names: Vec<&str> = m["sheets"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["customers", "orders", "big_orders"]);
        assert_eq!(m["sheets"][0]["rows"][1], json!([{ "n": 1 }, "王小明", "北京", "2025-03-14"]));
        assert_eq!(m["sheets"][0]["rows"][4], json!([{ "n": 4 }, "Kim \"Jay\" Park", "Seoul"]));
        assert_eq!(m["sheets"][1]["rows"][1][5], "0x89504E470D0A1A0A0000000D494844520000000100000001");
        assert_eq!(m["sheets"][1]["rows"][3][5], "plain text kept as a blob");
        assert_eq!(
            m["sheets"][2]["rows"],
            json!([["id", "name", "amount"], [{ "n": 1 }, "王小明", { "n": 129.0 }], [{ "n": 3 }, "田中 花子", { "n": 899.0 }], [{ "n": 4 }, "王小明", { "n": 1599.99 }]])
        );
        assert_eq!(std::fs::read(&p).unwrap(), before);
    }

    #[test]
    fn damaged_and_encrypted_files_are_refused() {
        let d = dir();
        let p = d.join("bad.db");
        // SQLCipher's file starts with its salt, not SQLite's header
        std::fs::write(&p, [0x5Au8; 4096]).unwrap();
        assert!(workbook(&p, VIEW).is_err());
        // the header, then nonsense
        let mut b = b"SQLite format 3\0".to_vec();
        b.extend([0xFFu8; 4000]);
        std::fs::write(&p, &b).unwrap();
        assert!(workbook(&p, VIEW).is_err());
        // a good database cut short
        make(
            &p,
            "CREATE TABLE t(a); WITH RECURSIVE k(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM k WHERE x < 5000) INSERT INTO t SELECT hex(randomblob(40)) FROM k;",
        );
        let b = std::fs::read(&p).unwrap();
        std::fs::write(&p, &b[..b.len() / 3]).unwrap();
        // SQLite may read what is left or call it malformed; either way it comes back as a result, never a crash
        let _ = workbook(&p, VIEW);
        let _ = std::fs::remove_dir_all(&d);
    }
}
