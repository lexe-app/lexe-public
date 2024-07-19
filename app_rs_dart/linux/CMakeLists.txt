# This cmake script unconditionally runs `cargo build -p app-rs` on each build
# and then bundles the resulting `libapp_rs.so` shared library in the final
# application package.

# The Flutter tooling requires that developers have CMake 3.10 or later
# installed. You should not increase this version, as doing so will cause
# the plugin to fail to compile for some customers of the plugin.
cmake_minimum_required(VERSION 3.10)

# TODO(phlip9): Right now this only builds for the current host platform. Figure
# out how to make this target aware.

# Project-level configuration.
set(PROJECT_NAME "app_rs_dart")

# Set some variables for convenience (these don't affect env).
# TODO(phlip9): getting the workspace dir this way only works if `app` is the
# package that's building us.
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

# # Flutter needs to bundle our shared lib with the actual executable.
set(app_rs_dart_bundled_libraries
  ${LIBAPP_RS_SO}
  PARENT_SCOPE
)

# # Dump ALL cmake variables for debugging
# get_cmake_property(_variableNames VARIABLES)
# list (SORT _variableNames)
# foreach (_variableName ${_variableNames})
#     message(STATUS "${_variableName}=${${_variableName}}")
# endforeach()

# BINARY_NAME=lexeapp
# CMAKE_SOURCE_DIR=app/linux (i.e., the dependent application package's linux dir)
# CMAKE_CURRENT_BINARY_DIR - where we're supposed to dump our shared lib?
# CMAKE_CXX_LIBRARY_ARCHITECTURE=x86_64-linux-gnu
# app_rs_dart_BINARY_DIR=app/build/linux/x64/release/plugins/app_rs_dart
