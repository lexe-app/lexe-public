import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show PaymentUpdatedIndex;
import 'package:app_rs_dart/ffi/types.ext.dart' show PaymentUpdatedIndexExt;
import 'package:flutter/foundation.dart';
import 'package:lexeapp/notifier_ext.dart' show LxChangeNotifier;
import 'package:lexeapp/prelude.dart';

/// [AppHandle.syncPayments] but instrumented with various signals for UI
/// consumption.
class PaymentSyncService {
  PaymentSyncService({required AppHandle app, void Function(String)? onError})
    : _app = app,
      _onError = onError;

  final AppHandle _app;
  final void Function(String)? _onError;

  bool isDisposed = false;

  /// The max [PaymentUpdatedIndex] the [PaymentSyncService] has observed from
  /// sync. This is `null` at startup or if there are no payments. Used to
  /// determine if there are any new/updated payments after a sync.
  PaymentUpdatedIndex? _latestUpdatedIndex;

  /// Notifies after each completed sync, successful or otherwise.
  final LxChangeNotifier _completed = LxChangeNotifier();
  Listenable get completed => this._completed;

  /// Notifies when a successful sync observes a newer update index,
  /// including updates synced by other callers.
  final LxChangeNotifier _updated = LxChangeNotifier();
  Listenable get updated => this._updated;

  /// True whenever we're currently syncing payments.
  final ValueNotifier<bool> _isSyncing = ValueNotifier(false);
  ValueListenable<bool> get isSyncing => this._isSyncing;

  Future<void> sync() async {
    assert(!this.isDisposed);

    // Skip if we're currently syncing
    if (this._isSyncing.value) return;

    // Do sync
    this._isSyncing.value = true;
    final res = await Result.tryFfiAsync(this._app.syncPayments);
    if (this.isDisposed) return;
    this._isSyncing.value = false;

    switch (res) {
      case Ok(:final ok):
        final latestUpdatedIndex = ok.latestUpdatedIndex;
        final anyChanged = this._latestUpdatedIndex < latestUpdatedIndex;
        this._latestUpdatedIndex = latestUpdatedIndex;
        if (anyChanged) this._updated.notify();
        info("payment-sync: anyChanged = $anyChanged");
      case Err(:final err):
        error("payment-sync: err: ${err.message}");
        this._onError?.call(err.message);
    }

    this._completed.notify();
  }

  void dispose() {
    assert(!this.isDisposed);

    this._completed.dispose();
    this._updated.dispose();
    this._isSyncing.dispose();

    this.isDisposed = true;
    // info("payment-sync: disposed");
  }
}
