//! Walker over the JSONTestSuite corpus (submodule at tests/vendor/JSONTestSuite).
//!
//! - `y_*` files: must parse in both EAGER and LAZY modes.
//! - `n_*` files: must fail to parse in EAGER mode.
//!   In LAZY mode the file MAY parse (structural-only) but a value-level
//!   access of the malformed field would fail; we do not assert against
//!   LAZY here.
//! - `i_*` files: implementation-defined; we record our behavior (no
//!   assertions). The list of accepted/rejected i_* cases is printed at
//!   the end of the test run for documentation.
//!
//! # Known failures
//!
//! Files listed in KNOWN_Y_FAILURES / KNOWN_N_FAILURES are skipped with a
//! logged explanation.  Removing a file from these lists re-enables the test.
//!
//! KNOWN_Y_FAILURES: y_* files we don't handle correctly yet.
//!   Each entry documents why; follow-up issues are referenced in comments.
//!
//! KNOWN_N_FAILURES: n_* files our eager validator passes when it shouldn't.
//!   These correspond to grammar-aware gaps deferred to issue #37.

use std::fs;
use std::path::Path;

use quickdecode::doc::Document;
use quickdecode::options::{Options, QJD_MODE_EAGER, QJD_MODE_LAZY};

/// y_* files that we currently reject but shouldn't.
/// Each is annotated with why and what follow-up would fix it.
const KNOWN_Y_FAILURES: &[&str] = &[
    // "y_string_utf8.json" — example placeholder (none currently needed)
];

/// n_* files that we currently accept but shouldn't (validator gap).
///
/// The grammar-aware eager pass in src/validate/mod.rs tracks parser
/// state per container and rejects token transitions that violate
/// RFC 8259.  Removing a file from this list re-enables the assertion.
const KNOWN_N_FAILURES: &[&str] = &[
    // (intentionally empty — see git history for the previous list,
    // which was closed by the grammar-aware structural pass.)
];

fn corpus_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn parsing_dir() -> std::path::PathBuf {
    corpus_dir().join("tests/vendor/JSONTestSuite/test_parsing")
}

fn iter_files(prefix: &str) -> Vec<std::path::PathBuf> {
    let dir = parsing_dir();
    let entries = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!(
            "missing JSONTestSuite submodule at {:?}: {} — run: git submodule update --init",
            dir, e
        ));
    let mut paths: Vec<_> = entries
        .filter_map(|r| r.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|s| s.to_str()) == Some("json")
                && p.file_name()
                    .and_then(|s| s.to_str())
                    .map(|n| n.starts_with(prefix))
                    .unwrap_or(false)
        })
        .collect();
    paths.sort();
    paths
}

fn is_known_y_failure(path: &std::path::Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    KNOWN_Y_FAILURES.contains(&name)
}

fn is_known_n_failure(path: &std::path::Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    KNOWN_N_FAILURES.contains(&name)
}

#[test]
fn y_files_accepted_in_both_modes() {
    let eager = Options { mode: QJD_MODE_EAGER, max_depth: 0 };
    let lazy  = Options { mode: QJD_MODE_LAZY,  max_depth: 0 };
    let mut failures = Vec::new();
    let mut skipped = 0usize;

    for path in iter_files("y_") {
        if is_known_y_failure(&path) {
            eprintln!("SKIP (known-y-failure): {:?}", path.file_name().unwrap());
            skipped += 1;
            continue;
        }
        let data = fs::read(&path).unwrap();
        let r_e = Document::parse_with_options(&data, &eager);
        let r_l = Document::parse_with_options(&data, &lazy);
        if r_e.is_err() || r_l.is_err() {
            failures.push((
                path.file_name().unwrap().to_owned(),
                format!("eager={:?} lazy={:?}", r_e.err(), r_l.err()),
            ));
        }
    }

    if skipped > 0 {
        eprintln!("y_* skipped (known failures): {}", skipped);
    }
    if !failures.is_empty() {
        for (n, e) in &failures {
            eprintln!("UNEXPECTED REJECT: {:?} → {}", n, e);
        }
        panic!("{} y_* file(s) unexpectedly rejected", failures.len());
    }
}

#[test]
fn n_files_rejected_in_eager_mode() {
    let eager = Options { mode: QJD_MODE_EAGER, max_depth: 0 };
    let mut accepted = Vec::new();
    let mut skipped = 0usize;

    for path in iter_files("n_") {
        if is_known_n_failure(&path) {
            eprintln!("SKIP (known-n-failure): {:?}", path.file_name().unwrap());
            skipped += 1;
            continue;
        }
        let data = fs::read(&path).unwrap();
        if Document::parse_with_options(&data, &eager).is_ok() {
            accepted.push(path.file_name().unwrap().to_owned());
        }
    }

    if skipped > 0 {
        eprintln!("n_* skipped (known failures): {}", skipped);
    }
    if !accepted.is_empty() {
        for n in &accepted {
            eprintln!("UNEXPECTED ACCEPT: {:?}", n);
        }
        panic!("{} n_* file(s) unexpectedly accepted", accepted.len());
    }
}

#[test]
fn document_i_files_behavior() {
    // Implementation-defined cases — document what we do, do not assert.
    let eager = Options { mode: QJD_MODE_EAGER, max_depth: 0 };
    for path in iter_files("i_") {
        let data = fs::read(&path).unwrap();
        let verdict = match Document::parse_with_options(&data, &eager) {
            Ok(_)  => "ACCEPT".to_owned(),
            Err(e) => format!("REJECT({:?})", e),
        };
        eprintln!("i_* {:?} → {}", path.file_name().unwrap(), verdict);
    }
}
