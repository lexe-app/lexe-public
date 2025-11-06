/// Combinators and extensions for Flutter's [Listenable], [ValueListenable],
/// [ValueNotifier], [ChangeNotifier], etc...
library;

import 'dart:async' show Completer, Timer;

import 'package:flutter/foundation.dart'
    show
        ChangeNotifier,
        Listenable,
        ValueListenable,
        ValueNotifier,
        VoidCallback,
        describeIdentity,
        kFlutterMemoryAllocationsEnabled;

/// A [ChangeNotifier] that lets us notify listeners externally.
class LxChangeNotifier extends ChangeNotifier {
  // [ChangeNotifier.notifyListeners] is not public... hence this method.
  void notify() => super.notifyListeners();
}

extension ListenableExt on Listenable {
  /// Listen for notifications like [addListener], but return an [LxListener]
  /// handle that can be easily paused, resumed, and disposed.
  LxListener listen(VoidCallback listener) {
    return LxListener.listen(this, listener);
  }

  /// Returns a Future that resolves when the next notification is fired.
  Future<void> next() {
    final Completer<void> completer = Completer.sync();

    void listener() {
      this.removeListener(listener);
      completer.complete();
    }

    this.addListener(listener);

    return completer.future;
  }
}

extension ValueListenableExt<T> on ValueListenable<T> {
  /// Returns a Future that resolves when the value is updated next.
  Future<T> nextValue() {
    final Completer<T> completer = Completer.sync();

    void listener() {
      this.removeListener(listener);
      completer.complete(this.value);
    }

    this.addListener(listener);

    return completer.future;
  }

  ComputedValueListenable<U> map<U>(U Function(T t) mapper) {
    final combined = ComputedValueListenable(mapper(this.value));

    void onParentUpdated() => combined._setValue(mapper(this.value));
    this.addListener(onParentUpdated);

    void onDispose() => this.removeListener(onParentUpdated);
    combined._onDispose = onDispose;

    return combined;
  }
}

/// [AlwaysValueNotifier] is a [ValueNotifier] that always notifies its
/// listeners whenever the value is set, as it doesn't check if the value
/// changed. Useful for cases where comparing the values for equality is
/// expensive or the value will almost always be different for each notification.
///
/// Like [ValueNotifier] the owner must call [dispose] when this goes
/// out-of-scope.
class AlwaysValueNotifier<T> extends ChangeNotifier
    implements ValueListenable<T> {
  AlwaysValueNotifier(this._value) {
    if (kFlutterMemoryAllocationsEnabled) {
      ChangeNotifier.maybeDispatchObjectCreation(this);
    }
  }

  /// The current value stored in this notifier.
  @override
  T get value => this._value;
  T _value;
  set value(T newValue) {
    this._value = newValue;
    this.notifyListeners();
  }

  @override
  String toString() => '${describeIdentity(this)}(${this._value})';
}

/// Create a [ValueListenable] that combines the [value]s of two parent
/// [ValueListenable]s.
///
/// Must call [dispose].
ComputedValueListenable<T> combine2<T, A, B>(
  ValueListenable<A> listenableA,
  ValueListenable<B> listenableB,
  T Function(A a, B b) combiner,
) {
  final value = combiner(listenableA.value, listenableB.value);
  final combined = ComputedValueListenable(value);

  void onParentUpdated() {
    combined._setValue(combiner(listenableA.value, listenableB.value));
  }

  listenableA.addListener(onParentUpdated);
  listenableB.addListener(onParentUpdated);

  void onDispose() {
    listenableA.removeListener(onParentUpdated);
    listenableB.removeListener(onParentUpdated);
  }

  combined._onDispose = onDispose;
  return combined;
}

/// Create a [ValueListenable] that combines the [value]s of two parent
/// [ValueListenable]s.
///
/// Must call [dispose].
ComputedValueListenable<T> combine3<T, A, B, C>(
  ValueListenable<A> listenableA,
  ValueListenable<B> listenableB,
  ValueListenable<C> listenableC,
  T Function(A a, B b, C c) combiner,
) {
  final value = combiner(
    listenableA.value,
    listenableB.value,
    listenableC.value,
  );
  final combined = ComputedValueListenable(value);

  void onParentUpdated() {
    combined._setValue(
      combiner(listenableA.value, listenableB.value, listenableC.value),
    );
  }

  listenableA.addListener(onParentUpdated);
  listenableB.addListener(onParentUpdated);
  listenableC.addListener(onParentUpdated);

  void onDispose() {
    listenableA.removeListener(onParentUpdated);
    listenableB.removeListener(onParentUpdated);
    listenableC.removeListener(onParentUpdated);
  }

  combined._onDispose = onDispose;
  return combined;
}

/// A [ValueListenable] that combines the [value]s of some number of parent
/// [ValueListenable]s.
///
/// Must call [dispose].
class ComputedValueListenable<T> extends ValueNotifier<T> {
  ComputedValueListenable(super._value);

  late final VoidCallback _onDispose;

  @override
  void dispose() {
    this._onDispose();
    super.dispose();
  }

  void _setValue(T value) => super.value = value;

  @override
  set value(T newValue) {
    throw UnsupportedError(
      "CombinedValueListenable doesn't support setting the value",
    );
  }
}

/// Notify listeners with the current [DateTime] every `period` [Duration].
class DateTimeNotifier extends AlwaysValueNotifier<DateTime> {
  factory DateTimeNotifier({required Duration period}) {
    final notifier = DateTimeNotifier._();
    final ticker = Timer.periodic(period, notifier._onTick);
    notifier._ticker = ticker;
    return notifier;
  }

  DateTimeNotifier._() : super(DateTime.now());

  late final Timer _ticker;

  void _onTick(Timer _timer) => this.value = DateTime.now();

  @override
  void dispose() {
    this._ticker.cancel();
    super.dispose();
  }
}

/// An easier handle on a consumer of a [Listenable].
///
/// The owner must call [dispose].
class LxListener {
  LxListener._(this._isPaused, this._listenable, this._listener);

  factory LxListener.listen(Listenable listenable, VoidCallback listener) {
    listenable.addListener(listener);
    return LxListener._(false, listenable, listener);
  }

  factory LxListener.paused(Listenable listenable, VoidCallback listener) {
    return LxListener._(true, listenable, listener);
  }

  bool _isPaused;
  final Listenable _listenable;
  final VoidCallback _listener;

  /// Pause listening for notifications. Does nothing if already paused.
  void pause() {
    if (!this._isPaused) {
      this._listenable.removeListener(this._listener);
      this._isPaused = true;
    }
  }

  /// Resume listening for notifications. Does nothing if already listening.
  void resume() {
    if (this._isPaused) {
      this._listenable.addListener(this._listener);
      this._isPaused = false;
    }
  }

  /// Stop listening for notifications. The owner must call this before the
  /// handle goes out-of-scope.
  void dispose() => this.pause();
}

/// Update a [ValueNotifier] only if the new value is not null.
extension ValueNotifierExt<T> on ValueNotifier<T?> {
  void update(final T? update) {
    if (update != null) {
      this.value = update;
    }
  }
}
