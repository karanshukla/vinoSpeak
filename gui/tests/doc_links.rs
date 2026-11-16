//! Every `` `tests::name` `` a comment cites must be a `#[test]` fn, so a rule
//! that names its test fails CI when the test is renamed or deleted. rustdoc
//! can't check these: `#[test]` fns are compiled out of the docs. See
//! docs/comment-style.md.

use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn every_cited_test_exists() {
    let mut broken = Vec::new();
    for file in rust_files(Path::new("src")) {
        let text = fs::read_to_string(&file).unwrap();
        for (line_no, line) in text.lines().enumerate() {
            if !line.trim_start().starts_with("//") {
                continue;
            }
            for cited in cited_tests(line) {
                if !resolves(&file, &text, cited) {
                    broken.push(format!("{}:{}: `{cited}`", file.display(), line_no + 1));
                }
            }
        }
    }
    assert!(broken.is_empty(), "no such test:\n{}", broken.join("\n"));
}

#[test]
fn the_check_finds_a_cited_test_and_rejects_a_missing_one() {
    let text = "/// (pinned by `tests::real`)\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn real() {}\n    fn helper() {}\n}\n";
    let file = Path::new("src/nowhere.rs");
    assert_eq!(cited_tests(text.lines().next().unwrap()), ["tests::real"]);
    assert!(resolves(file, text, "tests::real"));
    assert!(!resolves(file, text, "tests::helper"), "not a #[test]");
    assert!(!resolves(file, text, "tests::gone"));
}

#[test]
fn a_module_with_its_tests_in_a_file_of_their_own_resolves() {
    let cited = "app::tests::the_window_buttons_sit_in_the_top_right_corner";
    assert!(resolves(Path::new("src/main.rs"), "", cited));
    assert!(resolves(
        Path::new("src/app.rs"),
        "",
        "tests::the_window_buttons_sit_in_the_top_right_corner"
    ));
    assert!(!resolves(Path::new("src/main.rs"), "", "app::tests::gone"));
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files
}

/// Code spans like `` `tests::name` `` or `` `module::tests::name` ``.
fn cited_tests(line: &str) -> Vec<&str> {
    line.split('`')
        .skip(1)
        .step_by(2)
        .filter(|span| {
            span.contains("tests::")
                && span
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
        })
        .collect()
}

/// `tests::name` is in `text` (the file at `file`) or in its `<stem>/tests.rs`;
/// `module::tests::name` is in `src/module.rs` or `src/module/tests.rs`.
fn resolves(file: &Path, text: &str, cited: &str) -> bool {
    let (module, name) = cited.rsplit_once("tests::").unwrap();
    let module = module.trim_end_matches("::");
    let base = if module.is_empty() {
        if is_test_fn(text, name) {
            return true;
        }
        file.with_extension("")
    } else {
        Path::new("src").join(module.replace("::", "/"))
    };
    [base.with_extension("rs"), base.join("tests.rs")]
        .iter()
        .any(|path| fs::read_to_string(path).is_ok_and(|text| is_test_fn(&text, name)))
}

/// A `fn name(` whose attributes, above it, include `#[test]`.
fn is_test_fn(text: &str, name: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let signature = format!("fn {name}(");
    lines.iter().enumerate().any(|(i, line)| {
        line.trim_start().starts_with(&signature)
            && lines[..i]
                .iter()
                .rev()
                .map(|l| l.trim())
                .take_while(|l| l.starts_with("#[") || l.starts_with("///"))
                .any(|l| l == "#[test]")
    })
}
