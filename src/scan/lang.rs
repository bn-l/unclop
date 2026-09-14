//! Compiled-in grammars and their queries.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use tree_sitter::{Language, Query};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LangId {
    Rust,
    Python,
    TypeScript,
    Tsx,
    JavaScript,
    Go,
    C,
    Cpp,
    Ruby,
    Bash,
}

pub struct Lang {
    pub id: LangId,
    pub name: &'static str,
    pub language: Language,
    pub comment_kinds: &'static [&'static str],
    pub decls: Option<Query>,
    pub strings: Option<Query>,
}

#[derive(Default)]
pub struct Registry {
    langs: Vec<Lang>,
    by_ext: HashMap<&'static str, usize>,
}

impl Registry {
    pub fn new() -> Result<Registry> {
        let mut reg = Registry::default();

        #[cfg(feature = "lang-rust")]
        reg.add(
            LangId::Rust,
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            &["line_comment", "block_comment"],
            include_str!("../../queries/rust/decls.scm"),
            include_str!("../../queries/rust/strings.scm"),
            &["rs"],
        )?;

        #[cfg(feature = "lang-python")]
        reg.add(
            LangId::Python,
            "python",
            tree_sitter_python::LANGUAGE.into(),
            &["comment"],
            include_str!("../../queries/python/decls.scm"),
            include_str!("../../queries/python/strings.scm"),
            &["py", "pyi"],
        )?;

        #[cfg(feature = "lang-typescript")]
        {
            reg.add(
                LangId::TypeScript,
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                &["comment"],
                include_str!("../../queries/typescript/decls.scm"),
                include_str!("../../queries/typescript/strings.scm"),
                &["ts", "mts", "cts"],
            )?;
            reg.add(
                LangId::Tsx,
                "tsx",
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                &["comment"],
                include_str!("../../queries/typescript/decls.scm"),
                include_str!("../../queries/tsx/strings.scm"),
                &["tsx"],
            )?;
        }

        #[cfg(feature = "lang-javascript")]
        reg.add(
            LangId::JavaScript,
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            &["comment"],
            include_str!("../../queries/javascript/decls.scm"),
            include_str!("../../queries/javascript/strings.scm"),
            &["js", "mjs", "cjs", "jsx"],
        )?;

        #[cfg(feature = "lang-go")]
        reg.add(
            LangId::Go,
            "go",
            tree_sitter_go::LANGUAGE.into(),
            &["comment"],
            include_str!("../../queries/go/decls.scm"),
            include_str!("../../queries/go/strings.scm"),
            &["go"],
        )?;

        #[cfg(feature = "lang-c")]
        reg.add(
            LangId::C,
            "c",
            tree_sitter_c::LANGUAGE.into(),
            &["comment"],
            include_str!("../../queries/c/decls.scm"),
            include_str!("../../queries/c/strings.scm"),
            &["c", "h"],
        )?;

        #[cfg(feature = "lang-cpp")]
        reg.add(
            LangId::Cpp,
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            &["comment"],
            include_str!("../../queries/cpp/decls.scm"),
            include_str!("../../queries/cpp/strings.scm"),
            &["cc", "cpp", "cxx", "hpp", "hh", "hxx", "ipp"],
        )?;

        #[cfg(feature = "lang-ruby")]
        reg.add(
            LangId::Ruby,
            "ruby",
            tree_sitter_ruby::LANGUAGE.into(),
            &["comment"],
            include_str!("../../queries/ruby/decls.scm"),
            include_str!("../../queries/ruby/strings.scm"),
            &["rb", "rake", "gemspec"],
        )?;

        #[cfg(feature = "lang-bash")]
        reg.add(
            LangId::Bash,
            "bash",
            tree_sitter_bash::LANGUAGE.into(),
            &["comment"],
            include_str!("../../queries/bash/decls.scm"),
            include_str!("../../queries/bash/strings.scm"),
            &["sh", "bash", "zsh"],
        )?;

        Ok(reg)
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        id: LangId,
        name: &'static str,
        language: Language,
        comment_kinds: &'static [&'static str],
        decls: &str,
        strings: &str,
        exts: &[&'static str],
    ) -> Result<()> {
        let compile = |src: &str, what: &str| -> Result<Option<Query>> {
            if src.trim().is_empty() {
                return Ok(None);
            }
            Query::new(&language, src)
                .map(Some)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "{name} {what}: row {} col {}: {}",
                        e.row + 1,
                        e.column + 1,
                        e.message
                    )
                })
                .with_context(|| format!("compiling {name} {what} query"))
        };
        let decls = compile(decls, "decls.scm")?;
        let strings = compile(strings, "strings.scm")?;
        let index = self.langs.len();
        self.langs.push(Lang {
            id,
            name,
            language,
            comment_kinds,
            decls,
            strings,
        });
        for ext in exts {
            self.by_ext.insert(ext, index);
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.langs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.langs.is_empty()
    }

    /// Picks a grammar by extension, or by shebang for files without one.
    pub fn for_path(&self, path: &Path) -> Option<&Lang> {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext = ext.to_ascii_lowercase();
            return self.by_ext.get(ext.as_str()).map(|&i| &self.langs[i]);
        }
        let mut head = [0u8; 160];
        let n = File::open(path).ok()?.read(&mut head).ok()?;
        let head = String::from_utf8_lossy(&head[..n]);
        let first = head.lines().next()?;
        if !first.starts_with("#!") {
            return None;
        }
        if ["bash", "/sh", "zsh", "env sh", "dash", "ksh"]
            .iter()
            .any(|s| first.contains(s))
        {
            return self.by_ext.get("sh").map(|&i| &self.langs[i]);
        }
        if first.contains("python") {
            return self.by_ext.get("py").map(|&i| &self.langs[i]);
        }
        if first.contains("ruby") {
            return self.by_ext.get("rb").map(|&i| &self.langs[i]);
        }
        if first.contains("node") || first.contains("deno") || first.contains("bun") {
            return self.by_ext.get("js").map(|&i| &self.langs[i]);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_queries_compile() {
        let reg = Registry::new().expect("every bundled query compiles");
        assert!(!reg.is_empty());
        assert!(reg.for_path(Path::new("x.rs")).is_some());
    }
}
