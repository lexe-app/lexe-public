import 'dart:async' show Timer, Zone, ZoneDelegate, ZoneSpecification, runZoned;
import 'dart:collection' show Queue;

import 'package:collection/collection.dart' show HeapPriorityQueue;

typedef Microtask = void Function();

/// [MockTime] allows you to run a block with simulated and controlled time.
class MockTime {
  MockTime();

  /// True when the simulation is advancing time. Used to prevent advancing time
  /// in a task callback (re-entrancy).
  bool _isAdvancing = false;

  /// The amount of time elapsed in the simulation.
  Duration _elapsed = Duration.zero;

  /// The queue of ready-to-run microtasks.
  final Queue<Microtask> _microtasks = Queue<Microtask>();

  /// A min-heap of [Timer]s by their trigger deadline.
  final HeapPriorityQueue<MockTimer> _timers = HeapPriorityQueue<MockTimer>(
    // It's a max-heap but we want pop-min, so negate the comparator to get a
    // min-heap.
    (t0, t1) => -t0.deadline.compareTo(t1.deadline),
  );

  /// The simulation is quiescent if there are no outstanding tasks and all
  /// outstanding timers are periodic.
  bool isQuiescent() =>
      this._microtasks.isEmpty &&
      this._timers.unorderedElements.every((timer) => timer._isPeriodic);

  /// Run the given callback with simulated time.
  T run<T>(T Function(MockTime self) callback) {
    return runZoned(
      () => callback(this),
      zoneSpecification: ZoneSpecification(
        createTimer: this._createTimer,
        createPeriodicTimer: this._createPeriodicTimer,
        scheduleMicrotask: this._scheduleMicrotask,
      ),
    );
  }

  /// Move the simulated time forward by [duration], running any outstanding
  /// tasks and newly triggered timers.
  void advance(Duration duration) {
    // Don't support re-entrance
    assert(!this._isAdvancing);
    this._isAdvancing = true;

    final advanceDeadline = this._elapsed + duration;

    this._runAllMicrotasks();
    while (true) {
      if (this._timers.isEmpty) break;
      if (this._timers.first.deadline > advanceDeadline) break;

      final timer = this._timers.removeFirst();
      this._setElapsed(timer.deadline);
      timer._trigger();

      // Periodic timers need to be re-scheduled.
      if (timer._isPeriodic) this._timers.add(timer);

      this._runAllMicrotasks();
    }

    this._setElapsed(advanceDeadline);
    this._isAdvancing = false;
  }

  void _runAllMicrotasks() {
    while (this._microtasks.isNotEmpty) {
      final microtask = this._microtasks.removeFirst();
      microtask();
    }
  }

  void _setElapsed(Duration deadline) {
    if (deadline > this._elapsed) this._elapsed = deadline;
  }

  Timer _createTimer(
    Zone self,
    ZoneDelegate parent,
    Zone zone,
    Duration duration,
    void Function() callback,
  ) {
    final deadline = this._elapsed + duration;
    final timer = MockTimer(this, deadline, callback);
    this._timers.add(timer);
    return timer;
  }

  Timer _createPeriodicTimer(
    Zone self,
    ZoneDelegate parent,
    Zone zone,
    Duration period,
    void Function(Timer timer) callback,
  ) {
    final deadline = this._elapsed + period;
    final timer = MockTimer.periodic(this, deadline, callback, period);
    this._timers.add(timer);
    return timer;
  }

  void _scheduleMicrotask(
    Zone self,
    ZoneDelegate parent,
    Zone zone,
    Microtask microtask,
  ) {
    this._microtasks.add(microtask);
  }
}

class MockTimer implements Timer {
  MockTimer(this._runtime, this.deadline, void Function() callback)
    : _callback = callback,
      _period = null;

  MockTimer.periodic(
    this._runtime,
    this.deadline,
    void Function(Timer) callback,
    Duration period,
  ) : _callback = callback,
      _period = period;

  /// The simulation time this [Timer] should trigger at.
  Duration deadline;

  /// Periodic timers have this set non-null.
  final Duration? _period;

  final MockTime _runtime;
  final Function _callback;

  @override
  void cancel() => this._runtime._timers.remove(this);

  @override
  bool get isActive => this._runtime._timers.contains(this);

  int _tick = 0;

  @override
  int get tick => this._tick;

  bool get _isPeriodic => this._period != null;

  void _trigger() {
    this._tick += 1;
    final period = this._period;
    if (period != null) {
      this.deadline += period;
      this._callback(this);
    } else {
      this._callback();
    }
  }
}
