//! Kinds, categories, text normalization and content hashes.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::xxh3_64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Identifier,
    Comment,
    String,
}

impl Category {
    pub const ALL: [Category; 3] = [Category::Identifier, Category::Comment, Category::String];

    pub fn label(self) -> &'static str {
        match self {
            Category::Identifier => "identifiers",
            Category::Comment => "comments",
            Category::String => "strings",
        }
    }

    /// The singular name used as the config key and by `next --only`.
    pub fn name(self) -> &'static str {
        match self {
            Category::Identifier => "identifier",
            Category::Comment => "comment",
            Category::String => "string",
        }
    }

    /// Accepts the singular config key or the plural label.
    pub fn parse(s: &str) -> Option<Category> {
        Some(match s {
            "identifier" | "identifiers" => Category::Identifier,
            "comment" | "comments" => Category::Comment,
            "string" | "strings" => Category::String,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Fn,
    Type,
    Variant,
    Field,
    Var,
    Const,
    Param,
    Mod,
    Macro,
    Comment,
    Doc,
    #[serde(rename = "string")]
    Str,
}

impl Kind {
    pub fn category(self) -> Category {
        match self {
            Kind::Comment | Kind::Doc => Category::Comment,
            Kind::Str => Category::String,
            _ => Category::Identifier,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Fn => "fn",
            Kind::Type => "type",
            Kind::Variant => "variant",
            Kind::Field => "field",
            Kind::Var => "var",
            Kind::Const => "const",
            Kind::Param => "param",
            Kind::Mod => "mod",
            Kind::Macro => "macro",
            Kind::Comment => "comment",
            Kind::Doc => "doc",
            Kind::Str => "string",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "fn" => Kind::Fn,
            "type" => Kind::Type,
            "variant" => Kind::Variant,
            "field" => Kind::Field,
            "var" => Kind::Var,
            "const" => Kind::Const,
            "param" => Kind::Param,
            "mod" => Kind::Mod,
            "macro" => Kind::Macro,
            "comment" => Kind::Comment,
            "doc" => Kind::Doc,
            "string" => Kind::Str,
            _ => return None,
        })
    }

    /// When two query patterns capture the same node, the more specific kind wins.
    pub fn priority(self) -> u8 {
        match self {
            Kind::Field => 6,
            Kind::Const => 5,
            Kind::Param => 4,
            Kind::Fn | Kind::Type | Kind::Variant | Kind::Mod | Kind::Macro => 3,
            Kind::Var => 1,
            Kind::Comment | Kind::Doc | Kind::Str => 0,
        }
    }
}

pub fn hash_id(kind: Kind, normalized: &str) -> String {
    let mut buf = Vec::with_capacity(normalized.len() + 24);
    buf.extend_from_slice(kind.category().label().as_bytes());
    buf.push(0);
    buf.extend_from_slice(kind.label().as_bytes());
    buf.push(0);
    buf.extend_from_slice(normalized.as_bytes());
    let h = xxh3_64(&buf);
    format!("{h:016x}")[..10].to_string()
}

pub fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Content of a string literal without its quotes, prefix letters or raw-string hashes.
/// Returns the input unchanged when it does not look quoted.
pub fn strip_quotes(raw: &str) -> &str {
    let s = raw.trim();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && i < 3 && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'@') {
        i += 1;
    }
    let prefix = &s[..i];
    let mut j = i;
    while j < bytes.len() && bytes[j] == b'#' {
        j += 1;
    }
    let hashes = j - i;
    let body = &s[j..];
    for q in ["\"\"\"", "'''"] {
        if body.len() >= 6 && body.starts_with(q) && body.ends_with(q) {
            return &body[3..body.len() - 3];
        }
    }
    for q in ['"', '\'', '`'] {
        if body.starts_with(q) {
            let inner = if hashes > 0 && body.ends_with(&"#".repeat(hashes)) {
                &body[..body.len() - hashes]
            } else {
                body
            };
            if inner.len() >= 2 && inner.ends_with(q) {
                let inner = &inner[1..inner.len() - 1];
                // C++ raw string: R"delim(...)delim"
                if prefix.contains('R')
                    && let Some(open) = inner.find('(')
                {
                    let delim = &inner[..open];
                    let close = format!("){delim}");
                    if inner.ends_with(&close) && inner.len() >= open + 1 + close.len() {
                        return &inner[open + 1..inner.len() - close.len()];
                    }
                }
                return inner;
            }
        }
    }
    s
}

/// Comment text with delimiters removed and whitespace collapsed to single spaces.
pub fn normalize_comment(raw: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let mut l = line.trim();
        if let Some(r) = l.strip_suffix("*/") {
            l = r.trim_end();
        }
        for m in [
            "/*!", "/**", "/*", "///", "//!", "//", "#!", "#", "=begin", "=end",
        ] {
            if let Some(r) = l.strip_prefix(m) {
                l = r;
                break;
            }
        }
        if let Some(r) = l.strip_prefix('*') {
            l = r;
        }
        let l = l.trim();
        if !l.is_empty() {
            lines.push(l);
        }
    }
    collapse_ws(&lines.join(" "))
}

