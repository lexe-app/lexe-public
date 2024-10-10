import 'package:app_rs_dart/ffi/api.dart' show NodeInfo;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/foundation.dart';
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/notifier_ext.dart'
    show AlwaysValueNotifier, LxChangeNotifier;
import 'package:lexeapp/result.dart';

/// [AppHandle.nodeInfo] but instrumented with various signals for UI
/// consumption.
class NodeInfoService {
  NodeInfoService({required AppHandle app}) : _app = app;

  final AppHandle _app;

  bool isDisposed = false;

  final AlwaysValueNotifier<NodeInfo?> _nodeInfo = AlwaysValueNotifier(null);
  ValueListenable<NodeInfo?> get nodeInfo => this._nodeInfo;

  /// Notifies after each completed fetch, successful or otherwise.
  final LxChangeNotifier _completed = LxChangeNotifier();
  Listenable get completed => this._completed;

  /// True whenever we're fetching the next nodeInfo.
  final ValueNotifier<bool> _isFetching = ValueNotifier(false);
  ValueListenable<bool> get isFetching => this._isFetching;

  Future<void> fetch() async {
    assert(!this.isDisposed);

    // Skip if we're currently syncing
    if (this._isFetching.value) return;

    // Do sync
    this._isFetching.value = true;
    final res = await Result.tryFfiAsync(this._app.nodeInfo);
    if (this.isDisposed) return;
    this._isFetching.value = false;

    switch (res) {
      case Ok(:final ok):
        info("node-info: $ok");
        this._nodeInfo.value = ok;
      case Err(:final err):
        error("node-info: err: ${err.message}");
    }

    this._completed.notify();
  }

  void dispose() {
    assert(!this.isDisposed);

    this._completed.dispose();
    this._nodeInfo.dispose();
    this._isFetching.dispose();

    this.isDisposed = true;
    // info("node-info: disposed");
  }
}
