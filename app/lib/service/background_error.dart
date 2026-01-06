import 'package:flutter/cupertino.dart' show ValueNotifier;
import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/widgets.dart'
    show AppLifecycleState, WidgetsBinding, WidgetsBindingObserver;

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

/// Suppresses background errors when app is not in foreground.
class BackgroundErrorService with WidgetsBindingObserver {
  bool isDisposed = false;

  static const _resumeGracePeriod = Duration(seconds: 3);

  AppLifecycleState _lifecycleState = AppLifecycleState.resumed;
  DateTime? _resumedAt;

  /// Whether we should display error icon in the UI.
  ValueListenable<bool> get shouldDisplayErrors => this._shouldDisplayErrors;
  final ValueNotifier<bool> _shouldDisplayErrors = ValueNotifier(false);

  ValueListenable<List<BackgroundError>> get errors => this._errors;

  final ValueNotifier<List<BackgroundError>> _errors = ValueNotifier([]);

  void init() {
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    this._lifecycleState = state;
    if (state == AppLifecycleState.resumed) {
      this._resumedAt = DateTime.now();
    }
  }

  /// Returns true if errors should be suppressed due to app lifecycle state.
  bool get _shouldSuppressErrors {
    // Suppress when not in foreground
    if (this._lifecycleState != AppLifecycleState.resumed) return true;

    // Suppress during grace period after resuming
    final resumedAt = this._resumedAt;
    if (resumedAt != null) {
      final elapsed = DateTime.now().difference(resumedAt);
      if (elapsed < _resumeGracePeriod) return true;
    }

    return false;
  }

  void enqueue(BackgroundError err) {
    if (this._shouldSuppressErrors) return;

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

    WidgetsBinding.instance.removeObserver(this);
    this._shouldDisplayErrors.dispose();
    this._errors.dispose();

    this.isDisposed = true;
  }
}
