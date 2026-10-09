//! The state file: one JSON record per line in `.unclop.jsonl` at the project root.
//!
//! The file is locked for the whole command and rewritten in place, so every
//! process contends on the same inode. During a save the new content goes to a
//! journal file first; if a save is interrupted, the next load recovers from it.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::ids::{Category, Kind};

pub const STATE_FILE: &str = ".unclop.jsonl";
pub const JOURNAL_FILE: &str = ".unclop.jsonl.tmp";
const VERSION: u32 = 1;

pub fn state_path(root: &Path) -> PathBuf {
    root.join(STATE_FILE)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Done,
    Skipped,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: String,
    #[serde(default)]
    pub mtime: u64,
    #[serde(default)]
    pub size: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub skipped: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub path: String,
    pub id: String,
    pub kind: Kind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub line: [u32; 2],
    pub status: Status,
    #[serde(default)]
    pub ticks: Vec<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub changed_after_done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// Hash of the source line the item ends on and the next non-blank line. It
    /// tells identical items in one file apart when one of them is deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx: Option<u64>,
}

impl Item {
    pub fn key(&self) -> String {
        key_of(&self.path, &self.id)
    }

    pub fn is_pending(&self) -> bool {
        self.status == Status::Pending
    }

    /// The id the agent was last shown for this item: the alias while the item's
    /// id has changed since a chunk printed it, the id otherwise.
    pub fn shown_id(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.id)
    }
}

pub fn key_of(path: &str, id: &str) -> String {
    format!("{path}#{id}")
}

/// A query of six to nine characters without an ordinal abbreviates an id's hash.
/// A full ten-character id is never a prefix of `id~2`: it names a different item.
fn is_prefix_query(q: &str) -> bool {
    (6..10).contains(&q.len()) && !q.contains('~')
}

/// Splits `path#id` at the last `#`: ids never contain one, paths can.
pub fn split_key(key: &str) -> Option<(&str, &str)> {
    key.rsplit_once('#')
}

