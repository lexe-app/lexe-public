//! Runs the `flutter_rust_bridge` codegen on the `app-rs` crate.
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

        // dbg!(app_rs_dir.display());
        // dbg!(app_dir.display());

        let bindings_rs = app_rs_dir.join("src/bindings.rs");
        let bindings_generated_rs =
            app_rs_dir.join("src/bindings_generated.rs");
        let bindings_generated_dart =
            app_dir.join("lib/bindings_generated.dart");
        let bindings_generated_api_dart =
            app_dir.join("lib/bindings_generated_api.dart");
        let ios_bindings_generated_h =
            app_dir.join("ios/Runner/bindings_generated.h");
        let macos_path = app_dir.join("macos/Runner/");
        let macos_bindings_generated_h =
            app_dir.join("macos/Runner/bindings_generated.h");

        // dbg!(bindings_rs.display());
        // dbg!(bindings_generated_rs.display());
        // dbg!(bindings_generated_dart.display());
        // dbg!(bindings_generated_api_dart.display());
        // dbg!(ios_bindings_generated_h.display());
        // dbg!(macos_path.display());

        // flutter_rust_bridge options
        let configs = frb::config_parse(frb::RawOpts {
            verbose: true,

            // Path of input Rust code
            rust_input: vec![path_to_string(&bindings_rs)?],
            // Path to output generated Rust code.
            rust_output: Some(vec![path_to_string(&bindings_generated_rs)?]),

            // Path to output generated Dart code impls.
            dart_output: vec![path_to_string(&bindings_generated_dart)?],
            // Path to output generated Dart API declarations (decls only, no
            // impls) so you can easily read what APIs are available
            // from the Dart side.
            dart_decl_output: Some(path_to_string(
                &bindings_generated_api_dart,
            )?),

            // These steps dump headers with all the emitted ffi symbols. We
            // also reference these symbols from a dummy method so
            // they don't get stripped by the over-aggressive
            // iOS/macOS symbol stripper.
            c_output: Some(vec![path_to_string(&ios_bindings_generated_h)?]),
            extra_c_output_path: Some(vec![path_to_string(macos_path)?]),

            // Other options
            wasm: false,
            dart_format_line_length: 80,
            skip_add_mod_to_lib: true,
            inline_rust: true,
            ..Default::default()
        });

        // read Rust symbols from `src/bindings.rs`.
        let all_symbols = frb::get_symbols_if_no_duplicates(&configs)
            .with_context(|| {
                format!(
                    "flutter_rust_bridge: failed to read Rust symbols from '{}'",
                    bindings_rs.display(),
                )
            })?;
        // actually generate dart and rust ffi bindings.
        for config in configs.iter() {
            frb::frb_codegen(config, &all_symbols).context(
                "flutter_rust_bridge: failed to generate Rust+Dart ffi bindings",
            )?;
        }

        // run `git diff --exit-code <maybe-changed-files>` to see if any files
        // changed
        if self.check {
            let mut cmd = Command::new("git");
            cmd.args(["diff", "--exit-code"]).args([
                &bindings_generated_rs,
                &bindings_generated_dart,
                &bindings_generated_api_dart,
                &ios_bindings_generated_h,
                &macos_bindings_generated_h,
            ]);

            // dbg!(&cmd);

            let status = cmd
                .status()
                .context("Failed to run `git diff` on generated bindings")?;

            if !status.success() {
                return Err(format_err!(
                    "generated bindings are not up-to-date"
                ));
            }
        }

        Ok(())
    }
}
