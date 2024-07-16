# The Flutter tooling requires that developers have CMake 3.10 or later
# installed. You should not increase this version, as doing so will cause
# the plugin to fail to compile for some customers of the plugin.
cmake_minimum_required(VERSION 3.10)

project(app_rs_dart_library VERSION 0.0.1 LANGUAGES C)

add_library(app_rs_dart SHARED
  "app_rs_dart.c"
)

set_target_properties(app_rs_dart PROPERTIES
  PUBLIC_HEADER app_rs_dart.h
  OUTPUT_NAME "app_rs_dart"
)

target_compile_definitions(app_rs_dart PUBLIC DART_SHARED_LIB)
