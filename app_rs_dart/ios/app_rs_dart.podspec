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

  # This will ensure the source files in Classes/ are included in the native
  # builds of apps using this FFI plugin. Podspec does not support relative
  # paths, so Classes contains a forwarder C file that relatively imports
  # `../src/*` so that the C sources can be shared among all target platforms.
  s.source           = { :path => '.' }
  s.source_files = 'Classes/**/*'

  s.dependency 'Flutter'
  s.platform = :ios, '12.0'
  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    # Don't build Intel binaries to reduce build time.
    'EXCLUDED_ARCHS[sdk=iphone*]' => 'i386 x86_64',
    'OTHER_LDFLAGS' => '-force_load ${BUILT_PRODUCTS_DIR}/libapp_rs.a',
  }
  s.swift_version = '5.0'

  # Builds the `app_rs_dart.framework` shared library unconditionally on every
  # build.
  s.script_phase = {
    :name => 'Build app_rs_dart shared library',
    :script => '${PODS_TARGET_SRCROOT}/../build_ios_macos.sh ios',
    :execution_position => :before_compile,
    :output_files => ['${BUILT_PRODUCTS_DIR}/libapp_rs.a'],
  }
end
