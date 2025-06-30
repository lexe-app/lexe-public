import 'dart:async' show FutureOr, Stream, StreamController, StreamSubscription;

import 'package:flutter/foundation.dart';
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:rxdart_ext/rxdart_ext.dart';

extension StreamControllerExt<T> on StreamController<T> {
  /// Calls `add(event)` as long as the `StreamController` is not already
  /// closed.
  void addIfNotClosed(T event) {
    if (!this.isClosed) {
      this.add(event);
    }
  }
}

extension StreamExt<T> on Stream<T> {
  /// Log for each event or error that occurs on this stream.
  Stream<T> log({required String id}) => this.doOn(
    data: (data) => info("$id: $data"),
    error: (err, trace) => error("$id: error: $err, $trace"),
    // cancel: () => info("$id: cancel"),
    // done: () => info("$id: done"),
  );

  /// Alias for [Stream.where].
  Stream<T> filter(bool Function(T event) test) => this.where(test);

  /// Alias for [Stream.mapNotNull].
  Stream<R> filterMap<R extends Object>(R? Function(T) transform) =>
      MapNotNullStreamTransformer<T, R>(transform).bind(this);

  /// Creates a new stream with each data event of this stream asynchronously
  /// mapped to a new event.
  ///
  /// This is like [Stream.asyncMap] but it throws away any events that occur
  /// while the Future is running.
  ///
  /// The returned stream is a broadcast stream if this stream is.
  Stream<E> asyncMapUnbuffered<E>(FutureOr<E> Function(T event) convert) {
    StreamController<E> controller;
    if (this.isBroadcast) {
      controller = StreamController<E>.broadcast(sync: true);
    } else {
      controller = StreamController<E>(sync: true);
    }

    controller.onListen = () {
      FutureOr<Null> add(E value) {
        // Since the future might resolve after the stream has been closed, and
        // since dart futures are not cancelable, we need to check if the stream
        // is closed before adding the resolved value.
        controller.addIfNotClosed(value);
      }

      // Set this flag whenever we're running the `convert` Future. We'll skip
      // any new events while this flag is set.
      bool isProcessing = false;

      FutureOr<void> resume() {
        isProcessing = false;
      }

      final StreamSubscription<T> subscription = this.listen(
        onError: controller.addError,
        onDone: controller.close,
        (event) {
          // Skip events while we're processing.
          if (isProcessing) return;
          isProcessing = true;

          FutureOr<E> newValue;
          try {
            newValue = convert(event);
          } catch (e, s) {
            controller.addError(e, s);
            isProcessing = false;
            return;
          }

          if (newValue is Future<E>) {
            newValue
                .then(add, onError: controller.addError)
                .whenComplete(resume);
          } else {
            controller.add(newValue);
            isProcessing = false;
          }
        },
      );

      controller.onCancel = subscription.cancel;
      if (!this.isBroadcast) {
        controller
          ..onPause = subscription.pause
          ..onResume = subscription.resume;
      }
    };

    return controller.stream;
  }
}

extension StreamFilterOkExt<T extends Object, E> on Stream<Result<T, E>> {
  Stream<T> filterOk() => this.filterMap((res) => res.ok);
}

extension StreamFilterErrExt<T, E extends Object> on Stream<Result<T, E>> {
  Stream<E> filterErr() => this.filterMap((res) => res.err);
}

extension ValueStreamExt<T> on ValueStream<T> {
  /// Return a [ValueNotifier]-like object that is updated whenever this stream
  /// updates.
  ///
  /// NOTE: the returned [StreamValueNotifier] is an owned type that must be
  /// disposed!
  StreamValueNotifier<T> streamValueNotifier() {
    final listenable = StreamValueNotifier<T>(this.value);
    final subscription = this.listen(
      listenable._setValue,
      onDone: () {
        listenable._subscription = null;
      },
      cancelOnError: false,
    );
    listenable._subscription = subscription;
    return listenable;
  }
}

class StreamValueNotifier<T> extends ValueNotifier<T> {
  StreamValueNotifier(super._value);

  StreamSubscription<T>? _subscription;

  @override
  void dispose() {
    this._subscription?.cancel();
    super.dispose();
  }

  void _setValue(T value) => super.value = value;
}
