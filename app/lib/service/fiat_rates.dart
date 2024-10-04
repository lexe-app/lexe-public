import 'dart:async' show Stream, StreamController;
import 'dart:math' as math;

import 'package:app_rs_dart/ffi/api.dart' show FiatRate, FiatRates;
import 'package:app_rs_dart/ffi/api.ext.dart' show FiatRatesExt;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/settings.dart' show LxSettings;
import 'package:lexeapp/stream_ext.dart';
import 'package:lexeapp/value_listenable_stream.dart' show ValueListenableExt;
import 'package:rxdart_ext/rxdart_ext.dart';

/// Maintains the user's current preferred [FiatRate] stream and periodically
/// refreshes the full [FiatRates] feed in the background.
class FiatRateService {
  FiatRateService._({required this.cancelTx, required this.fiatRate});

  final StreamController<void> cancelTx;
  final StateStream<FiatRate?> fiatRate;

  factory FiatRateService.start({
    required AppHandle app,
    required LxSettings settings,
    Stream<Null>? ticker,
  }) {
    // We'll close this stream to stop the FiatRateService.
    final cancelTx = StreamController<void>.broadcast(sync: true);

    // Default to refreshing the FiatRates every 15 min
    ticker ??=
        Stream<Null>.periodic(const Duration(minutes: 15)).startWith(null);

    // A stream of FiatRates? that updates periodically.
    final Stream<FiatRates?> fiatRates = ticker.asyncMap(
      (_) async {
        final fiatRates = await _fetchWithRetries(
          app: app,
          // Stop retries early
          isCanceled: () => cancelTx.isClosed,
        );
        return fiatRates?.ok;
      },
    ).takeUntil(cancelTx.stream);

    // The user's current preferred fiat currency, as a stream of changes.
    final fiatCurrency =
        settings.fiatCurrency.toValueStream().takeUntil(cancelTx.stream);

    // Combine the FiatRates feed and preferred currency, exposing the single
    // preferred [FiatRate]
    final fiatRate = Rx.combineLatest2(
      fiatRates,
      fiatCurrency,
      (fiatRates, fiatCurrency) => fiatRates?.findByFiat(fiatCurrency ?? "USD"),
    ).log(id: "fiatRate").toStateStream(null).asBroadcastStateStream();

    return FiatRateService._(cancelTx: cancelTx, fiatRate: fiatRate);
  }

  void cancel() {
    this.cancelTx.close();
  }
}

/// Call [AppHandle.fiatRates] but with retries + exp. backoff
Future<Result<FiatRates, void>?> _fetchWithRetries({
  required AppHandle app,
  required bool Function() isCanceled,
}) async =>
    retryWithBackoff(
      () => _fetch(app),
      backoff: const ClampedExpBackoff(
        base: Duration(milliseconds: 2500),
        exp: 2.0,
        max: Duration(minutes: 1),
      ),
      isCanceled: isCanceled,
    );

Future<Result<FiatRates, void>> _fetch(AppHandle app) async =>
    (await Result.tryFfiAsync(app.fiatRates))
        .mapErr((err) => error("fiatRates: Failed to fetch: $err"));

bool _alwaysFalse() => false;

Future<Result<T, E>?> retryWithBackoff<T, E>(
  final Future<Result<T, E>> Function() fn, {
  required final BackoffPolicy backoff,
  final bool Function() isCanceled = _alwaysFalse,
}) async {
  int iter = 0;
  while (true) {
    final res = await fn();
    // Check for cancelation after every await point
    if (isCanceled()) return null;
    // Success -> return Ok
    if (res.isOk) return res;

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
