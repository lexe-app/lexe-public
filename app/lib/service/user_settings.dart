import 'package:app_rs_dart/ffi/api.dart'
    show UpdateUserSettingsRequest, UserSettings;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/app_data.dart' show AppData;
import 'package:flutter/foundation.dart'
    show Listenable, ValueListenable, ValueNotifier;
import 'package:lexeapp/app_data.dart' show LxAppData;
import 'package:lexeapp/notifier_ext.dart' show LxChangeNotifier;
import 'package:lexeapp/prelude.dart';

/// [AppHandle.getUserSettings] and [AppHandle.updateUserSettings], instrumented
/// with various signals for UI consumption.
///
/// The node is authoritative for these settings, unlike the app-local
/// settings in [AppHandle.settingsDb].
///
/// On fetch, user settings are locally cached in [LxAppData].
class UserSettingsService {
  UserSettingsService({
    required AppHandle app,
    required LxAppData appData,
    void Function(String)? onError,
  }) : _app = app,
       _appData = appData,
       _onError = onError;

  final AppHandle _app;
  final LxAppData _appData;
  final void Function(String)? _onError;

  bool isDisposed = false;

  /// Notifies after each completed fetch, successful or otherwise.
  Listenable get completed => this._completed;
  final LxChangeNotifier _completed = LxChangeNotifier();

  /// True whenever we're fetching the next [UserSettings].
  ValueListenable<bool> get isFetching => this._isFetching;
  final ValueNotifier<bool> _isFetching = ValueNotifier(false);

  /// True whenever an update is in-flight.
  ValueListenable<bool> get isUpdating => this._isUpdating;
  final ValueNotifier<bool> _isUpdating = ValueNotifier(false);

  Future<void> fetch() async {
    assert(!this.isDisposed);

    // Skip if we're currently fetching
    if (this._isFetching.value) return;

    this._isFetching.value = true;
    final res = await Result.tryFfiAsync(this._app.getUserSettings);
    if (this.isDisposed) return;
    this._isFetching.value = false;

    switch (res) {
      case Ok(:final ok):
        info("user-settings: $ok");
        this._appData.update(
          AppData(preferredFiatCurrency: ok.preferredFiatCurrency),
        );

        final cached = this._appData.preferredFiatCurrency.value;
        // If the node doesn't have a setting and an update to the node fails,
        // the app may store a value that can't be cleared by `AppData.update`.
        // So, push the cached value to the node instead.
        if (ok.preferredFiatCurrency != cached) {
          await this.update(preferredFiatCurrency: cached);
        }
      case Err(:final err):
        error("user-settings: err: ${err.message}");
        this._onError?.call(err.message);
    }

    this._completed.notify();
  }

  /// Update the node's settings. Settings left `null` are unchanged.
  Future<Result<void, String>> update({String? preferredFiatCurrency}) async {
    assert(!this.isDisposed);

    // Skip if we're currently updating
    if (this._isUpdating.value) return Err("Already updating");

    this._isUpdating.value = true;

    // Update the cache before we hit the network, so the UI reflects the new
    // settings immediately instead of a round-trip later.
    final rollback = this._appData.preferredFiatCurrency.value;
    this._appData.update(AppData(preferredFiatCurrency: preferredFiatCurrency));

    final req = UpdateUserSettingsRequest(
      preferredFiatCurrency: preferredFiatCurrency,
    );
    final res = await Result.tryFfiAsync(
      () => this._app.updateUserSettings(req: req),
    );
    if (this.isDisposed) return Err("Already disposed");
    this._isUpdating.value = false;

    switch (res) {
      case Ok(:final ok):
        this._appData.update(
          AppData(preferredFiatCurrency: ok.preferredFiatCurrency),
        );
        return Ok(null);
      case Err(:final err):
        // Undo the optimistic update.
        this._appData.update(AppData(preferredFiatCurrency: rollback));
        error("user-settings: update err: ${err.message}");
        return Err(err.message);
    }
  }

  void dispose() {
    assert(!this.isDisposed);

    this._completed.dispose();
    this._isFetching.dispose();
    this._isUpdating.dispose();

    this.isDisposed = true;
  }
}
