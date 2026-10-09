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
}

impl Item {
    pub fn key(&self) -> String {
        key_of(&self.path, &self.id)
    }

    pub fn is_pending(&self) -> bool {
        self.status == Status::Pending
    }
}

pub fn key_of(path: &str, id: &str) -> String {
    format!("{path}#{id}")
}

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
    Meta { v: u32 },
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
        let mut state = State::default();
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let rec: Record = serde_json::from_str(line)
                .with_context(|| format!("{}:{}: bad state record", origin.display(), n + 1))?;
            match rec {
                Record::Meta { v } => {
                    if v > VERSION {
                        bail!(
                            "state file version {v} is newer than this binary supports ({VERSION})"
                        );
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
        Ok(state)
    }

    /// JSONL: meta, then per file its record followed by its items, then chunks.
    pub fn serialize(&self) -> Result<String> {
        let mut buf = String::new();
        buf.push_str(&serde_json::to_string(&Record::Meta { v: VERSION })?);
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
        let (path, id) = key.split_once('#')?;
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
    pub fn resolve(&self, query: &str) -> Result<(String, String)> {
        let q = query.trim();
        if q.is_empty() {
            bail!("empty id");
        }
        if let Some((path, id)) = q.split_once('#') {
            return match self.item_by_key(q) {
                Some(_) => Ok((path.to_string(), id.to_string())),
                None => bail!("no item {q}"),
            };
        }
        let exact: Vec<&Item> = self
            .all_items()
            .filter(|i| i.id == q || i.alias.as_deref() == Some(q))
            .collect();
        match exact.len() {
            1 => return Ok((exact[0].path.clone(), exact[0].id.clone())),
            n if n > 1 => bail!("{q} matches {n} items; use path#id: {}", list_keys(&exact)),
            _ => {}
        }
        if q.len() >= 6 {
            let prefix: Vec<&Item> = self
                .all_items()
                .filter(|i| {
                    i.id.starts_with(q) || i.alias.as_deref().is_some_and(|a| a.starts_with(q))
                })
                .collect();
            match prefix.len() {
                1 => return Ok((prefix[0].path.clone(), prefix[0].id.clone())),
                n if n > 1 => bail!("{q} matches {n} items; use path#id: {}", list_keys(&prefix)),
                _ => {}
            }
        }
        bail!("no item matches {q}")
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
        let parsed = State::parse(&text, &path);

        // An empty or unparseable file next to a journal means a save was cut
        // short between truncating and writing. The journal holds the full content.
        let journal = self.root.join(JOURNAL_FILE);
        if (text.trim().is_empty() || parsed.is_err())
            && journal.is_file()
            && let Ok(journal_text) = fs::read_to_string(&journal)
            && !journal_text.trim().is_empty()
            && let Ok(state) = State::parse(&journal_text, &journal)
        {
            eprintln!("unclop: recovered state from an interrupted save");
            return Ok(state);
        }
        parsed
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
