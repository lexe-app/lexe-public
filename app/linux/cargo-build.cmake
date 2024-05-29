# This little script effectively calls `cargo build -p app-rs` on every build.
# It builds the `app-rs` crate as a shared lib (`libapp_rs.so`) and tells cmake
# to link and bundle it with the flutter app binary.

# TODO(phlip9): Right now this only builds for the current host platform. Figure
# out how to make this target aware.

# TODO(phlip9): Get this working with static linking

# Set some local variables for convenience (these don't affect env!).
set(CARGO_WORKSPACE_DIR "${CMAKE_SOURCE_DIR}/../..")

set(LIBAPP_RS_SO "${CARGO_WORKSPACE_DIR}/target/x86_64-unknown-linux-gnu/release/libapp_rs.so")

# Add the `ExternalProject` CMake module.
include(ExternalProject)

# Always run `cargo build -p app-rs` on each build.
#
# `cargo rustc --crate-type=cdylib` is like `cargo build` but lets us build a
# shared library specifically (libapp_rs.so).
#
# ${CMAKE_SOURCE_DIR} == app/linux
# [CMake `ExternalProject`](https://cmake.org/cmake/help/latest/module/ExternalProject.html)
# [CMake Generator Expressions (the $<...> template thing)](https://cmake.org/cmake/help/latest/manual/cmake-generator-expressions.7.html)
ExternalProject_Add(
    app_rs
    BUILD_COMMAND cargo rustc -p app-rs
        --target=x86_64-unknown-linux-gnu
        --manifest-path=${CARGO_WORKSPACE_DIR}/Cargo.toml
        --crate-type=cdylib
        --release
    BUILD_BYPRODUCTS ${LIBAPP_RS_SO}
    BUILD_IN_SOURCE true
    BUILD_ALWAYS true
    BUILD_JOB_SERVER_AWARE true
    CONFIGURE_COMMAND ""
    DOWNLOAD_COMMAND ""
    INSTALL_COMMAND ""
)

# Build the native lib before the flutter binary.
add_dependencies(${BINARY_NAME} app_rs)

# Tell the linker to link against our *.so lib.
# <https://cmake.org/cmake/help/latest/command/target_link_libraries.html>
target_link_libraries(${BINARY_NAME} PUBLIC ${LIBAPP_RS_SO})

# Flutter needs to bundle our shared libraries with the actual executable.
list(APPEND PLUGIN_BUNDLED_LIBRARIES ${LIBAPP_RS_SO})
