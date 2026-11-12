//! SQLite storage for the library.

use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::model::{Analysis, AnalysisStatus, Asset, Folder, Tag};
use crate::{Error, Result};

/// Schema migrations, applied in order. `PRAGMA user_version` records how
/// many have run. Never edit an entry that has shipped: append a new one.
const MIGRATIONS: &[&str] = &[
    // 1: library
    "
    CREATE TABLE folders (
        id        INTEGER PRIMARY KEY,
        path      TEXT NOT NULL UNIQUE,
        added_at  INTEGER NOT NULL
    );
    CREATE TABLE assets (
        id             INTEGER PRIMARY KEY,
        folder_id      INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
        path           TEXT NOT NULL UNIQUE,
        name           TEXT NOT NULL,
        ext            TEXT NOT NULL,
        size           INTEGER NOT NULL,
        mtime          INTEGER NOT NULL,
        hash           TEXT,
        duration_ms    INTEGER,
        sample_rate    INTEGER,
        channels       INTEGER,
        loudness_lufs  REAL,
        gain_db        REAL NOT NULL DEFAULT 0,
        bpm            REAL,
        beat_offset_ms REAL,
        status         INTEGER NOT NULL DEFAULT 0,
        error          TEXT,
        missing        INTEGER NOT NULL DEFAULT 0,
        source         TEXT NOT NULL DEFAULT 'local',
        license        TEXT,
        attribution    TEXT,
        added_at       INTEGER NOT NULL
    );
    CREATE INDEX assets_folder ON assets(folder_id);
    CREATE INDEX assets_status ON assets(status);
    CREATE INDEX assets_hash   ON assets(hash);
    CREATE TABLE tags (
        id       INTEGER PRIMARY KEY,
        name     TEXT NOT NULL UNIQUE COLLATE NOCASE,
        category TEXT NOT NULL DEFAULT 'custom'
    );
    CREATE TABLE asset_tags (
        asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
        tag_id   INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
        PRIMARY KEY (asset_id, tag_id)
    );
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    ",
    // 2: everything the GM builds (scenes, boards, music sets, cue lists,
    // campaigns...) is stored as one JSON document per item.
    "
    CREATE TABLE docs (
        id         INTEGER PRIMARY KEY,
        kind       TEXT NOT NULL,
        data       TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE INDEX docs_kind ON docs(kind);
    ",
];

const ASSET_COLUMNS: &str = "id, folder_id, path, name, ext, size, mtime, hash, duration_ms, \
     sample_rate, channels, loudness_lufs, gain_db, bpm, beat_offset_ms, status, error, missing, \
     source, license, attribution";

/// A file found on disk during a folder scan.
#[derive(Debug, Clone)]
pub struct FoundFile {
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: i64,
    pub mtime: i64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScanDiff {
    pub added: usize,
    pub changed: usize,
    pub went_missing: usize,
    pub restored: usize,
}

impl ScanDiff {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

pub struct Db {
    conn: Connection,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn asset_from_row(row: &Row) -> rusqlite::Result<Asset> {
    Ok(Asset {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        path: row.get(2)?,
        name: row.get(3)?,
        ext: row.get(4)?,
        size: row.get(5)?,
        mtime: row.get(6)?,
        hash: row.get(7)?,
        duration_ms: row.get(8)?,
        sample_rate: row.get(9)?,
        channels: row.get(10)?,
        loudness_lufs: row.get(11)?,
        gain_db: row.get(12)?,
        bpm: row.get(13)?,
        beat_offset_ms: row.get(14)?,
        status: AnalysisStatus::from_i64(row.get(15)?),
        error: row.get(16)?,
        missing: row.get::<_, i64>(17)? != 0,
        source: row.get(18)?,
        license: row.get(19)?,
        attribution: row.get(20)?,
        tags: Vec::new(),
    })
}

impl Db {
    /// Opens the library, upgrading it if it was made by an older version.
    ///
    /// Before an upgrade the old database is copied next to the original
    /// (`library.backup-v1.db`), so a bad upgrade can always be undone.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 0 && (version as usize) < MIGRATIONS.len() {
            let backup = path.with_extension(format!("backup-v{version}.db"));
            if !backup.exists() {
                conn.execute("VACUUM INTO ?1", params![backup.to_string_lossy()])?;
            }
        }
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    // ---- folders -------------------------------------------------------

    pub fn add_folder(&self, path: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO folders (path, added_at) VALUES (?1, ?2)",
            params![path, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn remove_folder(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM folders WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.path,
                    (SELECT COUNT(*) FROM assets a WHERE a.folder_id = f.id AND a.missing = 0)
             FROM folders f ORDER BY f.path COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            let path: String = r.get(1)?;
            Ok(Folder {
                id: r.get(0)?,
                missing: !Path::new(&path).is_dir(),
                path,
                asset_count: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn folder_path(&self, id: i64) -> Result<String> {
        self.conn
            .query_row("SELECT path FROM folders WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("folder {id}")))
    }

    // ---- scanning ------------------------------------------------------

    /// Reconciles the files found on disk with the rows stored for a folder.
    ///
    /// New and modified files are queued for analysis (status pending). Rows
    /// whose file is gone are kept but marked missing, so tags survive a
    /// file being moved and found again.
    pub fn apply_scan(&mut self, folder_id: i64, found: &[FoundFile]) -> Result<ScanDiff> {
        struct Existing {
            id: i64,
            size: i64,
            mtime: i64,
            missing: bool,
        }
        let mut existing: HashMap<String, Existing> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT id, path, size, mtime, missing FROM assets WHERE folder_id = ?1")?;
            let rows = stmt.query_map(params![folder_id], |r| {
                Ok((
                    r.get::<_, String>(1)?,
                    Existing {
                        id: r.get(0)?,
                        size: r.get(2)?,
                        mtime: r.get(3)?,
                        missing: r.get::<_, i64>(4)? != 0,
                    },
                ))
            })?;
            for row in rows {
                let (path, e) = row?;
                existing.insert(path, e);
            }
        }

        let mut diff = ScanDiff::default();
        let added_at = now();
        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare(
                "INSERT OR IGNORE INTO assets (folder_id, path, name, ext, size, mtime, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            let mut changed = tx.prepare(
                "UPDATE assets SET size = ?2, mtime = ?3, status = 0, error = NULL, missing = 0
                 WHERE id = ?1",
            )?;
            let mut restore = tx.prepare("UPDATE assets SET missing = 0 WHERE id = ?1")?;
            let mut lost = tx.prepare("UPDATE assets SET missing = 1 WHERE id = ?1")?;

            for f in found {
                match existing.remove(&f.path) {
                    None => {
                        diff.added += insert.execute(params![
                            folder_id, f.path, f.name, f.ext, f.size, f.mtime, added_at
                        ])?;
                    }
                    Some(e) if e.size != f.size || e.mtime != f.mtime => {
                        changed.execute(params![e.id, f.size, f.mtime])?;
                        diff.changed += 1;
                    }
                    Some(e) if e.missing => {
                        restore.execute(params![e.id])?;
                        diff.restored += 1;
                    }
                    Some(_) => {}
                }
            }
            // Whatever is left was not seen on disk.
            for e in existing.values().filter(|e| !e.missing) {
                lost.execute(params![e.id])?;
                diff.went_missing += 1;
            }
        }
        tx.commit()?;
        Ok(diff)
    }

    /// Assets waiting for analysis: (id, path).
    pub fn pending(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path FROM assets WHERE status = 0 AND missing = 0 ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Stores an analysis result and returns the id the asset now lives
    /// under.
    ///
    /// If a missing asset has the same content hash, the file was moved or
    /// renamed: the old row takes over the new path (keeping its id and
    /// tags) and the freshly imported row is dropped.
    pub fn finish_analysis(&mut self, id: i64, a: &Analysis) -> Result<i64> {
        let tx = self.conn.transaction()?;
        let moved_from: Option<i64> = tx
            .query_row(
                "SELECT id FROM assets WHERE hash = ?1 AND missing = 1 AND id != ?2 LIMIT 1",
                params![a.hash, id],
                |r| r.get(0),
            )
            .optional()?;

        let target = match moved_from {
            Some(old) => {
                let (folder_id, path, name, ext, size, mtime): (i64, String, String, String, i64, i64) =
                    tx.query_row(
                        "SELECT folder_id, path, name, ext, size, mtime FROM assets WHERE id = ?1",
                        params![id],
                        |r| {
                            Ok((
                                r.get(0)?,
                                r.get(1)?,
                                r.get(2)?,
                                r.get(3)?,
                                r.get(4)?,
                                r.get(5)?,
                            ))
                        },
                    )?;
                tx.execute("DELETE FROM assets WHERE id = ?1", params![id])?;
                tx.execute(
                    "UPDATE assets SET folder_id = ?2, path = ?3, name = ?4, ext = ?5, size = ?6,
                            mtime = ?7, missing = 0 WHERE id = ?1",
                    params![old, folder_id, path, name, ext, size, mtime],
                )?;
                old
            }
            None => id,
        };

        tx.execute(
            "UPDATE assets SET hash = ?2, duration_ms = ?3, sample_rate = ?4, channels = ?5,
                    loudness_lufs = ?6, gain_db = ?7, status = 1, error = NULL
             WHERE id = ?1",
            params![
                target,
                a.hash,
                a.duration_ms,
                a.sample_rate,
                a.channels,
                a.loudness_lufs,
                a.gain_db
            ],
        )?;
        tx.commit()?;
        Ok(target)
    }

    pub fn fail_analysis(&self, id: i64, message: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE assets SET status = 2, error = ?2 WHERE id = ?1",
            params![id, message],
        )?;
        Ok(())
    }

    // ---- assets --------------------------------------------------------

    pub fn all_assets(&self) -> Result<Vec<Asset>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {ASSET_COLUMNS} FROM assets"))?;
        let mut assets: Vec<Asset> = stmt
            .query_map([], asset_from_row)?
            .collect::<rusqlite::Result<_>>()?;

        let mut tags: HashMap<i64, Vec<Tag>> = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT at.asset_id, t.name, t.category FROM asset_tags at
             JOIN tags t ON t.id = at.tag_id ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                Tag {
                    name: r.get(1)?,
                    category: r.get(2)?,
                },
            ))
        })?;
        for row in rows {
            let (asset_id, tag) = row?;
            tags.entry(asset_id).or_default().push(tag);
        }
        for a in &mut assets {
            if let Some(t) = tags.remove(&a.id) {
                a.tags = t;
            }
        }
        Ok(assets)
    }

    pub fn asset(&self, id: i64) -> Result<Asset> {
        let mut asset = self
            .conn
            .query_row(
                &format!("SELECT {ASSET_COLUMNS} FROM assets WHERE id = ?1"),
                params![id],
                asset_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("asset {id}")))?;
        let mut stmt = self.conn.prepare(
            "SELECT t.name, t.category FROM asset_tags at JOIN tags t ON t.id = at.tag_id
             WHERE at.asset_id = ?1 ORDER BY t.name COLLATE NOCASE",
        )?;
        asset.tags = stmt
            .query_map(params![id], |r| {
                Ok(Tag {
                    name: r.get(0)?,
                    category: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(asset)
    }

    /// Replaces the tags on an asset. Tags are created on first use; a tag
    /// that already exists keeps its category.
    pub fn set_tags(&mut self, asset_id: i64, tags: &[Tag]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM asset_tags WHERE asset_id = ?1",
            params![asset_id],
        )?;
        for tag in tags {
            let name = tag.name.trim();
            if name.is_empty() {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO tags (name, category) VALUES (?1, ?2)",
                params![name, tag.category],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO asset_tags (asset_id, tag_id)
                 SELECT ?1, id FROM tags WHERE name = ?2",
                params![asset_id, name],
            )?;
        }
        // Drop tags nothing uses any more.
        tx.execute(
            "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM asset_tags)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Adds or removes one tag on many assets at once.
    pub fn set_tag_on(&mut self, asset_ids: &[i64], tag: &Tag, on: bool) -> Result<()> {
        let name = tag.name.trim();
        if name.is_empty() || asset_ids.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        if on {
            tx.execute(
                "INSERT OR IGNORE INTO tags (name, category) VALUES (?1, ?2)",
                params![name, tag.category],
            )?;
            let tag_id: i64 =
                tx.query_row("SELECT id FROM tags WHERE name = ?1", params![name], |r| r.get(0))?;
            let mut stmt =
                tx.prepare("INSERT OR IGNORE INTO asset_tags (asset_id, tag_id) VALUES (?1, ?2)")?;
            for id in asset_ids {
                stmt.execute(params![id, tag_id])?;
            }
        } else {
            let mut stmt = tx.prepare(
                "DELETE FROM asset_tags WHERE asset_id = ?1
                 AND tag_id IN (SELECT id FROM tags WHERE name = ?2)",
            )?;
            for id in asset_ids {
                stmt.execute(params![id, name])?;
            }
            drop(stmt);
            tx.execute(
                "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM asset_tags)",
                [],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Assets (id, path) carrying none of the given tags. With `since`, only
    /// assets of that folder added at or after that time.
    pub fn assets_without_tags(
        &self,
        tag_names: &[&str],
        since: Option<(i64, i64)>,
    ) -> Result<Vec<(i64, String)>> {
        let list = tag_names
            .iter()
            .map(|t| format!("'{}'", t.to_lowercase().replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");
        let (folder_id, added_at) = since.unwrap_or((-1, 0));
        let sql = format!(
            "SELECT a.id, a.path FROM assets a
             WHERE (?1 < 0 OR (a.folder_id = ?1 AND a.added_at >= ?2))
             AND NOT EXISTS (
                 SELECT 1 FROM asset_tags at JOIN tags t ON t.id = at.tag_id
                 WHERE at.asset_id = a.id AND lower(t.name) IN ({list}))"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![folder_id, added_at], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- documents -----------------------------------------------------

    /// All documents of one kind as (id, json), oldest first.
    pub fn docs(&self, kind: &str) -> Result<Vec<(i64, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, data FROM docs WHERE kind = ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![kind], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn doc(&self, id: i64) -> Result<Option<(String, String)>> {
        Ok(self
            .conn
            .query_row("SELECT kind, data FROM docs WHERE id = ?1", params![id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?)
    }

    /// Creates (`id: None`) or replaces a document. Returns its id.
    pub fn put_doc(&self, kind: &str, id: Option<i64>, data: &str) -> Result<i64> {
        match id {
            Some(id) => {
                let n = self.conn.execute(
                    "UPDATE docs SET data = ?2, updated_at = ?3 WHERE id = ?1 AND kind = ?4",
                    params![id, data, now(), kind],
                )?;
                if n == 0 {
                    return Err(Error::NotFound(format!("{kind} {id}")));
                }
                Ok(id)
            }
            None => {
                self.conn.execute(
                    "INSERT INTO docs (kind, data, updated_at) VALUES (?1, ?2, ?3)",
                    params![kind, data, now()],
                )?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn delete_doc(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM docs WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// All tags with how many (non-missing) assets carry them.
    pub fn tags(&self) -> Result<Vec<(Tag, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.name, t.category, COUNT(a.id) FROM tags t
             JOIN asset_tags at ON at.tag_id = t.id
             JOIN assets a ON a.id = at.asset_id AND a.missing = 0
             GROUP BY t.id ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                Tag {
                    name: r.get(0)?,
                    category: r.get(1)?,
                },
                r.get(2)?,
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- settings ------------------------------------------------------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: i64) -> FoundFile {
        FoundFile {
            path: path.into(),
            name: path.rsplit('/').next().unwrap().trim_end_matches(".wav").into(),
            ext: "wav".into(),
            size,
            mtime: 100,
        }
    }

    fn analysis(hash: &str) -> Analysis {
        Analysis {
            hash: hash.into(),
            duration_ms: 1000,
            sample_rate: 48000,
            channels: 2,
            loudness_lufs: Some(-20.0),
            gain_db: 2.0,
        }
    }

    #[test]
    fn migrates_to_latest_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let v = Db::open(&path).unwrap().schema_version().unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64);
        // Re-opening must not re-run migrations.
        assert_eq!(Db::open(&path).unwrap().schema_version().unwrap(), v);
    }

    #[test]
    fn scan_adds_updates_and_marks_missing() {
        let mut db = Db::open_in_memory().unwrap();
        let folder = db.add_folder("/lib").unwrap();

        let d = db
            .apply_scan(folder, &[file("/lib/a.wav", 1), file("/lib/b.wav", 2)])
            .unwrap();
        assert_eq!(d.added, 2);
        assert_eq!(db.pending().unwrap().len(), 2);

        let a_id = db.pending().unwrap()[0].0;
        db.finish_analysis(a_id, &analysis("h-a")).unwrap();
        assert_eq!(db.pending().unwrap().len(), 1);

        // a.wav changed on disk, b.wav deleted.
        let d = db.apply_scan(folder, &[file("/lib/a.wav", 9)]).unwrap();
        assert_eq!((d.added, d.changed, d.went_missing), (0, 1, 1));
        // Changed file is re-queued; the missing one is not.
        assert_eq!(db.pending().unwrap(), vec![(a_id, "/lib/a.wav".to_string())]);

        // b.wav comes back.
        let d = db
            .apply_scan(folder, &[file("/lib/a.wav", 9), file("/lib/b.wav", 2)])
            .unwrap();
        assert_eq!(d.restored, 1);
        // Same state again: nothing to do.
        let d = db
            .apply_scan(folder, &[file("/lib/a.wav", 9), file("/lib/b.wav", 2)])
            .unwrap();
        assert!(d.is_empty());
    }

    #[test]
    fn moved_file_keeps_id_and_tags() {
        let mut db = Db::open_in_memory().unwrap();
        let folder = db.add_folder("/lib").unwrap();
        db.apply_scan(folder, &[file("/lib/old.wav", 5)]).unwrap();
        let old_id = db.pending().unwrap()[0].0;
        db.finish_analysis(old_id, &analysis("same")).unwrap();
        db.set_tags(
            old_id,
            &[Tag {
                name: "Tavern".into(),
                category: "biome".into(),
            }],
        )
        .unwrap();

        // File renamed on disk.
        db.apply_scan(folder, &[file("/lib/sub/new.wav", 5)]).unwrap();
        let new_id = db.pending().unwrap()[0].0;
        assert_ne!(new_id, old_id);
        let final_id = db.finish_analysis(new_id, &analysis("same")).unwrap();

        assert_eq!(final_id, old_id);
        let all = db.all_assets().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].path, "/lib/sub/new.wav");
        assert!(!all[0].missing);
        assert_eq!(all[0].tags[0].name, "Tavern");
    }

    #[test]
    fn tags_replace_and_prune() {
        let mut db = Db::open_in_memory().unwrap();
        let folder = db.add_folder("/lib").unwrap();
        db.apply_scan(folder, &[file("/lib/a.wav", 1)]).unwrap();
        let id = db.pending().unwrap()[0].0;
        let t = |n: &str| Tag {
            name: n.into(),
            category: "custom".into(),
        };
        db.set_tags(id, &[t("dark"), t("forest"), t("  ")]).unwrap();
        assert_eq!(db.asset(id).unwrap().tags.len(), 2);
        db.set_tags(id, &[t("FOREST")]).unwrap();
        let tags = db.tags().unwrap();
        assert_eq!(tags.len(), 1);
        // Case-insensitive match keeps the original spelling.
        assert_eq!(tags[0].0.name, "forest");
    }

    #[test]
    fn removing_a_folder_removes_its_assets() {
        let mut db = Db::open_in_memory().unwrap();
        let folder = db.add_folder("/lib").unwrap();
        db.apply_scan(folder, &[file("/lib/a.wav", 1)]).unwrap();
        db.remove_folder(folder).unwrap();
        assert!(db.all_assets().unwrap().is_empty());
    }

    #[test]
    fn settings_round_trip() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.get_setting("k").unwrap(), None);
        db.set_setting("k", "1").unwrap();
        db.set_setting("k", "2").unwrap();
        assert_eq!(db.get_setting("k").unwrap().as_deref(), Some("2"));
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;

    fn file(path: &str) -> FoundFile {
        FoundFile {
            path: path.into(),
            name: path.rsplit('/').next().unwrap().into(),
            ext: "wav".into(),
            size: 1,
            mtime: 1,
        }
    }

    fn tag(name: &str) -> Tag {
        Tag {
            name: name.into(),
            category: "custom".into(),
        }
    }

    #[test]
    fn bulk_tagging_and_untagged_lookup() {
        let mut db = Db::open_in_memory().unwrap();
        let folder = db.add_folder("/lib").unwrap();
        db.apply_scan(folder, &[file("/lib/a.wav"), file("/lib/b.wav"), file("/lib/c.wav")])
            .unwrap();
        let ids: Vec<i64> = db.all_assets().unwrap().iter().map(|a| a.id).collect();

        db.set_tag_on(&ids[..2], &tag("Music"), true).unwrap();
        db.set_tag_on(&ids[..2], &tag("Music"), true).unwrap(); // idempotent
        let untagged = db.assets_without_tags(&["music", "Ambience"], None).unwrap();
        assert_eq!(untagged, vec![(ids[2], "/lib/c.wav".to_string())]);
        assert_eq!(db.tags().unwrap()[0].1, 2);

        // Limited to one folder's files added since a moment in time.
        assert_eq!(db.assets_without_tags(&["Music"], Some((folder, 0))).unwrap().len(), 1);
        assert!(db.assets_without_tags(&["Music"], Some((folder, i64::MAX))).unwrap().is_empty());
        assert!(db.assets_without_tags(&["Music"], Some((folder + 1, 0))).unwrap().is_empty());

        db.set_tag_on(&ids, &tag("music"), false).unwrap();
        assert!(db.tags().unwrap().is_empty(), "unused tags are pruned");
    }

    #[test]
    fn documents_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let a = db.put_doc("scene", None, r#"{"name":"Tavern"}"#).unwrap();
        let b = db.put_doc("scene", None, r#"{"name":"Crypt"}"#).unwrap();
        db.put_doc("board", None, "{}").unwrap();
        assert_eq!(db.docs("scene").unwrap().len(), 2);

        db.put_doc("scene", Some(a), r#"{"name":"Inn"}"#).unwrap();
        assert_eq!(db.doc(a).unwrap().unwrap(), ("scene".into(), r#"{"name":"Inn"}"#.into()));
        // The kind must match, and the document must exist.
        assert!(db.put_doc("board", Some(a), "{}").is_err());
        assert!(db.put_doc("scene", Some(999), "{}").is_err());

        db.delete_doc(b).unwrap();
        assert_eq!(db.docs("scene").unwrap().len(), 1);
        assert!(db.doc(b).unwrap().is_none());
    }

    #[test]
    fn an_old_library_is_backed_up_before_it_is_upgraded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        {
            // A version-1 library with one folder in it.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute("INSERT INTO folders (path, added_at) VALUES ('/old', 0)", [])
                .unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len() as i64);
        assert_eq!(db.folders().unwrap().len(), 1);

        let backup = dir.path().join("library.backup-v1.db");
        assert!(backup.exists());
        let old = Connection::open(&backup).unwrap();
        let version: i64 = old.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 1);
        let folders: i64 = old.query_row("SELECT COUNT(*) FROM folders", [], |r| r.get(0)).unwrap();
        assert_eq!(folders, 1);

        // Opening again changes nothing and makes no second backup.
        drop(db);
        Db::open(&path).unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().filter(|e| {
            e.as_ref().unwrap().file_name().to_string_lossy().contains("backup")
        }).count(), 1);
    }
}
