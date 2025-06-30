import 'dart:async' show Timer, unawaited;

import 'package:flutter/foundation.dart' show Listenable;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/notifier_ext.dart' show LxChangeNotifier;

/// Manage refreshes from a few sources:
/// * User-initiated manual refresh (ex: press refresh button, pull-to-refresh).
/// * Periodic background refresh after every 1 min of _inactivity_.
/// * Burst refresh (ex: after sending a payment, we quickly refresh to poll status).
class RefreshService {
  bool isDisposed = false;
  bool _isBurstRefreshing = false;

  /// Notifies listeners whenever any refresh is triggered.
  Listenable get refresh => this._refresh;
  final LxChangeNotifier _refresh = LxChangeNotifier();

  /// Trigger a refresh passively, after 1 min of _inactivity_.
  late Timer _backgroundTimer = this._makeBackgroundTimer();

  /// Don't allow refreshes more than once / 1 sec.
  final ThrottleTime _throttle = ThrottleTime(const Duration(seconds: 1));

  /// Unconditionally trigger a refresh, without considering any throttling.
  void triggerRefreshUnthrottled() {
    assert(!this.isDisposed);
    info("refresh: triggered");
    // Reset background timer. We reset here instead of using a `Timer.periodic`
    // so the background timer only successfully triggers after 1 min of
    // _inactivity_.
    this._backgroundTimer.cancel();
    this._backgroundTimer = this._makeBackgroundTimer();
    // Update throttle
    this._throttle.update();
    // Notify listeners
    this._refresh.notify();
  }

  /// Trigger a refresh event. Will be throttled if last refresh was less than 3
  /// sec ago.
  void triggerRefresh() {
    if (this._throttle.isAllowed()) {
      this.triggerRefreshUnthrottled();
    } else {
      info("refresh: throttled");
    }
  }

  /// Pause the background refreshing.
  void pauseBackgroundRefresh() => this._backgroundTimer.cancel();
  void resumeBackgroundRefresh() =>
      this._backgroundTimer = this._makeBackgroundTimer();

  /// Trigger a "burst" of refreshes in rapid succession after we e.g. send a
  /// payment and want to quickly poll its status as it updates.
  void triggerBurstRefresh() {
    if (this._isBurstRefreshing) return;
    // Fire off a task to execute the background refresh.
    unawaited(this._doBurstRefresh());
  }

  Future<void> _doBurstRefresh() async {
    this._isBurstRefreshing = true;

    // Start with a few quick refreshes to pick up any immediate changes. Then
    // poll every 8 sec to catch slow LN payments that can take ~30-60 sec to
    // finalize.
    //
    // TODO(phlip9): currently wasteful. need node event stream to improve.
    const delays = [
      Duration(seconds: 0),
      Duration(seconds: 1),
      Duration(seconds: 2),
      Duration(seconds: 4),
      Duration(seconds: 8), // 16
      Duration(seconds: 8), // 24
      Duration(seconds: 8), // 32
      Duration(seconds: 8), // 40
      Duration(seconds: 8), // 48
      Duration(seconds: 8), // 56
    ];

    for (final delay in delays) {
      await Future.delayed(delay);
      if (this.isDisposed) return;

      info("refresh: burst refresh");
      this.triggerRefresh();
    }

    this._isBurstRefreshing = false;
  }

  Timer _makeBackgroundTimer() =>
      Timer(const Duration(minutes: 1), this.triggerRefresh);

  void dispose() {
    assert(!this.isDisposed);

    this._backgroundTimer.cancel();
    this._refresh.dispose();

    this.isDisposed = true;
  }
}

/// Throttle events so they don't occur more frequently than once in every
/// [duration].
class ThrottleTime {
  ThrottleTime(this._duration);

  final Duration _duration;
  DateTime? _prev;

  /// Returns true if an event would be allowed and not throttled.
  bool isAllowed() {
    final prev = this._prev;
    if (prev == null) return true;

    final now = DateTime.now();
    if (now.isBefore(prev)) return false;

    final elapsed = now.difference(prev);
    return elapsed >= this._duration;
  }

  /// Returns true if an event should be allowed, and updates the throttle if so.
  bool isAllowedAndUpdate() {
    final isAllowed = this.isAllowed();
    if (isAllowed) this.update();
    return isAllowed;
  }

  /// Unconditionally updates the throttle so it disallows new events until
  /// [_duration] time has elapsed.
  void update() {
    final now = DateTime.now();
    final nowMs = now.millisecondsSinceEpoch;
    final prevMs = this._prev?.millisecondsSinceEpoch ?? 0;
    if (nowMs > prevMs) this._prev = now;
  }
}
