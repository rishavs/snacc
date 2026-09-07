//! Repository file-size guard (Specification 029 section 5, Phase 7).
//!
//! A production file over 500 lines is acceptable only when it is one cohesive
//! unit whose split would require inventing an abstraction. Such a file is
//! listed explicitly in [`EXCEPTIONS`] with its reason; every other
//! production `.rs` file must stay within the soft 500-line target. Adding an
//! exception is a visible, reviewed change: record the path with a reason
//! here, or split the file instead.

use std::path::{Path, PathBuf};

/// Workspace-relative paths (with `/` separators) that may exceed 500 lines,
/// each with the reason splitting it further would hurt readability.
/// Remove an entry once its file is split below the target.
const EXCEPTIONS: &[(&str, &str)] = &[
    (
        "crates/snacc-compiler/src/checker/mod.rs",
        "checker program walk and shared types as one unit; tests live in checker/tests.rs (Specification 029 Phase 2)",
    ),
    (
        "crates/snacc-compiler/src/llvm/imports.rs",
        "runtime import declarations as one table-driven unit (Specification 029 Phases 3-4)",
    ),
    (
        "crates/snacc-compiler/src/llvm/lower/mod.rs",
        "Codegen struct, call lowering, and module construction as one unit (Specification 029 Phase 3)",
    ),
    (
        "crates/snacc-compiler/src/llvm/lower/stmt.rs",
        "statement and block lowering as one unit (Specification 029 Phase 3)",
    ),
    (
        "crates/snacc-compiler/src/llvm/lower/equality.rs",
        "structural equality and return_on_error lowering as one unit (Specification 029 Phase 3)",
    ),
    (
        "crates/snacc-compiler/src/llvm/lower/collections.rs",
        "list, map, and set operation lowering as one unit (Specification 029 Phase 3)",
    ),
    (
        "crates/snacc-compiler/src/llvm/lower/cleanup.rs",
        "drop and cleanup lowering as one unit, 0.2% over target (Specification 029 Phase 3)",
    ),
    (
        "crates/snacc-compiler/src/llvm/lower/expr.rs",
        "expression lowering as one unit (Specification 029 Phase 3)",
    ),
    (
        "crates/snacc-runtime/src/map.rs",
        "map runtime as one unit: four monomorphization families sharing store discipline (Specification 029 section 5)",
    ),
    (
        "crates/snacc-runtime/src/set.rs",
        "set runtime as one unit, 1% over target (Specification 029 section 5)",
    ),
    (
        "crates/snacc-runtime/src/lib.rs",
        "runtime ABI vocabulary plus the symbol-set golden test, 2% over target (Specification 029 section 5)",
    ),
    (
        "crates/snacc-compiler/src/types.rs",
        "one cohesive type table with resolution rules (Specification 029 section 5 exception, currently larger pending further split)",
    ),
    (
        "crates/snacc-compiler/src/lexer/mod.rs",
        "token definitions, scanning, numeric, and text rules share lexer state (Specification 029 section 5)",
    ),
    (
        "crates/snacc-compiler/src/ast.rs",
        "cohesive AST definition, 2% over target, left alone per Specification 029 section 2",
    ),
    (
        "crates/snacc-compiler/src/checker/places.rs",
        "one cohesive borrow/move/place analysis unit; splitting further would invent an abstraction (Specification 029 section 5)",
    ),
    (
        "crates/snacc-compiler/src/checker/program.rs",
        "declaration collection, signatures, and generics as one unit (Specification 029 Phase 2)",
    ),
    (
        "crates/snacc-compiler/src/checker/stmt.rs",
        "statements, blocks, and control flow as one unit (Specification 029 Phase 2)",
    ),
    (
        "crates/snacc-compiler/src/checker/calls.rs",
        "calls, methods, constructors, and bridge arguments as one unit (Specification 029 Phase 2)",
    ),
    (
        "crates/snacc-compiler/src/checker/expr.rs",
        "expression checking as one unit (Specification 029 Phase 2)",
    ),
];

fn is_test_only(path: &Path) -> bool {
    if path
        .components()
        .any(|component| component.as_os_str() == "tests")
    {
        return true;
    }
    match path.file_name().and_then(|name| name.to_str()) {
        Some("tests.rs") => true,
        Some(name) => name.ends_with("_tests.rs"),
        None => false,
    }
}

fn collect_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = entry.path();
            if path.is_dir() {
                let skip = match entry.file_name().to_str() {
                    Some("target") | Some(".git") => true,
                    _ => false,
                };
                if !skip {
                    stack.push(path);
                }
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}

#[test]
fn production_files_stay_within_the_soft_line_target() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("..").join("..");
    let mut files = Vec::new();
    for top in ["crates", "apps"] {
        collect_rs_files(&root.join(top), &mut files);
    }
    let mut offenders = Vec::new();
    for path in &files {
        if is_test_only(path) {
            continue;
        }
        let relative = match path.strip_prefix(&root) {
            Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let lines = content.lines().count();
        let excused = EXCEPTIONS.iter().any(|(path, _)| *path == relative);
        if lines > 500 && !excused {
            offenders.push(format!("{relative} ({lines} lines)"));
        }
    }
    assert!(
        offenders.is_empty(),
        "production files exceed the 500-line soft target without a recorded exception \
         (Specification 029 section 5):\n  {}\nSplit the file or record its path with a \
         reason in EXCEPTIONS in crates/snacc-compiler/tests/file_size_guard.rs.",
        offenders.join("\n  ")
    );
}
