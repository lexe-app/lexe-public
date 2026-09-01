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
    final preferredFiatCurrency = ValueNotifier(appData.preferredFiatCurrency);

    return LxAppData._(db, humanBitcoinAddress, preferredFiatCurrency);
  }

  LxAppData._(this._db, this._humanBitcoinAddress, this._preferredFiatCurrency);

  final AppDataDb _db;

  final ValueNotifier<GetHumanBitcoinAddressResponse?> _humanBitcoinAddress;
  ValueListenable<GetHumanBitcoinAddressResponse?> get humanBitcoinAddress =>
      this._humanBitcoinAddress;

  final ValueNotifier<String?> _preferredFiatCurrency;
  ValueListenable<String?> get preferredFiatCurrency =>
      this._preferredFiatCurrency;

  void reset() {
    this._db.reset();

    this._humanBitcoinAddress.value = null;
    this._preferredFiatCurrency.value = null;
  }

  FfiResult<void> update(final AppData update) {
    // Update Rust SettingsDb persistence layer.
    final result = Result.tryFfi(() => this._db.update(update: update));
    if (result.isErr) {
      return result;
    }

    // Update ValueNotifier's
    this._humanBitcoinAddress.update(update.humanBitcoinAddress);
    this._preferredFiatCurrency.update(update.preferredFiatCurrency);

    // Can't create an Ok(void), so just return this `result` that conveniently
    // has the right type.
    return result;
  }
}
