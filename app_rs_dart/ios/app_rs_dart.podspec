#
# To learn more about a Podspec see http://guides.cocoapods.org/syntax/podspec.html.
# Run `pod lib lint app_rs_dart.podspec` to validate before publishing.
#
Pod::Spec.new do |s|
  s.name             = 'app_rs_dart'
  s.version          = '0.0.1'
  s.summary          = 'Lexe app flutter/dart FFI'
  s.description      = 'Lexe app flutter/dart FFI'
  s.homepage         = 'https://lexe.app/'
  s.license          = { :type => 'PolyForm Noncommercial License 1.0.0', :file => '../../LICENSE.md' }
  s.author           = { 'Lexe Corporation' => 'noreply@lexe.app' }
  s.source           = { :path => '.' }
  # We have a dummy `app_rs_dart.c` file here, which tricks xcodebuild into
  # building a Framework. Flutter also seems to inject some extra symbols in
  # the compiled `app_rs_dart.o` somehow.
  s.source_files = 'Classes/**/*'
  s.dependency 'Flutter'
  s.platform = :ios, '12.0'
  s.swift_version = '5.0'

  # Configure xcodebuild for this Pod specifically.
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    # Don't build Intel binaries to reduce build time.
    'EXCLUDED_ARCHS[sdk=iphone*]' => 'i386 x86_64',
    # Force our static library to get linked into the final `app_rs_dart.framework`.
    # Fortunately, this doesn't break dead code elimination or stripping, so
    # while the raw `libapp_rs.a` is say ~23 MiB per platform, the final framework
    # will only be ~9.5 MiB, while still holding onto all the symbols we need.
    'OTHER_LDFLAGS' => '-force_load ${BUILT_PRODUCTS_DIR}/libapp_rs.a',
  }

  # Builds the `app-rs` crate as a static lib for all requested targets and then
  # places the output `libapp_rs.a` into `${BUILT_PRODUCTS_DIR}`.
  #
  # Later, xcodebuild will link this into the final plugin `app_rs_dart.framework`
  # shared library, along with the dummy `app_rs_dart.o` object.
  #
  # Run this script unconditionally on every build.
  s.script_phase = {
    :name => 'Build libapp_rs.a unified static library',
    :script => '${PODS_TARGET_SRCROOT}/../build_ios_macos.sh ios',
    :execution_position => :before_compile,
    :output_files => ['${BUILT_PRODUCTS_DIR}/libapp_rs.a'],
  }
end
