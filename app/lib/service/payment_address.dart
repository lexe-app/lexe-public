import 'package:app_rs_dart/ffi/api.dart' show PaymentAddress;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/app_data.dart' show AppData;
import 'package:app_rs_dart/ffi/types.dart' show Username;
import 'package:flutter/foundation.dart'
    show Listenable, ValueListenable, ValueNotifier;
import 'package:lexeapp/app_data.dart' show LxAppData;
import 'package:lexeapp/backoff.dart' show ClampedExpBackoff, retryWithBackoff;
import 'package:lexeapp/logger.dart' show debug, error;
import 'package:lexeapp/notifier_ext.dart' show LxChangeNotifier;
import 'package:lexeapp/result.dart' show Err, Ok, Result;

/// Merge states from [AppHandle.getPaymentAddress] and [AppData.paymentAddress] but instrumented
/// with various signals for UI consumption.
class PaymentAddressService {
  PaymentAddressService({required AppHandle app, required LxAppData appData})
    : _app = app,
      _appData = appData;

  final AppHandle _app;
  final LxAppData _appData;
  bool isDisposed = false;
  DateTime? _lastFetchedAt;

  /// The most recent [PaymentAddress]. `null` if we haven't stored any payment address yet.
  ValueListenable<PaymentAddress?> get paymentAddress =>
      this._appData.paymentAddress;

  /// Notifies after each completed fetch, successful or otherwise.
  Listenable get completed => this._completed;
  final LxChangeNotifier _completed = LxChangeNotifier();

  /// Notifies after each completed fetch, successful or otherwise.
  ValueListenable<bool> get isFetching => this._isFetching;
  final ValueNotifier<bool> _isFetching = ValueNotifier(false);

  ValueListenable<bool> get isUpdating => this._isUpdating;
  final ValueNotifier<bool> _isUpdating = ValueNotifier(false);

  bool get canFetch =>
      this._lastFetchedAt == null ||
      DateTime.now().difference(this._lastFetchedAt!) >
          const Duration(seconds: 10);

  Future<void> fetch() async {
    assert(!this.isDisposed);

    // Skip if we're currently syncing
    if (this._isFetching.value) return;

    // Skip if we tried to fetch recently
    if (!this.canFetch) return;

    // Do sync
    this._isFetching.value = true;
    final res = await this._fetchWithRetries(
      // Stop retries early
      isCanceled: () => this.isDisposed,
      onError: (String err) {
        error("paymentAddress: Failed to fetch: $err");
      },
    );
    if (this.isDisposed) return;

    this._isFetching.value = false;

    switch (res) {
      case null:
        debug("paymentAddress: Cancelled");
        return;
      case Ok(:final ok):
        this._appData.update(AppData(paymentAddress: ok));
        this._lastFetchedAt = DateTime.now();
      case Err():
        error("paymentAddress: Exhausted retries");
    }

    this._completed.notify();
  }

  Future<Result<void, String>> update({required Username username}) async {
    assert(!this.isDisposed);

    if (this._isUpdating.value) return Err("Already updating");

    this._isUpdating.value = true;
    final res = await Result.tryFfiAsync(
      () => this._app.updatePaymentAddress(username: username),
    );
    if (this.isDisposed) return Err("Already disposed");

    this._isUpdating.value = false;

    switch (res) {
      case Ok(:final ok):
        this._appData.update(AppData(paymentAddress: ok));
        this._lastFetchedAt = DateTime.now();
        return Ok(null);
      case Err(:final err):
        error("payment-address: err: ${err.message}");
        return Err(err.message);
    }
  }

  void dispose() {
    assert(!this.isDisposed);

    this._completed.dispose();
    this._isFetching.dispose();
    this._isUpdating.dispose();

    this.isDisposed = true;
  }

  Future<Result<PaymentAddress, void>> _fetch() async =>
      Result.tryFfiAsync(this._app.getPaymentAddress);

  Future<Result<PaymentAddress, void>?> _fetchWithRetries({
    required bool Function() isCanceled,
    void Function(String)? onError,
  }) async => retryWithBackoff(
    () => this._fetch(),
    backoff: const ClampedExpBackoff(
      base: Duration(milliseconds: 2500),
      exp: 2.0,
      max: Duration(minutes: 1),
    ),
    isCanceled: isCanceled,
    onError: onError,
  );
}
