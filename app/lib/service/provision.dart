import 'dart:async';

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/foundation.dart' show ValueListenable, ValueNotifier;
import 'package:lexeapp/prelude.dart';

/// This service manages provisioning the user node.
///
/// Currently, at startup, the wallet operates in "offline" mode until we
/// successfully provision for the first time in the session.
class ProvisionService {
  ProvisionService({required AppHandle app}) : _app = app;

  final AppHandle _app;

  bool isDisposed = false;

  /// Whether we already successfully provisioned the user node on startup.
  ValueListenable<bool> get isProvisioned => this._isProvisioned;
  final ValueNotifier<bool> _isProvisioned = ValueNotifier(false);

  /// Whether we're currently provisioning the user node.
  ValueListenable<bool> get isProvisioning => this._isProvisioning;
  final ValueNotifier<bool> _isProvisioning = ValueNotifier(false);

  ValueListenable<bool> get shouldDisplayWarning => this._shouldDisplayWarning;
  final ValueNotifier<bool> _shouldDisplayWarning = ValueNotifier(false);

  /// If nobody has started provisioned yet, then start provisioning.
  /// Otherwise return immediately.
  ///
  /// Use this fn if you just need to (maybe) start provisioning.
  Future<void> provision() async {
    assert(!this.isDisposed);

    /// Return early if we're already provisioned. We only need to provision
    /// once.
    if (this.isProvisioned.value) return;

    // Skip if we're currently syncing
    if (this._isProvisioning.value) return;

    // Do sync
    this._isProvisioning.value = true;
    final res = await Result.tryFfiAsync(() => this._app.provision());
    if (this.isDisposed) return;
    this._isProvisioning.value = false;

    switch (res) {
      case Ok():
        info("node provisioned");
        this._shouldDisplayWarning.value = false;
        this._isProvisioned.value = true;
      case Err(:final err):
        this._shouldDisplayWarning.value = true;
        error("provision: err: ${err.message}");
    }
  }

  /// If nobody has started provisioned yet, then start provisioning.
  /// Always waits until the first provision completes. Returns immediately if
  /// we've already provisioned at least once this session.
  ///
  /// Use this fn to gate logic until after we've successfully provisioned.
  Future<void> waitUntilProvisioned() {
    assert(!this.isDisposed);
    if (this.isProvisioned.value) return Future.value();

    // Kick off a provision in the background if we're the first one waiting.
    if (!this._isProvisioning.value) unawaited(this.provision());

    final completer = Completer<void>();

    void listener() {
      this.isProvisioned.removeListener(listener);
      completer.complete();
    }

    this.isProvisioned.addListener(listener);

    return completer.future;
  }

  void wrapListener(void Function() listener) {
    assert(!this.isDisposed);
    if (this._isProvisioning.value || !this.isProvisioned.value) return;
    listener();
  }

  void dispose() {
    assert(!this.isDisposed);

    this._shouldDisplayWarning.dispose();
    this._isProvisioned.dispose();
    this._isProvisioning.dispose();

    this.isDisposed = true;
  }
}
