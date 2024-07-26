// This file is automatically generated, so please do not edit it.
// Generated by `flutter_rust_bridge`@ 2.1.0.

//
// From: `dart_preamble` in `app-rs-codegen/src/lib.rs`
// ignore_for_file: invalid_internal_annotation, always_use_package_imports, directives_ordering, prefer_const_constructors, sort_unnamed_constructors_first
//

// ignore_for_file: invalid_use_of_internal_member, unused_import, unnecessary_import

import '../frb_generated.dart';
import 'app.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

// These functions are ignored because they are not marked as `pub`: `new`
// These function are ignored because they are on traits that is not defined in current crate (put an empty `#[frb]` on it to unignore): `from`, `try_from`

class Settings {
  final String? locale;
  final String? fiatCurrency;

  const Settings({
    this.locale,
    this.fiatCurrency,
  });

  @override
  int get hashCode => locale.hashCode ^ fiatCurrency.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is Settings &&
          runtimeType == other.runtimeType &&
          locale == other.locale &&
          fiatCurrency == other.fiatCurrency;
}

class SettingsDb {
  final SettingsDbRs inner;

  const SettingsDb({
    required this.inner,
  });

  /// Read all settings.
  Settings read() => AppRs.instance.api.crateFfiSettingsSettingsDbRead(
        that: this,
      );

  /// Reset all settings to their defaults.
  void reset() => AppRs.instance.api.crateFfiSettingsSettingsDbReset(
        that: this,
      );

  /// Update the in-memory settings by merging in any non-null fields in
  /// `update`. The settings will be persisted asynchronously, outside of this
  /// call.
  void update({required Settings update}) => AppRs.instance.api
      .crateFfiSettingsSettingsDbUpdate(that: this, update: update);

  @override
  int get hashCode => inner.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is SettingsDb &&
          runtimeType == other.runtimeType &&
          inner == other.inner;
}
