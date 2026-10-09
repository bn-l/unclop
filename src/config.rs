//! Config: prompt, rule lists and knobs. Lives in the XDG config directory as
//! Markdown, or in the older YAML format; a project-root `.unclop.yaml` can
//! append notes and override the chunk size.

mod markdown;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use etcetera::BaseStrategy;
use etcetera::base_strategy::Xdg;
use serde::{Deserialize, Serialize};

use crate::ids::Category;

pub const PROJECT_FILE: &str = ".unclop.yaml";
const MARKDOWN_FILE: &str = "config.md";
const YAML_FILE: &str = "config.yaml";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Rules {
    #[serde(default)]
    pub comment: Vec<String>,
    #[serde(default)]
    pub identifier: Vec<String>,
    #[serde(default)]
    pub string: Vec<String>,
}

impl Rules {
    pub fn for_category(&self, c: Category) -> &[String] {
        match c {
            Category::Identifier => &self.identifier,
            Category::Comment => &self.comment,
            Category::String => &self.string,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct StringsCfg {
    #[serde(default = "default_min_words")]
    pub min_words: usize,
}

impl Default for StringsCfg {
    fn default() -> Self {
        StringsCfg {
            min_words: default_min_words(),
        }
    }
}

fn default_min_words() -> usize {
    2
}

fn default_chunk_size() -> usize {
    100
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    prompt: String,
    /// Only the Markdown format has per-category prompts.
    #[serde(skip)]
    prompts: BTreeMap<Category, String>,
    #[serde(default)]
    rules: Rules,
    #[serde(default = "default_chunk_size")]
    chunk_size: usize,
    #[serde(default)]
    strings: StringsCfg,
}

#[derive(Debug, Default, Deserialize)]
struct ProjectFile {
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    chunk_size: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub prompt: String,
    /// Printed above a category's rules in chunks that contain the category.
    pub prompts: BTreeMap<Category, String>,
    pub notes: Option<String>,
    pub rules: Rules,
    pub chunk_size: usize,
    pub strings: StringsCfg,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            prompt: String::new(),
            prompts: BTreeMap::new(),
            notes: None,
            rules: Rules::default(),
            chunk_size: default_chunk_size(),
            strings: StringsCfg::default(),
        }
    }
}

pub fn config_path(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        return Ok(p.to_path_buf());
    }
    if let Ok(p) = std::env::var("UNCLOP_CONFIG")
        && !p.is_empty()
    {
        return Ok(PathBuf::from(p));
    }
    let xdg = Xdg::new().context("cannot determine the home directory")?;
    Ok(default_file(&xdg.config_dir().join("unclop")))
}

/// config.md in `dir`, or config.yaml when only that older file exists.
fn default_file(dir: &Path) -> PathBuf {
    let md = dir.join(MARKDOWN_FILE);
    let yaml = dir.join(YAML_FILE);
    if !md.exists() && yaml.exists() {
        yaml
    } else {
        md
    }
}

/// A config path ending in .md is read as Markdown, any other as YAML.
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

pub fn load(root: &Path, override_path: Option<&Path>) -> Result<Config> {
    let path = config_path(override_path)?;
    let text = fs::read_to_string(&path).with_context(|| {
        format!(
            "no config at {}. Run `unclop init` to create one.",
            path.display()
        )
    })?;
    let file = if is_markdown(&path) {
        markdown::parse(&text)
    } else {
        serde_saphyr::from_str::<ConfigFile>(&text).map_err(anyhow::Error::from)
    }
    .with_context(|| format!("parsing {}", path.display()))?;

    let project_path = root.join(PROJECT_FILE);
    let project: ProjectFile = if project_path.exists() {
        let text = fs::read_to_string(&project_path)?;
        serde_saphyr::from_str(&text)
            .with_context(|| format!("parsing {}", project_path.display()))?
    } else {
        ProjectFile::default()
    };

    let notes = project
        .notes
        .map(|n| n.trim_end().to_string())
        .filter(|n| !n.is_empty());

    Ok(Config {
        prompt: file.prompt.trim_end().to_string(),
        prompts: file.prompts,
        notes,
        rules: file.rules,
        chunk_size: project.chunk_size.unwrap_or(file.chunk_size).max(1),
        strings: file.strings,
    })
}

