import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/foundation.dart' show ValueListenable, ValueNotifier;
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/result.dart' show Err, Ok, Result;

/// Derived from [AppHandle}. Knows if we're currently provisioned or not.
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

  Future<void> provision() async {
    assert(!this.isDisposed);

    /// Return early if we're already provisioned. We only need to provision once.
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
        break;
      case Err(:final err):
        this._shouldDisplayWarning.value = true;
        error("provision: err: ${err.message}");
        break;
    }
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
