import 'package:flutter/cupertino.dart' show ValueNotifier;
import 'package:flutter/foundation.dart' show ValueListenable;

enum BackgroundErrorKind { nodeInfo, fiatRates, paymentSync }

class BackgroundError {
  BackgroundError(this.kind, this.message);

  BackgroundError.nodeInfo(String message)
    : this(BackgroundErrorKind.nodeInfo, message);
  BackgroundError.fiatRates(String message)
    : this(BackgroundErrorKind.fiatRates, message);
  BackgroundError.paymentSync(String message)
    : this(BackgroundErrorKind.paymentSync, message);

  final BackgroundErrorKind kind;
  final String message;
  final DateTime timestamp = DateTime.now();

  late final String id = Object.hash(
    kind.name,
    timestamp.microsecondsSinceEpoch,
    message,
  ).toRadixString(16);

  @override
  String toString() {
    return '${this.kind.name} ${this.timestamp.microsecondsSinceEpoch}: $message';
  }
}

/// A set of methods for displaying errors that happened in the background.
class BackgroundErrorService {
  bool isDisposed = false;

  /// Whether we should display error icon in the UI.
  ValueListenable<bool> get shouldDisplayErrors => this._shouldDisplayErrors;
  final ValueNotifier<bool> _shouldDisplayErrors = ValueNotifier(false);

  ValueListenable<List<BackgroundError>> get errors => this._errors;

  final ValueNotifier<List<BackgroundError>> _errors = ValueNotifier([]);

  void init() {}

  void enqueue(BackgroundError err) {
    this._errors.value.add(err);
    this._shouldDisplayErrors.value = true;
  }

  void ignore(BackgroundError err) {
    final updated = List<BackgroundError>.from(this._errors.value);
    updated.remove(err);
    this._errors.value = updated;
    this._shouldDisplayErrors.value = updated.isNotEmpty;
  }

  void ignoreAll() {
    this._errors.value = [];
    this._shouldDisplayErrors.value = false;
  }

  void dispose() {
    assert(!this.isDisposed);

    this._shouldDisplayErrors.dispose();
    this._errors.dispose();

    this.isDisposed = true;
  }
}
