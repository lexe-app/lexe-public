import 'package:app_rs_dart/ffi/api.dart';
import 'package:app_rs_dart/ffi/app_data.dart' show AppData, AppDataDb;

import 'package:flutter/foundation.dart';
import 'package:lexeapp/notifier_ext.dart' show ValueNotifierExt;
import 'package:lexeapp/result.dart';

/// Lexe App settings
class LxAppData {
  factory LxAppData(final AppDataDb db) {
    final appData = db.read();

    final paymentAddress = ValueNotifier(appData.paymentAddress);

    return LxAppData._(db, paymentAddress);
  }

  LxAppData._(this._db, this._paymentAddress);

  final AppDataDb _db;

  final ValueNotifier<PaymentAddress?> _paymentAddress;
  ValueListenable<PaymentAddress?> get paymentAddress => this._paymentAddress;

  void reset() {
    this._db.reset();

    this._paymentAddress.value = null;
  }

  FfiResult<void> update(final AppData update) {
    // Update Rust SettingsDb persistence layer.
    final result = Result.tryFfi(() => this._db.update(update: update));
    if (result.isErr) {
      return result;
    }

    // Update ValueNotifier's
    this._paymentAddress.update(update.paymentAddress);

    // Can't create an Ok(void), so just return this `result` that conveniently
    // has the right type.
    return result;
  }
}
