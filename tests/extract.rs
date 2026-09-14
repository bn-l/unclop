//! Snapshot tests: what each fixture yields, one line per item.

use std::fs;
use std::path::Path;

use unclop::config::Config;
use unclop::scan::{extract, lang::Registry};

fn rows(fixture: &str) -> String {
    let registry = Registry::new().expect("queries compile");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let lang = registry.for_path(&path).expect("grammar for fixture");
    let src = fs::read_to_string(&path).expect("fixture readable");
    let items = extract(lang, fixture, &src, &Config::default()).expect("extract");
    items
        .iter()
        .map(|i| {
            let scope = i
                .scope
                .as_ref()
                .map(|s| format!("  [in {s}]"))
                .unwrap_or_default();
            let span = if i.line[0] == i.line[1] {
                format!("L{}", i.line[0])
            } else {
                format!("L{}-{}", i.line[0], i.line[1])
            };
            format!("{:<8} {:<8} {}{}", i.kind.label(), span, i.text, scope)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn rust() {
    insta::assert_snapshot!("rust", rows("rust/sample.rs"));
}

#[test]
fn python() {
    insta::assert_snapshot!("python", rows("python/sample.py"));
}

#[test]
fn typescript() {
    insta::assert_snapshot!("typescript", rows("typescript/sample.ts"));
}

#[test]
fn tsx() {
    insta::assert_snapshot!("tsx", rows("tsx/sample.tsx"));
}

#[test]
fn javascript() {
    insta::assert_snapshot!("javascript", rows("javascript/sample.js"));
}

#[test]
fn go() {
    insta::assert_snapshot!("go", rows("go/sample.go"));
}

#[test]
fn c() {
    insta::assert_snapshot!("c", rows("c/sample.c"));
}

#[test]
fn cpp() {
    insta::assert_snapshot!("cpp", rows("cpp/sample.cpp"));
}

#[test]
fn ruby() {
    insta::assert_snapshot!("ruby", rows("ruby/sample.rb"));
}

#[test]
fn bash() {
    insta::assert_snapshot!("bash", rows("bash/sample.sh"));
}

#[test]
fn ids_are_stable_across_runs() {
    let a = rows("rust/sample.rs");
    let b = rows("rust/sample.rs");
    assert_eq!(a, b);
}
