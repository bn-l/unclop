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
    /// The marker of the last rule.
    marker: Option<Marker>,
}

fn heading(c: Category) -> &'static str {
    match c {
        Category::Identifier => "Identifiers",
        Category::Comment => "Comments",
        Category::String => "Strings",
    }
}

/// How a list item is marked: the bullet character, or the number and the
/// character after it.
#[derive(Clone, Copy, PartialEq)]
enum Marker {
    Bullet(char),
    Number(u32, char),
}

impl Marker {
    /// Whether an item marked `next` continues the list this item is in. Markdown
    /// starts a new list when the marker character changes; a number lower than
    /// the one before means a second numbered list.
    fn continues(self, next: Marker) -> bool {
        match (self, next) {
            (Marker::Bullet(a), Marker::Bullet(b)) => a == b,
            (Marker::Number(n, a), Marker::Number(m, b)) => a == b && m >= n,
            _ => false,
        }
    }
}

/// The marker and the text after it, for a list item (`- `, `* `, `+ `, `1. ` or
/// `1) `) at the start of a line.
fn list_item(line: &str) -> Option<(Marker, &str)> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    let (marker, rest) = if digits > 0 {
        let number = line[..digits].parse().ok()?;
        let delimiter = line[digits..]
            .chars()
            .next()
            .filter(|c| matches!(c, '.' | ')'))?;
        (Marker::Number(number, delimiter), &line[digits + 1..])
    } else {
        let bullet = line
            .chars()
            .next()
            .filter(|c| matches!(c, '-' | '*' | '+'))?;
        (Marker::Bullet(bullet), &line[1..])
    };
    (rest.is_empty() || rest.starts_with([' ', '\t'])).then_some((marker, rest))
}

#[cfg(test)]
fn item_text(line: &str) -> Option<&str> {
    list_item(line).map(|(_, text)| text)
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
                marker: None,
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
        if let Some((marker, start)) = list_item(line) {
            if section.marker.is_some_and(|m| !m.continues(marker)) {
                bail!(
                    "line {n}: a second list starts here. Every list item in a section is a \
                     rule, so indent a list that belongs to the section's prompt."
                );
            }
            section.marker = Some(marker);
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
    fn a_list_in_a_section_prompt_must_be_indented() {
        let doc = "## Identifiers\n- a\n## Comments\nSteps:\n1. Read it.\n2. Rewrite it.\n\n\
                   1. No aphorism.\n2. No coinage.\n## Strings\n- c\n";
        assert!(
            error(doc).contains("line 8: a second list starts here"),
            "{}",
            error(doc)
        );
        let bullets = doc
            .replace("1. Read", "- Read")
            .replace("2. Rewrite", "- Rewrite");
        assert!(error(&bullets).contains("line 8: a second list starts here"));

        let indented = doc
            .replace("1. Read", "   1. Read")
            .replace("2. Rewrite", "   2. Rewrite");
        let f = parse(&indented).unwrap();
        assert_eq!(f.rules.comment, ["No aphorism.", "No coinage."]);
        assert!(f.prompts[&Category::Comment].contains("1. Read it."));
    }

    #[test]
    fn one_list_may_repeat_or_skip_numbers() {
        let doc = "## Identifiers\n1. a\n1. b\n\n3. c\n## Comments\n- b\n## Strings\n- c\n";
        assert_eq!(parse(doc).unwrap().rules.identifier, ["a", "b", "c"]);
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
