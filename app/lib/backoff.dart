import 'dart:math' as math show pow;

import 'package:lexeapp/result.dart' show Result;

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
