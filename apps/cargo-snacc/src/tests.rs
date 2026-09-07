use super::*;
use crate::{
    bridge::{declares_bridge_assertion_include, render_bridge_assertions},
    cli::{cargo_executable, parse_options, run},
    commands::init::{ensure_cargo_main_template, ensure_new_path},
    metadata::{Package, Selected, SnaccMetadata, Target, package_relative_entry},
};
use snacc_compiler::check;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn selected() -> Selected {
    Selected {
        package: Package {
            id: "path+file:///workspace#app@0.1.0".into(),
            name: "app".into(),
            manifest_path: PathBuf::from("C:/workspace/Cargo.toml"),
            targets: vec![Target {
                name: "app".into(),
                kind: vec!["bin".into()],
                src_path: PathBuf::from("C:/workspace/src/main.rs"),
            }],
            metadata: None,
        },
        package_id: "path+file:///workspace#app@0.1.0".into(),
        package_root: PathBuf::from("C:/workspace"),
        entry: PathBuf::from("C:/workspace/src/main.nrs"),
        host_bin: "app".into(),
        host_src_path: PathBuf::from("C:/workspace/src/main.rs"),
        target_directory: PathBuf::from("C:/workspace/target"),
    }
}

#[test]
fn options_reject_conflicting_profiles() {
    let args = vec!["--release".into(), "--profile".into(), "dev".into()];
    assert!(parse_options(&args).is_err());
}

#[test]
fn cargo_external_subcommand_prefix_is_accepted() {
    let error = run(vec!["snacc".into(), "unknown".into()]).unwrap_err();
    assert_eq!(error.0, "unknown command 'unknown'");
}

#[test]
fn options_preserve_arguments_after_separator() {
    let args = vec!["--offline".into(), "--".into(), "one".into(), "two".into()];
    let parsed = parse_options(&args).unwrap();
    assert!(parsed.options.offline);
    assert_eq!(parsed.trailing, ["one", "two"]);
}

#[test]
fn cargo_artifact_selection_requires_package_target_and_kind() {
    let messages = concat!(
        "{\"reason\":\"compiler-artifact\",\"package_id\":\"other\",\"target\":{\"name\":\"app\",\"kind\":[\"bin\"]},\"executable\":\"wrong.exe\"}\n",
        "{\"reason\":\"compiler-artifact\",\"package_id\":\"path+file:///workspace#app@0.1.0\",\"target\":{\"name\":\"app\",\"kind\":[\"bin\"]},\"executable\":\"right.exe\"}\n"
    );
    assert_eq!(
        cargo_executable(messages, &selected(), "app", "bin"),
        Some(PathBuf::from("right.exe"))
    );
}

/// Builds a `Selected` the way package selection does: from a real
/// directory, with `package_root` and `entry` both canonicalized. On
/// Windows that produces `\\?\C:\...` extended-length paths, which a plain
/// `C:/...` fixture never reproduces.
fn canonical_selected(root: &Path) -> Selected {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.nrs"), "print(0)\n").unwrap();
    fs::write(root.join("Cargo.toml"), "").unwrap();
    let package_root = fs::canonicalize(root).unwrap();
    let entry = fs::canonicalize(package_root.join("src/main.nrs")).unwrap();
    let mut selected = selected();
    // The manifest path stays as Cargo metadata reports it: not canonical.
    selected.package.manifest_path = root.join("Cargo.toml");
    selected.package_root = package_root;
    selected.entry = entry;
    selected
}

#[test]
fn package_relative_entry_strips_a_canonicalized_package_root() {
    let directory = tempfile::tempdir().unwrap();
    let selected = canonical_selected(directory.path());
    assert_eq!(
        package_relative_entry(&selected),
        Path::new("src").join("main.nrs")
    );
}

