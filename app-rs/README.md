# app-rs

This crate contains the Rust logic and FFI interface used in the Lexe mobile
app, which is written in [flutter](https://flutter.dev). The mobile app UI lives
[here](../app/README.md).

## Updating [`cargo-xcode`](https://gitlab.com/kornelski/cargo-xcode) integration

This process updates the generated `app-rs.xcodeproj` Xcode integration, used
for hooking up `cargo build -p app-rs` to the macOS and iOS app flutter builds.
We shouldn't need to do this very often, if at all.

First, make sure `cargo-xcode` is up-to-date:

```bash
$ cargo install -f cargo-xcode
```

Then regenerate the Xcode project:

```bash
$ cd app-rs
$ cargo xcode \
    --manifest-path ./Cargo.toml \
    --platforms "macosx iphoneos iphonesimulator"
```

For some reason it also generates these for our other targets. Delete those:

```bash
$ rm -rf \
    app-rs-codegen/app-rs-codegen.xcodeproj \
    node/node.xcodeproj \
    run-sgx/run-sgx.xcodeproj \
    sgx-test/sgx-test.xcodeproj
```

Open the _parent_ Xcode projects (`app/ios/Runner.xcodeproj` and
`app/macos/Runner.xcodeproj`). Under **Build Phases**, ensure they have this
generated `app-rs` project in the **Link Binary With Libraries** section.
