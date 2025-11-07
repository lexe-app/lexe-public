import 'dart:async' show Timer, unawaited;
import 'dart:math' as math;

import 'package:app_rs_dart/ffi/api.dart' show FiatRate, FiatRates;
import 'package:app_rs_dart/ffi/api.ext.dart' show FiatRatesExt;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/foundation.dart';
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/notifier_ext.dart' show AlwaysValueNotifier;
import 'package:lexeapp/result.dart';
import 'package:lexeapp/settings.dart' show LxSettings;

/// Maintains the user's current preferred [FiatRate] stream and periodically
/// refreshes the full [FiatRates] feed in the background.
class FiatRateService {
  FiatRateService._(this._app, this._settings, this._onError);

  factory FiatRateService.start({
    required AppHandle app,
    required LxSettings settings,
    void Function(String)? onError,
  }) {
    final svc = FiatRateService._(app, settings, onError);

    svc.fiatRates.addListener(svc.updateFiatRate);
    settings.fiatCurrency.addListener(svc.updateFiatRate);

    // Kick off with an initial fetch
    unawaited(svc.fetch());

    return svc;
  }

  final AppHandle _app;
  final LxSettings _settings;
  void Function(String)? _onError;

  bool isDisposed = false;

  late final Timer _ticker = Timer.periodic(
    const Duration(minutes: 15),
    (timer) => this.fetch,
  );

  final AlwaysValueNotifier<FiatRates?> fiatRates = AlwaysValueNotifier(null);
  final ValueNotifier<FiatRate?> fiatRate = ValueNotifier(null);

  Future<void> fetch() async {
    assert(!this.isDisposed);

    final fiatRates = await _fetchWithRetries(
      app: this._app,
      // Stop retries early
      isCanceled: () => this.isDisposed,
      onError: this._onError,
    );
    if (this.isDisposed) return;

    if (fiatRates case Ok(:final ok)) this.fiatRates.value = ok;
  }

  void updateFiatRate() {
    final fiatCurrency = this._settings.fiatCurrency.value;
    final fiatRate = this.fiatRates.value?.findByFiat(fiatCurrency ?? "USD");
    info("fiat-rate: $fiatRate");
    this.fiatRate.value = fiatRate;
  }

  void dispose() {
    assert(!this.isDisposed);

    this._ticker.cancel();
    this.fiatRates.dispose();
    this.fiatRate.dispose();

    this.fiatRates.removeListener(this.updateFiatRate);
    this._settings.fiatCurrency.removeListener(this.updateFiatRate);

    this.isDisposed = true;
    // info("fiat-rates: disposed");
  }
}

/// Call [AppHandle.fiatRates] but with retries + exp. backoff
Future<Result<FiatRates, void>?> _fetchWithRetries({
  required AppHandle app,
  required bool Function() isCanceled,
  void Function(String)? onError,
}) async => retryWithBackoff(
  () => _fetch(app),
  backoff: const ClampedExpBackoff(
    base: Duration(milliseconds: 2500),
    exp: 2.0,
    max: Duration(minutes: 1),
  ),
  isCanceled: isCanceled,
  onError: onError,
);

Future<Result<FiatRates, FfiError>> _fetch(AppHandle app) async =>
    Result.tryFfiAsync(app.fiatRates);

bool _alwaysFalse() => false;

Future<Result<T, E>?> retryWithBackoff<T, E>(
  final Future<Result<T, E>> Function() fn, {
  required final BackoffPolicy backoff,
  final bool Function() isCanceled = _alwaysFalse,
  void Function(String)? onError,
}) async {
  int iter = 0;
  while (true) {
    final res = await fn();
    // Check for cancelation after every await point
    if (isCanceled()) return null;
    // Success -> return Ok
    if (res.isOk) return res;

    error("fiatRates: Failed to fetch: ${res.err.toString()}");
    onError?.call(res.err.toString());
    // Error -> compute next backoff and wait
    final nextBackoff = backoff.nextBackoff(iter);
    // Ran out of attempts -> return Err
    if (nextBackoff == null) return res;

    await Future.delayed(nextBackoff);
    if (isCanceled()) return null;

    iter += 1;
  }
}

abstract interface class BackoffPolicy {
  const BackoffPolicy();

  Duration? nextBackoff(int iter);
}

class ClampedExpBackoff extends BackoffPolicy {
  const ClampedExpBackoff({
    required this.base,
    required this.exp,
    this.max = const Duration(minutes: 15),
  });

  final Duration base;
  final double exp;
  final Duration max;

  @override
  Duration? nextBackoff(int iter) {
    final nextMs = this.base.inMilliseconds * math.pow(this.exp, iter);
    if (nextMs.isInfinite) return this.max;

    return Duration(
      milliseconds: nextMs.round().clamp(0, this.max.inMilliseconds),
    );
  }
}
