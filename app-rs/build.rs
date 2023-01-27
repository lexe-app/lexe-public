//! This build.rs re-runs the `flutter_rust_bridge_codegen` tool whenever
//! `src/bindings.rs` changes.
//!
//! The codegen generates Rust<->C ffi bindings in `src/bindings_generated.rs`,
//! which are then consumed by Dart's `ffigen` tool to produce the final Dart
//! code in `../app/lib/bindings.dart` and `../app/lib/bindings_api.dart`.

use lib_flutter_rust_bridge_codegen as frb;

#[allow(unused)]
fn dump_envs() {
    for (key, val) in std::env::vars() {
        eprintln!("{key} => {val}");
    }
}

fn main() {
    // // Uncomment this to see flutter_rust_bridge debug logs
    // env_logger::init();

    // dump_envs();
    // panic!("debug");

    println!("cargo:rerun-if-changed=src/bindings.rs");

    // TODO(phlip9): only generate ios and mac outputs for those targets?

    // flutter_rust_bridge options
    let configs = frb::config_parse(frb::RawOpts {
        // Path of input Rust code
        rust_input: vec!["src/bindings.rs".to_owned()],
        // Path to output generated Rust code.
        rust_output: Some(vec!["src/bindings_generated.rs".to_owned()]),

        // Path to output generated Dart code impls.
        dart_output: vec!["../app/lib/bindings_generated.dart".to_owned()],
        // Path to output generated Dart API declarations (decls only, no impls)
        // so you can easily read what APIs are available from the Dart side.
        dart_decl_output: Some(
            "../app/lib/bindings_generated_api.dart".to_owned(),
        ),

        // These steps dump headers with all the emitted ffi symbols. We also
        // reference these symbols from a dummy method so they don't get
        // stripped by the over-agressive iOS/macOS symbol stripper.
        c_output: Some(vec![
            "../app/ios/Runner/bindings_generated.h".to_owned()
        ]),
        // extra_c_output_path: Some(vec!["../app/macos/Runner/".to_owned()]),

        // Other options
        wasm: false,
        dart_format_line_length: 80,
        skip_add_mod_to_lib: true,
        // inlines the cbindgen section into `bindings_generated.rs`
        inline_rust: true,
        ..Default::default()
    });

    // read Rust symbols to generate bindings for from `src/bindings.rs`.
    let all_symbols = frb::get_symbols_if_no_duplicates(&configs).expect(
        "flutter_rust_bridge: failed to read Rust symbols from bindings.rs",
    );
    // actually generate dart and rust ffi bindings.
    for config in configs.iter() {
        frb::frb_codegen(config, &all_symbols).expect(
            "flutter_rust_bridge: failed to generate Dart ffi bindings",
        );
    }
}