pub const DEFAULT_MARKDOWN: &str = r###"---
# unclop config. Text before the first "## " heading is the prompt printed at the top
# of every chunk. The "## Identifiers", "## Comments" and "## Strings" sections hold
# the rules for each category as a list, numbered by position. A rule starts with
# "- " or "1. " at the start of a line and continues on the lines below it. Text
# between a heading and its list is printed above those rules, only in chunks that
# contain the category.

# Items per chunk. Whole files are packed until the next would not fit.
chunk_size: 100

strings:
  # A string literal is listed only if it has at least this many words containing letters.
  min_words: 2
---

PROMPT PLACEHOLDER

## Identifiers

1. IDENTIFIER RULE PLACEHOLDER 1
2. IDENTIFIER RULE PLACEHOLDER 2
3. IDENTIFIER RULE PLACEHOLDER 3

## Comments

1. COMMENT RULE PLACEHOLDER 1
2. COMMENT RULE PLACEHOLDER 2
3. COMMENT RULE PLACEHOLDER 3

## Strings

1. STRING RULE PLACEHOLDER 1
2. STRING RULE PLACEHOLDER 2
3. STRING RULE PLACEHOLDER 3
"###;

/// The older config format, written when `--config` names a path that is not Markdown.
pub const DEFAULT_YAML: &str = r#"# unclop config. The prompt and rules are printed at the top of every chunk.
# Rules are plain strings, numbered by position; edit them freely.
prompt: |
  PROMPT PLACEHOLDER

rules:
  comment:
    - COMMENT RULE PLACEHOLDER 1
    - COMMENT RULE PLACEHOLDER 2
    - COMMENT RULE PLACEHOLDER 3
  identifier:
    - IDENTIFIER RULE PLACEHOLDER 1
    - IDENTIFIER RULE PLACEHOLDER 2
    - IDENTIFIER RULE PLACEHOLDER 3
  string:
    - STRING RULE PLACEHOLDER 1
    - STRING RULE PLACEHOLDER 2
    - STRING RULE PLACEHOLDER 3

# Items per chunk. Whole files are packed until the next would not fit.
chunk_size: 100

strings:
  # A string literal is listed only if it has at least this many words containing letters.
  min_words: 2
"#;

/// Writes the default config when the file does not exist. Returns true if it wrote.
pub fn write_default_if_absent(path: &Path) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let default = if is_markdown(path) {
        DEFAULT_MARKDOWN
    } else {
        DEFAULT_YAML
    };
    fs::write(path, default).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_configs_parse() {
        let md = markdown::parse(DEFAULT_MARKDOWN).unwrap();
        let yaml: ConfigFile = serde_saphyr::from_str(DEFAULT_YAML).unwrap();
        for file in [md, yaml] {
            assert_eq!(file.rules.comment.len(), 3);
            assert_eq!(file.rules.identifier.len(), 3);
            assert_eq!(file.rules.string.len(), 3);
            assert_eq!(file.chunk_size, 100);
            assert_eq!(file.strings.min_words, 2);
            assert_eq!(file.prompt.trim(), "PROMPT PLACEHOLDER");
            assert!(file.prompts.is_empty());
        }
    }

    #[test]
    fn markdown_wins_over_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let (md, yaml) = (dir.path().join(MARKDOWN_FILE), dir.path().join(YAML_FILE));
        assert_eq!(
            default_file(dir.path()),
            md,
            "neither exists: init writes Markdown"
        );
        fs::write(&yaml, "").unwrap();
        assert_eq!(default_file(dir.path()), yaml, "only the old file exists");
        fs::write(&md, "").unwrap();
        assert_eq!(default_file(dir.path()), md);
        assert!(is_markdown(Path::new("a/config.MD")));
        assert!(!is_markdown(Path::new("a/config.yaml")));
        assert!(!is_markdown(Path::new("a/config")));
    }

    #[test]
    fn project_file_parses() {
        let p: ProjectFile =
            serde_saphyr::from_str("notes: |\n  keep ctx\nchunk_size: 40\n").unwrap();
        assert_eq!(p.chunk_size, Some(40));
        assert_eq!(p.notes.unwrap().trim(), "keep ctx");
    }
}
