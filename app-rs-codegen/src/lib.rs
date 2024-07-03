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

use std::{path::Path, process::Command};

use anyhow::{format_err, Context};
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

fn find_app_rs_dir() -> Option<&'static Path> {
    let candidates = ["app-rs/Cargo.toml", "public/app-rs/Cargo.toml"];
    for candidate in candidates {
        let path = Path::new(candidate);
        if path.is_file() {
            return path.parent();
        }
    }

    None
}

fn path_to_string<P: AsRef<Path>>(path: P) -> anyhow::Result<String> {
    let path = path.as_ref();
    path.to_str().map(str::to_owned).ok_or_else(|| {
        format_err!("path is not valid UTF-8: '{}'", path.display())
    })
}

impl Args {
    pub fn run(self) -> anyhow::Result<()> {
        let app_rs_dir = find_app_rs_dir().ok_or_else(|| {
            format_err!(
                "failed to find app-rs directory. Try running in the base \
                 directory of the repo."
            )
        })?;
        let app_dir = app_rs_dir.parent().unwrap().join("app");

        let ffi_generated_rs = app_rs_dir.join("src/ffi/ffi_generated.rs");
        let ffi_generated_dart = app_dir.join("lib/app_rs");
        let ios_ffi_generated_h = app_dir.join("ios/Runner/ffi_generated.h");
        let macos_ffi_generated_h =
            app_dir.join("macos/Runner/ffi_generated.h");

        // flutter_rust_bridge options
        // Docs: [`GenerateCommandArgsPrimary`](https://github.com/fzyzcjy/flutter_rust_bridge/blob/master/frb_codegen/src/binary/commands.rs#L52)
        let config = frb::codegen::Config {
            // verbose: true,
            // dump: Some(vec![frb::codegen::ConfigDumpContent::Config]),
            // dump_all: Some(true),

            // The Rust crate root dir.
            rust_root: Some(path_to_string(app_rs_dir)?),
            // The Dart package root dir.
            dart_root: Some(path_to_string(app_dir)?),

            // The comma-separated list of input Rust modules to generate Dart
            // interfaces for.
            //
            // TODO(phlip9): apparently this now accepts third-party crates?
            // Will have to experiment.
            rust_input: Some(["crate::ffi::ffi"].join(",")),

            // Path to output generated Rust code.
            rust_output: Some(path_to_string(&ffi_generated_rs)?),

            // Path to output generated Dart code impls.
            dart_output: Some(path_to_string(&ffi_generated_dart)?),

            // // Path to output generated Dart API declarations (decls only, no
            // // impls) so you can easily read what APIs are available
            // // from the Dart side.
            // dart_decl_output: Some(path_to_string(&ffi_generated_api_dart)?),

            // These steps dump headers with all the emitted ffi symbols. We
            // also reference these symbols from a dummy method so
            // they don't get stripped by the over-aggressive
            // iOS/macOS symbol stripper.
            c_output: Some(path_to_string(&ios_ffi_generated_h)?),
            duplicated_c_output: Some(vec![path_to_string(
                &macos_ffi_generated_h,
            )?]),

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

            // Other options
            dart3: Some(true),
            dart_format_line_length: Some(80),
            full_dep: Some(false), // What does this do?
            add_mod_to_lib: Some(false),
            web: Some(false),
            enable_lifetime: Some(false),
            type_64bit_int: Some(true),
            ..Default::default()
        };
        let meta_config = frb::codegen::MetaConfig { watch: false };

        // generate dart and rust ffi bindings
        frb::codegen::generate(config, meta_config).context(
            "flutter_rust_bridge: failed to generate Rust+Dart ffi bindings ",
        ).unwrap();

        // run `git diff --exit-code <maybe-changed-files>` to see if any files
        // changed
        if self.check {
            let mut cmd = Command::new("git");
            cmd.args(["diff", "--exit-code"]).args([
                // TODO(phlip9): update
                &ffi_generated_rs,
                &ffi_generated_dart,
                // &ffi_generated_api_dart,
                &ios_ffi_generated_h,
                &macos_ffi_generated_h,
            ]);

            let status = cmd.status().context(
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