#[test]
fn rendered_assertions_name_the_entry_relative_to_the_package() {
    let directory = tempfile::tempdir().unwrap();
    let selected = canonical_selected(directory.path());
    let source =
        "extern rust \"snacc_user_double\" fun double(value: Int64): Int64\nprint(double(2))";
    let checked = snacc_compiler::check(source).expect("bridge declaration should check");
    let rendered = render_bridge_assertions(&checked, package_relative_entry(&selected), source);

    assert!(
        rendered.contains("// snacc: double (src\\main.nrs:1:1)")
            || rendered.contains("// snacc: double (src/main.nrs:1:1)"),
        "assertion comment must name the entry relative to the package, got:\n{rendered}"
    );
    // A leaked absolute path pushes the `line:column` suffix past the width
    // rustc renders, which is the whole point of the trailing comment.
    assert!(
        !rendered.contains("\\\\?\\"),
        "assertion comment leaked an extended-length path:\n{rendered}"
    );
}

#[test]
fn declares_bridge_assertion_include_requires_the_exact_pair() {
    assert!(declares_bridge_assertion_include(
        "mod interop;\n\n#[cfg(snacc_bridge_assertions)]\ninclude!(env!(\"SNACC_BRIDGE_ASSERTIONS\"));\n"
    ));
    assert!(!declares_bridge_assertion_include("mod interop;\n"));
    assert!(!declares_bridge_assertion_include(
        "#[cfg(snacc_bridge_assertions)]\nfn unrelated() {}\n"
    ));
}

#[test]
fn selected_test_helper_carries_a_host_src_path() {
    assert_eq!(
        selected().host_src_path,
        PathBuf::from("C:/workspace/src/main.rs")
    );
}

#[test]
fn metadata_schema_rejects_unknown_fields() {
    let result = serde_json::from_value::<SnaccMetadata>(serde_json::json!({
        "schema-version": 1,
        "entry": "src/main.nrs",
        "host-bin": "app",
        "unknown": true
    }));
    assert!(result.is_err());
}

#[test]
fn diagnostic_rendering_preserves_all_messages_and_locations() {
    let diagnostics = Diagnostics {
        items: vec![
            snacc_compiler::Diagnostic {
                phase: snacc_compiler::DiagnosticPhase::TypeCheck,
                message: "first".into(),
                span: Some(0..1),
            },
            snacc_compiler::Diagnostic {
                phase: snacc_compiler::DiagnosticPhase::TypeCheck,
                message: "second".into(),
                span: Some(2..3),
            },
        ],
    };
    let rendered = diagnostic_error(Path::new("main.nrs"), "x\ny", &diagnostics).0;
    assert!(rendered.contains("main.nrs:1:1: TypeCheck error: first"));
    assert!(rendered.contains("main.nrs:2:1: TypeCheck error: second"));
}

#[test]
fn init_preflight_rejects_existing_source_and_custom_host() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("main.nrs");
    fs::write(&source, "print(42)").unwrap();
    assert!(ensure_new_path(&source).is_err());

    let host = directory.path().join("main.rs");
    fs::write(&host, "fn main() { run_custom_host(); }").unwrap();
    assert!(ensure_cargo_main_template(&host).is_err());
    fs::write(&host, "fn main() {\n    println!(\"Hello, world!\");\n}\n").unwrap();
    assert!(ensure_cargo_main_template(&host).is_ok());
}

#[test]
fn init_preflight_recognizes_every_historical_generated_host() {
    let directory = tempfile::tempdir().unwrap();
    let host = directory.path().join("main.rs");
    for template in [
        HOST_MAIN_TEMPLATE_PRE_RFC_007,
        HOST_MAIN_TEMPLATE_PRE_FERRIS,
        HOST_MAIN_TEMPLATE,
    ] {
        fs::write(&host, template).unwrap();
        assert!(
            ensure_cargo_main_template(&host).is_ok(),
            "template should be eligible for reinitialization:\n{template}"
        );
    }
}

#[test]
fn init_preflight_rejects_a_modified_generated_host() {
    let directory = tempfile::tempdir().unwrap();
    let host = directory.path().join("main.rs");
    let modified = HOST_MAIN_TEMPLATE.replace("Hello from a Snacc application!", "edited");
    fs::write(&host, modified).unwrap();
    assert!(ensure_cargo_main_template(&host).is_err());
}

