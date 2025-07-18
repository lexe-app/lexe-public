// This file is automatically generated, so please do not edit it.
// @generated by `flutter_rust_bridge`@ 2.7.1.

//
// From: `dart_preamble` in `app-rs-codegen/src/lib.rs`
// ignore_for_file: invalid_internal_annotation, always_use_package_imports, directives_ordering, prefer_const_constructors, sort_unnamed_constructors_first
//

// ignore_for_file: invalid_use_of_internal_member, unused_import, unnecessary_import

import '../frb_generated.dart';
import '../lib.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'types.dart';

// Rust type: RustOpaqueNom<SecretStoreRs>
abstract class SecretStoreRs implements RustOpaqueInterface {}

/// Dart interface to the app secret store.
class SecretStore {
  final SecretStoreRs inner;

  const SecretStore.raw({required this.inner});

  /// Create a handle to the secret store for the current app configuration.
  factory SecretStore({required Config config}) =>
      AppRs.instance.api.crateFfiSecretStoreSecretStoreNew(config: config);

  /// Read the user's root seed from the secret store.
  RootSeed? readRootSeed() =>
      AppRs.instance.api.crateFfiSecretStoreSecretStoreReadRootSeed(that: this);

  @override
  int get hashCode => inner.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is SecretStore &&
          runtimeType == other.runtimeType &&
          inner == other.inner;
}