static PRAGMA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(eslint|@ts-|prettier-ignore|biome-ignore|istanbul |c8 |v8 |noqa|type: ?ignore|type: ?\(|pylint:|mypy:|fmt: ?(on|off)|go:|nolint|rubocop:|frozen_string_literal|shellcheck|-\*-|vim:|vi:|coding[:=]|@generated|sourceMappingURL|#?(end)?region\b|\+build |@flow\b|@jsx\b|@ts-check\b|jshint|jslint|globals? )",
    )
    .unwrap()
});

static LICENSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(SPDX-License-Identifier|Copyright\s*(\(c\)|©|\d{4})|\(c\)\s*\d{4}|All rights reserved|Licensed under the|Permission is hereby granted|WITHOUT WARRANTIES OR CONDITIONS)",
    )
    .unwrap()
});

/// A tool directive such as `eslint-disable` or `@ts-expect-error`.
pub fn is_pragma(normalized: &str) -> bool {
    PRAGMA.is_match(normalized)
}

/// Comments that are machinery rather than prose: pragmas, separators, license headers, shebangs.
pub fn is_noise_comment(normalized: &str, raw: &str, first_row: usize) -> bool {
    if !normalized.chars().any(|c| c.is_alphabetic()) {
        return true;
    }
    if first_row == 0 && raw.trim_start().starts_with("#!") {
        return true;
    }
    is_pragma(normalized) || LICENSE.is_match(normalized)
}

static MACHINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(https?://|wss?://|ftp://|file://|\.{0,2}/|~/|[a-z]:\\|data:|mailto:|[0-9a-f]{7,}$|\d{4}-\d{2}-\d{2}|#[0-9a-f]{3,8}$|[^\s@]+@[^\s@]+\.[^\s@]+$|application/|text/|image/)",
    )
    .unwrap()
});

/// True when a string literal reads as prose worth reviewing.
pub fn worth_string(normalized: &str, min_words: usize) -> bool {
    let words = normalized
        .split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphabetic()))
        .count();
    if words < min_words {
        return false;
    }
    let dense: Vec<char> = normalized.chars().filter(|c| !c.is_whitespace()).collect();
    if dense.is_empty() {
        return false;
    }
    let letters = dense.iter().filter(|c| c.is_alphabetic()).count();
    if (letters as f64) / (dense.len() as f64) < 0.6 {
        return false;
    }
    !MACHINE.is_match(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_delimiters() {
        assert_eq!(normalize_comment("// hello  world"), "hello world");
        assert_eq!(normalize_comment("/// doc line"), "doc line");
        assert_eq!(normalize_comment("/* a\n * b\n */"), "a b");
        assert_eq!(normalize_comment("/** a\n * b */"), "a b");
        assert_eq!(normalize_comment("# python"), "python");
        assert_eq!(normalize_comment("=begin\nruby\n=end"), "ruby");
    }

    #[test]
    fn strips_quotes() {
        assert_eq!(strip_quotes("\"abc\""), "abc");
        assert_eq!(strip_quotes("'abc'"), "abc");
        assert_eq!(strip_quotes("`abc`"), "abc");
        assert_eq!(strip_quotes("r#\"abc\"#"), "abc");
        assert_eq!(strip_quotes("b\"abc\""), "abc");
        assert_eq!(strip_quotes("f\"abc\""), "abc");
        assert_eq!(strip_quotes("u8\"abc\""), "abc");
        assert_eq!(strip_quotes("R\"(raw text)\""), "raw text");
        assert_eq!(strip_quotes("R\"xy(raw (text))xy\""), "raw (text)");
        assert_eq!(strip_quotes("\"\"\"doc\"\"\""), "doc");
        assert_eq!(strip_quotes("r\"\"\"doc\"\"\""), "doc");
        assert_eq!(strip_quotes("plain"), "plain");
    }

    #[test]
    fn noise() {
        assert!(is_noise_comment("----------", "// ----------", 3));
        assert!(is_noise_comment(
            "eslint-disable-next-line",
            "// eslint-disable-next-line",
            3
        ));
        assert!(is_noise_comment("type: ignore", "# type: ignore", 3));
        assert!(is_noise_comment(
            "SPDX-License-Identifier: MIT",
            "// SPDX-License-Identifier: MIT",
            0
        ));
        assert!(is_noise_comment("bin/bash", "#!/bin/bash", 0));
        assert!(!is_noise_comment("bin/bash", "#!/bin/bash", 4));
        assert!(!is_noise_comment("Helper", "// Helper", 3));
        assert!(!is_noise_comment(
            "Regional settings are loaded lazily",
            "// Regional settings are loaded lazily",
            3
        ));
    }

    #[test]
    fn worth() {
        assert!(worth_string("An unexpected error occurred", 2));
        assert!(!worth_string("error", 2));
        assert!(!worth_string("https://example.com/a b", 2));
        assert!(!worth_string("./src/foo bar", 2));
        assert!(!worth_string("%s: %d", 2));
        assert!(!worth_string("a1b2c3 d4e5f6 000", 2));
    }

    #[test]
    fn ids_are_ten_hex() {
        let id = hash_id(Kind::Fn, "foo");
        assert_eq!(id.len(), 10);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(hash_id(Kind::Fn, "foo"), hash_id(Kind::Var, "foo"));
    }
}
