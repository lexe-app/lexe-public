import 'package:app_rs_dart/ffi/api.dart';
import 'package:app_rs_dart/ffi/app_data.dart' show AppData, AppDataDb;

import 'package:flutter/foundation.dart';
import 'package:lexeapp/notifier_ext.dart' show ValueNotifierExt;
import 'package:lexeapp/result.dart';

/// Lexe App settings
class LxAppData {
  factory LxAppData(final AppDataDb db) {
    final appData = db.read();

    final humanBitcoinAddress = ValueNotifier(appData.humanBitcoinAddress);

    return LxAppData._(db, humanBitcoinAddress);
  }

  LxAppData._(this._db, this._humanBitcoinAddress);

  final AppDataDb _db;

  final ValueNotifier<HumanBitcoinAddress?> _humanBitcoinAddress;
  ValueListenable<HumanBitcoinAddress?> get humanBitcoinAddress =>
      this._humanBitcoinAddress;

  void reset() {
    this._db.reset();

    this._humanBitcoinAddress.value = null;
  }

  FfiResult<void> update(final AppData update) {
    // Update Rust SettingsDb persistence layer.
    final result = Result.tryFfi(() => this._db.update(update: update));
    if (result.isErr) {
      return result;
    }

    // Update ValueNotifier's
    this._humanBitcoinAddress.update(update.humanBitcoinAddress);

    // Can't create an Ok(void), so just return this `result` that conveniently
    // has the right type.
    return result;
  }
}
