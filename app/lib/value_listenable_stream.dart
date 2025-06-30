//! Adapt a `ValueListenable<T>` into a `ValueStream<T>`

import 'dart:async' show Stream, StreamController, StreamSubscription;

import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:rxdart/rxdart.dart'
    show StreamNotification, ValueStream, ValueStreamError;
import 'package:rxdart_ext/rxdart_ext.dart';

/// Convenience extension method on [ValueListenable] to convert it into a
/// [ValueStream].
extension ValueListenableExt<T> on ValueListenable<T> {
  ValueListenableStream<T> toValueStream() => ValueListenableStream(this);
}

/// A Single-subscription [ValueStream] that will emit data whenever a
/// [ValueListenable] changes.
class ValueListenableStream<T> extends Stream<T> implements ValueStream<T> {
  ValueListenableStream(this._valueListenable);

  final ValueListenable<T> _valueListenable;
  StreamController<T>? _controller;

  @override
  StreamSubscription<T> listen(
    void Function(T event)? onData, {
    Function? onError,
    void Function()? onDone,
    bool? cancelOnError,
  }) {
    if (this._controller != null) {
      throw StateError(
        "There's already a listener on this ValueListenableStream",
      );
    }

    this._controller = StreamController<T>(
      onListen: _onListen,
      onPause: _onPause,
      onResume: _onResume,
      onCancel: _onCancel,
    );

    return this._controller!.stream.listen(
      onData,
      onError: onError,
      onDone: onDone,
      cancelOnError: cancelOnError,
    );
  }

  /// This callback is registered with the inner `_valueListenable` and gets
  /// called whenever it has a new value.
  void _listener() {
    this._controller!.add(this._valueListenable.value);
  }

  /// Close the stream. A [ValueListenable] doesn't notify when it gets disposed
  /// so this needs to be called manually.
  Future close() async {
    await this._controller?.close();
  }

  // These callbacks are registered with the inner `_controller`

  void _onListen() {
    // Emit the initial value
    this._controller!.add(this._valueListenable.value);

    this._valueListenable.addListener(this._listener);
  }

  void _onResume() {
    this._valueListenable.addListener(this._listener);
  }

  void _onPause() {
    this._valueListenable.removeListener(this._listener);
  }

  void _onCancel() {
    this._valueListenable.removeListener(this._listener);
  }

  // impl ValueStream

  @override
  bool get hasValue => true;

  @override
  T get value => this._valueListenable.value;

  @override
  T? get valueOrNull => this._valueListenable.value;

  @override
  bool get hasError => false;

  @override
  Object get error => throw ValueStreamError.hasNoError();

  @override
  Object? get errorOrNull => null;

  @override
  StackTrace? get stackTrace => null;

  @override
  StreamNotification<T>? get lastEventOrNull {
    final _controller = this._controller;
    if (_controller == null) {
      return null;
    } else if (_controller.isClosed) {
      return StreamNotification<T>.done();
    } else {
      return StreamNotification<T>.data(this.value);
    }
  }
}
