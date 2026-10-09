//! The Markdown config. Optional YAML front matter between `---` lines sets the
//! numbers. Text before the first `## ` heading is the prompt. Each `## ` section
//! names a category: its list items are the rules, numbered by position, and the
//! text above the list is printed above those rules.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::{ConfigFile, Rules, StringsCfg, default_chunk_size};
use crate::ids::Category;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrontMatter {
    #[serde(default = "default_chunk_size")]
    chunk_size: usize,
    #[serde(default)]
    strings: StringsCfg,
}

struct Section {
    category: Category,
    line: usize,
    intro: Vec<String>,
    /// Line number and text of each rule.
    rules: Vec<(usize, String)>,
}

fn heading(c: Category) -> &'static str {
    match c {
        Category::Identifier => "Identifiers",
        Category::Comment => "Comments",
        Category::String => "Strings",
    }
}

/// The text after a list marker (`- `, `* `, `+ `, `1. ` or `1) `) at the start of a line.
fn item_text(line: &str) -> Option<&str> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    let rest = if digits > 0 {
        line[digits..].strip_prefix(['.', ')'])?
    } else {
        line.strip_prefix(['-', '*', '+'])?
    };
    (rest.is_empty() || rest.starts_with([' ', '\t'])).then_some(rest)
}

/// The YAML between a first line of `---` and the next `---` line, and the index
/// of the first line after it.
fn split_front_matter(lines: &[&str]) -> Result<(Option<String>, usize)> {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return Ok((None, 0));
    }
    let Some(end) = lines.iter().skip(1).position(|l| l.trim_end() == "---") else {
        bail!("line 1: the front matter has no closing \"---\" line");
    };
    Ok((Some(lines[1..=end].join("\n")), end + 2))
}

