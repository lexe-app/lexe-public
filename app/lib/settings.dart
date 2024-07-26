import 'package:app_rs_dart/ffi/settings.dart' show Settings, SettingsDb;
import 'package:flutter/foundation.dart';
import 'package:lexeapp/result.dart';

/// Lexe App settings
class LxSettings {
  factory LxSettings(final SettingsDb db) {
    final settings = db.read();
    final locale = ValueNotifier(settings.locale);
    final fiatCurrency = ValueNotifier(settings.fiatCurrency);
    return LxSettings._(db, locale, fiatCurrency);
  }

  LxSettings._(this._db, this._locale, this._fiatCurrency);

  final SettingsDb _db;

  final ValueNotifier<String?> _locale;
  ValueListenable<String?> get locale => this._locale;

  final ValueNotifier<String?> _fiatCurrency;
  ValueListenable<String?> get fiatCurrency => this._fiatCurrency;

  void reset() {
    this._db.reset();
    this._locale.value = null;
    this._fiatCurrency.value = null;
  }

  FfiResult<void> update(final Settings update) {
    // Update Rust SettingsDb persistence layer.
    final result = Result.tryFfi(() => this._db.update(update: update));
    if (result.isErr) {
      return result;
    }

    // Update ValueNotifier's
    this._locale.update(update.locale);
    this._fiatCurrency.update(update.fiatCurrency);

    // Can't create an Ok(void), so just return this `result` that conveniently
    // has the right type.
    return result;
  }
}

extension ValueNotifierExt<T> on ValueNotifier<T?> {
  /// Only update if new value is not null.
  void update(final T? update) {
    if (update != null) {
      this.value = update;
    }
  }
}
