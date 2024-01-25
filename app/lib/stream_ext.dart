import 'dart:async' show Stream, StreamController;

import 'package:rxdart_ext/rxdart_ext.dart';

import 'logger.dart';

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
  Stream<T> log({
    required String id,
  }) =>
      this.doOn(
        data: (data) => info("$id: $data"),
        error: (err, trace) => error("$id: error: $err, $trace"),
      );
}