pub(super) fn parse(text: &str) -> Result<ConfigFile> {
    let lines: Vec<&str> = text.lines().collect();
    let (front, body) = split_front_matter(&lines)?;
    let front: FrontMatter = match front {
        Some(yaml) if !yaml.trim().is_empty() => {
            serde_saphyr::from_str(&yaml).context("front matter")?
        }
        _ => FrontMatter {
            chunk_size: default_chunk_size(),
            strings: StringsCfg::default(),
        },
    };

    let mut prompt: Vec<&str> = Vec::new();
    let mut sections: Vec<Section> = Vec::new();
    // True while the line above belongs to a rule, so an unindented line continues it.
    let mut in_rule = false;
    for (i, &line) in lines.iter().enumerate().skip(body) {
        let n = i + 1;
        if let Some(title) = line.strip_prefix("## ") {
            let title = title.trim();
            let Some(category) = Category::parse(&title.to_lowercase()) else {
                bail!(
                    "line {n}: unknown section \"## {title}\". The sections are \
                     \"## Identifiers\", \"## Comments\" and \"## Strings\"."
                );
            };
            if sections.iter().any(|s| s.category == category) {
                bail!("line {n}: a second \"## {}\" section", heading(category));
            }
            sections.push(Section {
                category,
                line: n,
                intro: Vec::new(),
                rules: Vec::new(),
            });
            in_rule = false;
            continue;
        }
        let Some(section) = sections.last_mut() else {
            prompt.push(line);
            continue;
        };
        if line.trim().is_empty() {
            if section.rules.is_empty() {
                section.intro.push(String::new());
            }
            in_rule = false;
            continue;
        }
        if let Some(start) = item_text(line) {
            section.rules.push((n, start.trim().to_string()));
            in_rule = true;
            continue;
        }
        let Some((_, rule)) = section.rules.last_mut() else {
            section.intro.push(line.to_string());
            continue;
        };
        if !in_rule && !line.starts_with([' ', '\t']) {
            bail!(
                "line {n}: text below the rules of \"## {}\". Start a rule with \"- \" or \
                 \"1. \", or indent the line to continue the rule above it.",
                heading(section.category)
            );
        }
        if !rule.is_empty() {
            rule.push(' ');
        }
        rule.push_str(line.trim());
        in_rule = true;
    }

    let mut lists: BTreeMap<Category, Vec<String>> = BTreeMap::new();
    let mut prompts = BTreeMap::new();
    for category in Category::ALL {
        let Some(section) = sections.iter().find(|s| s.category == category) else {
            bail!(
                "no \"## {}\" section. Each category needs a section with at least one rule.",
                heading(category)
            );
        };
        if section.rules.is_empty() {
            bail!(
                "line {}: \"## {}\" has no rules. Start each rule with \"- \" or \"1. \".",
                section.line,
                heading(category)
            );
        }
        if let Some((n, _)) = section.rules.iter().find(|(_, r)| r.is_empty()) {
            bail!("line {n}: empty rule");
        }
        let intro = section.intro.join("\n").trim().to_string();
        if !intro.is_empty() {
            prompts.insert(category, intro);
        }
        lists.insert(
            category,
            section.rules.iter().map(|(_, r)| r.clone()).collect(),
        );
    }
    let mut take = |c| lists.remove(&c).unwrap_or_default();

    Ok(ConfigFile {
        prompt: prompt.join("\n").trim().to_string(),
        prompts,
        rules: Rules {
            identifier: take(Category::Identifier),
            comment: take(Category::Comment),
            string: take(Category::String),
        },
        chunk_size: front.chunk_size,
        strings: front.strings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"---
chunk_size: 40
strings:
  min_words: 3
---

Shared prompt, line one.
# A level-one heading is prompt text.

## Identifiers

1. No coinage: use the established word.
2. Clear beats short. When a name is long, follow these rules: 1. do not abbreviate,
   2. remove only redundant words.

## Comments

Prose sentence for comments: keep "quotes" and the reader's #1 fact.

- No aphorism.

- Put the fact first. Reasons come
after it.

- Wrapped across a blank line.

  Second paragraph of the same rule.

## Strings

* State each fact once.
"#;

    #[test]
    fn parses_every_part() {
        let f = parse(DOC).unwrap();
        assert_eq!(f.chunk_size, 40);
        assert_eq!(f.strings.min_words, 3);
        assert_eq!(
            f.prompt,
            "Shared prompt, line one.\n# A level-one heading is prompt text."
        );
        assert_eq!(
            f.rules.identifier,
            [
                "No coinage: use the established word.",
                "Clear beats short. When a name is long, follow these rules: 1. do not \
                 abbreviate, 2. remove only redundant words.",
            ]
        );
        assert_eq!(
            f.rules.comment,
            [
                "No aphorism.",
                "Put the fact first. Reasons come after it.",
                "Wrapped across a blank line. Second paragraph of the same rule.",
            ]
        );
        assert_eq!(f.rules.string, ["State each fact once."]);
        assert_eq!(
            f.prompts.get(&Category::Comment).map(String::as_str),
            Some("Prose sentence for comments: keep \"quotes\" and the reader's #1 fact.")
        );
        assert!(!f.prompts.contains_key(&Category::Identifier));
    }

    #[test]
    fn front_matter_is_optional() {
        let f = parse("P\n## Identifiers\n- a\n## Comments\n- b\n## Strings\n- c\n").unwrap();
        assert_eq!(f.prompt, "P");
        assert_eq!(f.chunk_size, default_chunk_size());
        assert_eq!(f.strings.min_words, StringsCfg::default().min_words);
    }

    fn error(text: &str) -> String {
        format!("{:#}", parse(text).unwrap_err())
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        let tail = "## Comments\n- b\n## Strings\n- c\n";
        assert!(
            error(&format!("## Names\n- a\n{tail}"))
                .contains("line 1: unknown section \"## Names\"")
        );
        assert!(
            error(&format!("## Identifiers\n- a\n## Identifier\n- x\n{tail}"))
                .contains("line 3: a second \"## Identifiers\" section")
        );
        assert!(
            error("## Identifiers\n- a\n## Comments\n- b\n").contains("no \"## Strings\" section")
        );
        assert!(
            error(&format!("## Identifiers\nonly prose\n{tail}"))
                .contains("line 1: \"## Identifiers\" has no rules")
        );
        assert!(
            error(&format!("## Identifiers\n- a\n\nstray\n{tail}"))
                .contains("line 4: text below the rules")
        );
        assert!(error(&format!("## Identifiers\n-\n{tail}")).contains("line 2: empty rule"));
        assert!(error("---\nchunk_size: 5\n").contains("no closing \"---\""));
        assert!(
            error(&format!(
                "---\nchunk_sise: 5\n---\n## Identifiers\n- a\n{tail}"
            ))
            .contains("front matter")
        );
    }

    #[test]
    fn list_markers() {
        assert_eq!(item_text("- a"), Some(" a"));
        assert_eq!(item_text("12) a"), Some(" a"));
        assert_eq!(item_text("-"), Some(""));
        assert_eq!(item_text("---"), None);
        assert_eq!(item_text("**bold**"), None);
        assert_eq!(item_text("2026 was a year"), None);
        assert_eq!(item_text("  - indented"), None);
    }
}