#[test]
fn render_bridge_assertions_sorts_by_snacc_name_and_maps_types() {
    let source = concat!(
        "extern rust \"snacc_user_zeta\" fun zeta(value: Bool)\n",
        "extern rust \"snacc_user_alpha\" fun alpha(a: Int64, b: Float64): Bool\n",
        "print(0)\n"
    );
    let checked = check(source).expect("bridge declarations should type check");
    let rendered = render_bridge_assertions(&checked, Path::new("src/main.nrs"), source);
    let alpha_line = rendered
        .lines()
        .find(|line| line.contains("snacc_user_alpha"))
        .expect("alpha assertion line");
    let zeta_line = rendered
        .lines()
        .find(|line| line.contains("snacc_user_zeta"))
        .expect("zeta assertion line");
    assert!(rendered.find(alpha_line).unwrap() < rendered.find(zeta_line).unwrap());
    assert!(alpha_line.contains("fn(i64, f64) -> u8"));
    assert!(alpha_line.contains("crate::interop::snacc_user_alpha"));
    assert!(alpha_line.contains("// snacc: alpha (src/main.nrs:2:1)"));
    assert!(zeta_line.contains("fn(u8) -> ()"));
    assert!(zeta_line.contains("// snacc: zeta (src/main.nrs:1:1)"));
}

/// Specification 009 section 5.2's exact bridge/ABI mapping, checked
/// against the pure rendering function so this regresses without needing
/// a full LLVM/cargo build (see the slower, real-link coverage in
/// `apps/cargo-snacc/tests/cargo_hosted.rs`).
#[test]
fn render_bridge_assertions_maps_every_new_scalar_type() {
    let source = concat!(
        "extern rust \"snacc_user_u8\" fun echo_u8(value: Byte): Byte\n",
        "extern rust \"snacc_user_u16\" fun echo_u16(value: UInt16): UInt16\n",
        "extern rust \"snacc_user_u32\" fun echo_u32(value: UInt32): UInt32\n",
        "extern rust \"snacc_user_u64\" fun echo_u64(value: UInt64): UInt64\n",
        "extern rust \"snacc_user_f32\" fun echo_f32(value: Float32): Float32\n",
        "print(0)\n"
    );
    let checked = check(source).expect("bridge declarations should type check");
    let rendered = render_bridge_assertions(&checked, Path::new("src/main.nrs"), source);
    for (symbol, mapping) in [
        ("snacc_user_u8", "fn(u8) -> u8"),
        ("snacc_user_u16", "fn(u16) -> u16"),
        ("snacc_user_u32", "fn(u32) -> u32"),
        ("snacc_user_u64", "fn(u64) -> u64"),
        ("snacc_user_f32", "fn(f32) -> f32"),
    ] {
        let line = rendered
            .lines()
            .find(|line| line.contains(symbol))
            .unwrap_or_else(|| panic!("assertion line for {symbol} was not rendered"));
        assert!(
            line.contains(mapping),
            "{symbol} should render '{mapping}', got: {line}"
        );
    }
}

#[test]
fn render_bridge_assertions_with_no_externs_still_checks_the_abi_version() {
    let source = "print(0)\n";
    let checked = check(source).expect("source should type check");
    let rendered = render_bridge_assertions(&checked, Path::new("src/main.nrs"), source);
    assert!(rendered.starts_with("// Generated by cargo-snacc"));
    assert!(rendered.contains(&format!(
        "const _: () = assert!(snacc_runtime::ABI_VERSION == {}, \"snacc compiler/runtime ABI version mismatch\");",
        snacc_compiler::ABI_VERSION
    )));
    assert!(!rendered.contains("crate::interop::"));
}

#[test]
fn render_bridge_assertions_is_deterministic() {
    let source = "extern rust \"snacc_user_a\" fun a(): Int64\nprint(0)\n";
    let checked = check(source).expect("source should type check");
    let first = render_bridge_assertions(&checked, Path::new("src/main.nrs"), source);
    let second = render_bridge_assertions(&checked, Path::new("src/main.nrs"), source);
    assert_eq!(first, second);
}