/// Why `State::resolve` found no single item.
#[derive(Debug)]
pub enum ResolveError {
    NotFound(String),
    /// Several items match. Never a sign that an item was deleted.
    Ambiguous(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound(m) | ResolveError::Ambiguous(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ResolveError {}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Chunk {
    pub worker: String,
    pub keys: Vec<String>,
    pub issued: u64,
    #[serde(default)]
    pub snapshot: BTreeMap<String, String>,
    /// The category `next --only` limited the chunk to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only: Option<Category>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum Record {
    /// Opens the file with the scan key and closes it with `end`, so a file cut short
    /// at a line boundary is told apart from a complete one. Older versions read
    /// both as plain meta records.
    Meta {
        v: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scan: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        end: bool,
    },
    File(FileRecord),
    Item(Item),
    Chunk(Chunk),
}

#[derive(Clone, Debug, Default)]
pub struct FileEntry {
    pub record: FileRecord,
    pub items: Vec<Item>,
}

#[derive(Debug, Default)]
pub struct State {
    pub files: BTreeMap<String, FileEntry>,
    pub chunks: BTreeMap<String, Chunk>,
    /// The scan settings the stored items were extracted with. A file is parsed
    /// again when they differ, even if its size and modification time did not change.
    pub scan_key: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub pending: usize,
    pub done: usize,
    pub skipped: usize,
}

impl Counts {
    pub fn add(&mut self, item: &Item) {
        match item.status {
            Status::Pending => self.pending += 1,
            Status::Done => self.done += 1,
            Status::Skipped => self.skipped += 1,
        }
    }
}

impl State {
    /// Parses the JSONL text of a state file. `origin` is only used in error messages.
    pub fn parse(text: &str, origin: &Path) -> Result<State> {
        Ok(Self::parse_checked(text, origin)?.0)
    }

    /// Parses like `parse` and also reports whether the text ends with the end record.
    fn parse_checked(text: &str, origin: &Path) -> Result<(State, bool)> {
        let mut state = State::default();
        let mut complete = false;
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let rec: Record = serde_json::from_str(line)
                .with_context(|| format!("{}:{}: bad state record", origin.display(), n + 1))?;
            complete = matches!(rec, Record::Meta { end: true, .. });
            match rec {
                Record::Meta { v, scan, .. } => {
                    if v > VERSION {
                        bail!(
                            "state file version {v} is newer than this binary supports ({VERSION})"
                        );
                    }
                    if let Some(scan) = scan {
                        state.scan_key = scan;
                    }
                }
                Record::File(rec) => {
                    let entry = state.entry_mut(&rec.path.clone());
                    entry.record = rec;
                }
                Record::Item(item) => {
                    state.entry_mut(&item.path.clone()).items.push(item);
                }
                Record::Chunk(chunk) => {
                    state.chunks.insert(chunk.worker.clone(), chunk);
                }
            }
        }
        Ok((state, complete))
    }

    /// JSONL: meta, then per file its record followed by its items, then chunks,
    /// then the end record.
    pub fn serialize(&self) -> Result<String> {
        let mut buf = String::new();
        buf.push_str(&serde_json::to_string(&Record::Meta {
            v: VERSION,
            scan: Some(self.scan_key.clone()).filter(|s| !s.is_empty()),
            end: false,
        })?);
        buf.push('\n');
        for entry in self.files.values() {
            buf.push_str(&serde_json::to_string(&Record::File(entry.record.clone()))?);
            buf.push('\n');
            for item in &entry.items {
                buf.push_str(&serde_json::to_string(&Record::Item(item.clone()))?);
                buf.push('\n');
            }
        }
        for chunk in self.chunks.values() {
            buf.push_str(&serde_json::to_string(&Record::Chunk(chunk.clone()))?);
            buf.push('\n');
        }
        buf.push_str(&serde_json::to_string(&Record::Meta {
            v: VERSION,
            scan: None,
            end: true,
        })?);
        buf.push('\n');
        Ok(buf)
    }

    pub fn entry_mut(&mut self, path: &str) -> &mut FileEntry {
        self.files
            .entry(path.to_string())
            .or_insert_with(|| FileEntry {
                record: FileRecord {
                    path: path.to_string(),
                    ..FileRecord::default()
                },
                items: Vec::new(),
            })
    }

    pub fn all_items(&self) -> impl Iterator<Item = &Item> {
        self.files.values().flat_map(|e| e.items.iter())
    }

    pub fn all_items_mut(&mut self) -> impl Iterator<Item = &mut Item> {
        self.files.values_mut().flat_map(|e| e.items.iter_mut())
    }

    pub fn item_by_key(&self, key: &str) -> Option<&Item> {
        let (path, id) = split_key(key)?;
        self.files.get(path)?.items.iter().find(|i| i.id == id)
    }

    pub fn get_mut(&mut self, path: &str, id: &str) -> Option<&mut Item> {
        self.files
            .get_mut(path)?
            .items
            .iter_mut()
            .find(|i| i.id == id)
    }

    /// Resolves an id the way the agent typed it: `path#id`, a full id, an alias
    /// (the id an item had when it was issued), or a unique prefix of six or more chars.
    pub fn resolve(&self, query: &str) -> Result<(String, String), ResolveError> {
        let q = query.trim();
        if q.is_empty() {
            return Err(ResolveError::NotFound("empty id".to_string()));
        }
        let found = |i: &Item| Ok((i.path.clone(), i.id.clone()));
        let ambiguous = |items: &[&Item]| {
            Err(ResolveError::Ambiguous(format!(
                "{q} matches {} items; use path#id: {}",
                items.len(),
                list_keys(items)
            )))
        };
        if let Some((path, id)) = split_key(q) {
            let item = self.files.get(path).and_then(|e| {
                e.items
                    .iter()
                    .find(|i| i.id == id || i.alias.as_deref() == Some(id))
            });
            return item.map_or_else(
                || Err(ResolveError::NotFound(format!("no item {q}"))),
                found,
            );
        }
        let exact: Vec<&Item> = self
            .all_items()
            .filter(|i| i.id == q || i.alias.as_deref() == Some(q))
            .collect();
        match exact.len() {
            1 => return found(exact[0]),
            n if n > 1 => return ambiguous(&exact),
            _ => {}
        }
        if is_prefix_query(q) {
            let prefix: Vec<&Item> = self
                .all_items()
                .filter(|i| {
                    i.id.starts_with(q) || i.alias.as_deref().is_some_and(|a| a.starts_with(q))
                })
                .collect();
            match prefix.len() {
                1 => return found(prefix[0]),
                n if n > 1 => return ambiguous(&prefix),
                _ => {}
            }
        }
        Err(ResolveError::NotFound(format!("no item matches {q}")))
    }

    /// The chunk key an id was printed under, for an id that matches no item:
    /// the item was deleted from the source after the chunk was issued.
    pub fn issued_key(&self, query: &str) -> Option<String> {
        let q = query.trim();
        self.chunks.values().find_map(|c| {
            c.keys
                .iter()
                .find(|k| {
                    split_key(k)
                        .is_some_and(|(_, id)| id == q || (is_prefix_query(q) && id.starts_with(q)))
                })
                .cloned()
        })
    }

    /// The most recently issued chunk that holds `key`. Chunks from an earlier run
    /// with a different worker count can hold it too, with older text.
    pub fn chunk_holding(&self, key: &str) -> Option<&Chunk> {
        self.chunks
            .values()
            .filter(|c| c.keys.iter().any(|k| k == key))
            .max_by_key(|c| c.issued)
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for item in self.all_items() {
            c.add(item);
        }
        c
    }

    pub fn counts_by_category(&self) -> BTreeMap<Category, Counts> {
        let mut map: BTreeMap<Category, Counts> = BTreeMap::new();
        for cat in Category::ALL {
            map.insert(cat, Counts::default());
        }
        for item in self.all_items() {
            map.get_mut(&item.kind.category()).unwrap().add(item);
        }
        map
    }

    /// Pending items and the number of files that have at least one.
    pub fn pending(&self) -> (usize, usize) {
        let mut items = 0;
        let mut files = 0;
        for entry in self.files.values() {
            let n = entry.items.iter().filter(|i| i.is_pending()).count();
            if n > 0 {
                files += 1;
                items += n;
            }
        }
        (items, files)
    }
}

fn list_keys(items: &[&Item]) -> String {
    items
        .iter()
        .take(6)
        .map(|i| i.key())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The locked state file, open for the duration of one command.
pub struct StateFile<'a> {
    file: &'a mut File,
    root: &'a Path,
}

impl StateFile<'_> {
    pub fn load(&mut self) -> Result<State> {
        let path = state_path(self.root);
        let mut text = String::new();
        self.file.seek(SeekFrom::Start(0))?;
        self.file
            .read_to_string(&mut text)
            .with_context(|| format!("reading {}", path.display()))?;
        let parsed = State::parse_checked(&text, &path);

        // A save writes the journal first, then truncates and rewrites the state
        // file, then deletes the journal. A state file that is empty, unparseable or
        // missing its end record next to a journal means that save was cut short.
        let journal = self.root.join(JOURNAL_FILE);
        let recovered = journal
            .is_file()
            .then(|| fs::read_to_string(&journal).ok())
            .flatten()
            .filter(|t| !t.trim().is_empty())
            .and_then(|t| State::parse_checked(&t, &journal).ok());
        let use_journal = match (&parsed, &recovered) {
            (_, None) => false,
            // A state file from an older version has no end record; only a journal
            // that has one is known to be more complete.
            (Ok((_, false)), Some((_, journal_complete))) if !text.trim().is_empty() => {
                *journal_complete
            }
            (Ok((_, complete)), Some(_)) => !complete,
            (Err(_), Some(_)) => true,
        };
        if use_journal && let Some((state, _)) = recovered {
            eprintln!("unclop: recovered state from an interrupted save");
            return Ok(state);
        }
        parsed.map(|(state, _)| state)
    }

    pub fn save(&mut self, state: &State) -> Result<()> {
        let buf = state.serialize()?;
        let journal = self.root.join(JOURNAL_FILE);
        {
            let mut j =
                File::create(&journal).with_context(|| format!("writing {}", journal.display()))?;
            j.write_all(buf.as_bytes())?;
            j.sync_all()?;
        }
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file
            .write_all(buf.as_bytes())
            .with_context(|| format!("writing {}", state_path(self.root).display()))?;
        self.file.sync_all()?;
        let _ = fs::remove_file(&journal);
        Ok(())
    }
}

/// Opens `.unclop.jsonl` (creating it if needed), takes the exclusive lock, and
/// runs `f` with a handle that reads and writes through the locked descriptor.
pub fn with_lock<T>(root: &Path, f: impl FnOnce(&mut StateFile) -> Result<T>) -> Result<T> {
    let path = state_path(root);
    let file = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    let mut lock = fd_lock::RwLock::new(file);
    let mut guard = lock.write().context("locking the state file")?;
    let mut handle = StateFile {
        file: &mut guard,
        root,
    };
    f(&mut handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str, id: &str, status: Status) -> Item {
        Item {
            path: path.into(),
            id: id.into(),
            kind: Kind::Fn,
            text: "name".into(),
            scope: None,
            line: [1, 1],
            status,
            ticks: vec![true, false],
            was: None,
            changed_after_done: false,
            alias: Some("old".into()),
            ctx: None,
        }
    }

    fn sample() -> State {
        let mut state = State::default();
        state
            .entry_mut("a.rs")
            .items
            .push(item("a.rs", "abc123", Status::Pending));
        state.entry_mut("a.rs").record.mtime = 5;
        state.entry_mut("b.rs").record.skipped = true;
        state.chunks.insert(
            "1/1".into(),
            Chunk {
                worker: "1/1".into(),
                keys: vec!["a.rs#abc123".into()],
                issued: 7,
                snapshot: [("a.rs#abc123".to_string(), "name".to_string())].into(),
                only: Some(Category::Identifier),
            },
        );
        state
    }

    #[test]
    fn paths_may_contain_hash() {
        let mut state = State::default();
        state.entry_mut("src/c#/a.rs").items.push(item(
            "src/c#/a.rs",
            "abcdef0123",
            Status::Pending,
        ));
        assert!(state.item_by_key("src/c#/a.rs#abcdef0123").is_some());
        assert_eq!(
            state.resolve("src/c#/a.rs#abcdef0123").unwrap(),
            ("src/c#/a.rs".to_string(), "abcdef0123".to_string())
        );
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let state = sample();
        with_lock(dir.path(), |sf| {
            assert!(
                sf.load()?.files.is_empty(),
                "fresh file loads as empty state"
            );
            sf.save(&state)?;
            let loaded = sf.load()?;
            assert_eq!(loaded.files.len(), 2);
            assert_eq!(loaded.files["a.rs"].record.mtime, 5);
            assert!(loaded.files["b.rs"].record.skipped);
            assert_eq!(loaded.files["a.rs"].items[0].alias.as_deref(), Some("old"));
            assert_eq!(loaded.chunks["1/1"].keys, vec!["a.rs#abc123"]);
            assert_eq!(loaded.chunks["1/1"].only, Some(Category::Identifier));
            Ok(())
        })
        .unwrap();
        assert!(dir.path().join(STATE_FILE).is_file());
        assert!(!dir.path().join(JOURNAL_FILE).exists());
        let entries: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(entries.len(), 1, "only the state file is left behind");
    }

    #[test]
    fn recovers_from_interrupted_save() {
        let dir = tempfile::tempdir().unwrap();
        let state = sample();
        fs::write(dir.path().join(JOURNAL_FILE), state.serialize().unwrap()).unwrap();
        fs::write(dir.path().join(STATE_FILE), "").unwrap();
        with_lock(dir.path(), |sf| {
            let loaded = sf.load()?;
            assert_eq!(loaded.files.len(), 2);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_save_cut_at_a_line_boundary_recovers_from_the_journal() {
        let dir = tempfile::tempdir().unwrap();
        let full = sample().serialize().unwrap();
        let cut: String = full.lines().take(2).map(|l| format!("{l}\n")).collect();
        assert!(
            State::parse(&cut, Path::new("cut")).is_ok(),
            "the cut file still parses"
        );
        fs::write(dir.path().join(JOURNAL_FILE), &full).unwrap();
        fs::write(dir.path().join(STATE_FILE), cut).unwrap();
        with_lock(dir.path(), |sf| {
            let loaded = sf.load()?;
            assert_eq!(loaded.files.len(), 2);
            assert_eq!(loaded.chunks.len(), 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn an_old_file_without_the_end_record_loads() {
        let dir = tempfile::tempdir().unwrap();
        let old: String = sample()
            .serialize()
            .unwrap()
            .lines()
            .filter(|l| !l.contains("\"end\""))
            .map(|l| format!("{l}\n"))
            .collect();
        fs::write(dir.path().join(STATE_FILE), old).unwrap();
        with_lock(dir.path(), |sf| {
            assert_eq!(sf.load()?.files.len(), 2);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn corrupt_state_without_journal_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(STATE_FILE), "{not json\n").unwrap();
        let result = with_lock(dir.path(), |sf| sf.load());
        assert!(result.is_err());
    }

    #[test]
    fn resolves_ids() {
        let mut state = State::default();
        state
            .entry_mut("a.rs")
            .items
            .push(item("a.rs", "abcdef0123", Status::Pending));
        state
            .entry_mut("b.rs")
            .items
            .push(item("b.rs", "abcdef9999", Status::Pending));
        assert_eq!(state.resolve("abcdef0123").unwrap().0, "a.rs");
        assert_eq!(state.resolve("abcdef99").unwrap().0, "b.rs");
        assert!(state.resolve("abcdef").is_err());
        assert_eq!(state.resolve("b.rs#abcdef9999").unwrap().0, "b.rs");
        assert!(state.resolve("old").is_err());
    }
}
