import 'package:app_rs_dart/ffi/api.dart';
import 'package:app_rs_dart/ffi/app_data.dart' show AppData, AppDataDb;

import 'package:flutter/foundation.dart';
import 'package:lexeapp/notifier_ext.dart' show ValueNotifierExt;
import 'package:lexeapp/result.dart';

/// Lexe App settings
class LxAppData {
  factory LxAppData(final AppDataDb db) {
    final appData = db.read();

    final humanAddress = ValueNotifier(appData.humanAddress);

    return LxAppData._(db, humanAddress);
  }

  LxAppData._(this._db, this._humanAddress);

  final AppDataDb _db;

  final ValueNotifier<HumanAddress?> _humanAddress;
  ValueListenable<HumanAddress?> get humanAddress => this._humanAddress;

  void reset() {
    this._db.reset();

    this._humanAddress.value = null;
  }

  FfiResult<void> update(final AppData update) {
    // Update Rust SettingsDb persistence layer.
    final result = Result.tryFfi(() => this._db.update(update: update));
    if (result.isErr) {
      return result;
    }

    // Update ValueNotifier's
    this._humanAddress.update(update.humanAddress);

    // Can't create an Ok(void), so just return this `result` that conveniently
    // has the right type.
    return result;
  }
}
