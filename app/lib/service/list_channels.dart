import 'package:app_rs_dart/ffi/api.dart' show ListChannelsResponse;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/foundation.dart' show ValueListenable, ValueNotifier;
import 'package:lexeapp/notifier_ext.dart';
import 'package:lexeapp/prelude.dart';

/// [AppHandle.listChannels] but instrumented with various signals for UI
/// consumption.
class ListChannelsService {
  ListChannelsService({required AppHandle app}) : _app = app;

  final AppHandle _app;

  bool isDisposed = false;

  /// The most recent [ListChannelsResponse].
  ValueListenable<ListChannelsResponse?> get listChannels => this._listChannels;
  final AlwaysValueNotifier<ListChannelsResponse?> _listChannels =
      AlwaysValueNotifier(null);

  /// True whenever we're fetching the node channels.
  ValueListenable<bool> get isFetching => this._isFetching;
  final ValueNotifier<bool> _isFetching = ValueNotifier(false);

  Future<void> fetch() async {
    assert(!this.isDisposed);

    // Skip if currently fetching
    if (this._isFetching.value) return;

    // List channels
    this._isFetching.value = true;
    final res = await Result.tryFfiAsync(this._app.listChannels);
    if (this.isDisposed) return;
    this._isFetching.value = false;

    switch (res) {
      case Ok(:final ok):
        info("list-channels: n = ${ok.channels.length}");
        this._listChannels.value = ok;
      case Err(:final err):
        error("list-channels: err: ${err.message}");
    }
  }

  void dispose() {
    assert(!this.isDisposed);

    this._isFetching.dispose();
    this._listChannels.dispose();

    this.isDisposed = true;
  }
}
