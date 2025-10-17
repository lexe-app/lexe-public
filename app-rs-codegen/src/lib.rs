//! Runs `flutter_rust_bridge` codegen on the `app-rs` crate.
//!
//! We previously ran this logic in an `app-rs/build.rs` build script, but
//! several issues with both `flutter_rust_bridge` and `flutter` itself made
//! this just too hacky to rely on.
//!
//! `flutter_rust_bridge_codegen` has an unclean build process which just
//! modifies files all over the place; this doesn't fit well with `build.rs`,
//! which expects build scripts to only touch files in `$OUT_DIR`.
//!
//! The native FFI build integration in `flutter` itself is also incomplete and
//! currently in-flux. See [dart-lang/sdk - vm/ffi: native assets feature #50565](https://github.com/dart-lang/sdk/issues/50565)
//! for the current status/roadmap for this feature.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, format_err};
use argh::FromArgs;
use lib_flutter_rust_bridge_codegen as frb;

/// Generates the Rust and Dart FFI interface files for the `app-rs` crate.
#[derive(FromArgs)]
pub struct Args {
    /// run codegen in check mode. If the generated files don't match the
    /// checked-in versions, the tool will return an error. Beware that this
    /// still modifies the files.
    #[argh(switch)]
    pub check: bool,
}

impl Args {
    pub fn run(self) -> anyhow::Result<()> {
        let app_rs_dir = find_app_rs_dir().ok_or_else(|| {
            format_err!(
                "failed to find app-rs directory. Try running in the base \
                 directory of the repo."
            )
        })?;
        let workspace_dir = app_rs_dir.parent().unwrap();
        let app_rs_dart_dir = workspace_dir.join("app_rs_dart");

        let frb_generated_rs = app_rs_dir.join("src/frb_generated.rs");
        let app_rs_dart_lib_dir = app_rs_dart_dir.join("lib");

        // flutter_rust_bridge options
        // Docs: [`GenerateCommandArgsPrimary`](https://github.com/fzyzcjy/flutter_rust_bridge/blob/master/frb_codegen/src/binary/commands.rs#L52)
        let config = frb::codegen::Config {
            // verbose: true,
            // dump: Some(vec![frb::codegen::ConfigDumpContent::Config]),
            // dump_all: Some(true),

            // The Rust crate root dir.
            rust_root: Some(path_to_string(&app_rs_dir)?),
            // The Dart package root dir.
            dart_root: Some(path_to_string(&app_rs_dart_dir)?),

            // The comma-separated list of input Rust modules to generate Dart
            // interfaces for.
            //
            // TODO(phlip9): apparently this now accepts third-party crates?
            // Will have to experiment.
            rust_input: Some([
                // Generate Dart interfaces for all Rust modules in the
                // `app-rs/src/ffi` subdir.
                "crate::ffi",
            ].join(",")),

            // Path to output generated Rust code.
            rust_output: Some(path_to_string(&frb_generated_rs)?),

            // Path to output generated Dart code impls.
            dart_output: Some(path_to_string(&app_rs_dart_lib_dir)?),

            // The class name of the main entrypoint to the Rust API.
            // Defaults to "RustLib".
            dart_entrypoint_class_name: Some("AppRs".to_owned()),
            // Disable some lints in the generated code
            dart_preamble: Some(r#"

//
// From: `dart_preamble` in `app-rs-codegen/src/lib.rs`
// ignore_for_file: invalid_internal_annotation, always_use_package_imports, directives_ordering, prefer_const_constructors, sort_unnamed_constructors_first
//

"#.to_owned()),

            // Setting this to `true` appears to make frb generate code similar
            // to v1? It works better with static linking, so I'll keep this set
            full_dep: Some(false),
            // When `false`, appears to box u64, i64 and usize, as they're not
            // representable in dart.
            // `type_64bit_int=true` looks broken when used with `full_dep=true`
            type_64bit_int: Some(true),

            // While we're using a custom nightly rustfmt, don't need to
            // double-format.
            rust_format: Some(false),

            // Other options
            dart3: Some(true),
            dart_format_line_length: Some(80),
            add_mod_to_lib: Some(false),
            web: Some(false),
            enable_lifetime: Some(false),
            ..Default::default()
        };
        let meta_config = frb::codegen::MetaConfig { watch: false };

        // generate dart and rust ffi bindings
        frb::codegen::generate(config, meta_config).context(
            "flutter_rust_bridge: failed to generate Rust+Dart ffi bindings ",
        ).unwrap();

        // run `rustfmt` with our nightly version
        Command::new("rustup")
            .args(["run", "nightly-2025-10-16", "cargo", "fmt", "--all"])
            .status()
            .context("Failed to run `cargo fmt` with nightly compiler")?;

        // Maybe update `app_rs_dart/build_rust_ios_macos.input.xcfilelist`.
        // This file is used in `app_rs_dart`'s CocoaPods/Xcode integration.
        let xcfilelist_path =
            app_rs_dart_dir.join("build_rust_ios_macos.input.xcfilelist");
        let xcfilelist = build_app_rs_xcfilelist(workspace_dir);
        update_app_rs_xcfilelist_if_changed(&xcfilelist_path, xcfilelist);

        // run `git diff --exit-code` to see if any files
        // changed
        if self.check {
            let status = Command::new("git")
                .args(["diff", "--exit-code"])
                .current_dir(workspace_dir)
                .status()
                .context(
                    "Failed to run `git diff` on generated ffi bindings",
                )?;

            if !status.success() {
                return Err(format_err!(
                    "generated ffi bindings are not up-to-date"
                ));
            }
        }

        Ok(())
    }
}

fn find_app_rs_dir() -> Option<PathBuf> {
    let candidates = ["app-rs/Cargo.toml", "public/app-rs/Cargo.toml"];
    for candidate in candidates {
        let path = Path::new(candidate);
        if path.is_file() {
            // Get the absolute path of the parent
            return path.parent()?.canonicalize().ok();
        }
    }

    None
}

fn path_to_string(path: &Path) -> anyhow::Result<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        format_err!("path is not valid UTF-8: '{}'", path.display())
    })
}

/// Generate workspace input xcfilelist for
/// `app_rs_dart/{ios,macos}/app_rs_dart.podspec`. This is a list
/// of all source file dependencies not covered by the
/// cargo-generated libapp_rs.d dep file.
fn build_app_rs_xcfilelist(workspace_dir: &Path) -> String {
    let walk = ignore::WalkBuilder::new(workspace_dir)
        .hidden(false)
        .follow_links(false)
        .max_depth(Some(2))
        .build();
    const PREFIX: &str = "${PODS_TARGET_SRCROOT}/../../";
    let mut xcfilelist = walk
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?;
            if file_name.ends_with(".toml") {
                let rel_path = path.strip_prefix(workspace_dir).unwrap();
                let pod_rel_path = format!("{PREFIX}{}", rel_path.display());
                Some(pod_rel_path)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    xcfilelist.push(format!("{PREFIX}Cargo.lock"));
    xcfilelist.sort_unstable();

    let mut buf = xcfilelist.join("\n");
    buf.push('\n');
    buf
}

/// Only write the new xcfilelist if it's actually changed. This avoids changing
/// the file modified time if there are no changes.
fn update_app_rs_xcfilelist_if_changed(
    xcfilelist_path: &Path,
    updated: String,
) {
    let existing = std::fs::read_to_string(xcfilelist_path).unwrap();
    if updated != existing {
        eprintln!("Updating '{}'", xcfilelist_path.display());
        std::fs::write(xcfilelist_path, updated).unwrap();
    }
}
