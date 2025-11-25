import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show RevocableClient;
import 'package:flutter/foundation.dart' show ValueListenable, ValueNotifier;
import 'package:lexeapp/prelude.dart';

/// [RevocableClient] CRUD APIs but instrumented with various signals for UI
/// consumption.
class ClientsService {
  ClientsService({required AppHandle app}) : _app = app;

  final AppHandle _app;

  bool isDisposed = false;

  /// The most recent clients list result.
  ValueListenable<FfiResult<List<RevocableClient>>> get clients =>
      this._clients;
  final ValueNotifier<FfiResult<List<RevocableClient>>> _clients =
      ValueNotifier(const Ok(<RevocableClient>[]));

  /// True whenever we're fetching the active clients.
  ValueListenable<bool> get isFetching => this._isFetching;
  final ValueNotifier<bool> _isFetching = ValueNotifier(false);

  Future<void> fetch() async {
    assert(!this.isDisposed);

    // Skip if currently fetching
    if (this._isFetching.value) return;

    // List clients
    this._isFetching.value = true;
    final res = await Result.tryFfiAsync(this._app.listClients);
    if (this.isDisposed) return;
    this._isFetching.value = false;

    switch (res) {
      case Ok(:final ok):
        info("list-clients: n = ${ok.length}");
        // sort clients by creation date (newest first)
        ok.sort((c1, c2) => c2.createdAt.compareTo(c1.createdAt));
      case Err(:final err):
        error("list-channels: err: ${err.message}");
    }
    this._clients.value = res;
  }

  void dispose() {
    assert(!this.isDisposed);

    this._isFetching.dispose();
    this._clients.dispose();

    this.isDisposed = true;
  }
}
