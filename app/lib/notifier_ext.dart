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
    notifyListeners();
  }

  @override
  String toString() => '${describeIdentity(this)}(${this._value})';
}

/// Create a [ValueListenable] that combines the [value]s of two parent
/// [ValueListenable]s.
///
/// Must call [dispose].
CombinedValueListenable<T> combine2<T, A, B>(
  ValueListenable<A> listenableA,
  ValueListenable<B> listenableB,
  T Function(A a, B b) combiner,
) {
  final value = combiner(listenableA.value, listenableB.value);
  final combined = CombinedValueListenable(value);

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

/// A [ValueListenable] that combines the [value]s of two parent
/// [ValueListenable]s.
///
/// Must call [dispose].
class CombinedValueListenable<T> extends ValueNotifier<T> {
  CombinedValueListenable(super._value);

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
        "CombinedValueListenable doesn't support setting the value");
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
