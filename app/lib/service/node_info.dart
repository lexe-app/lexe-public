import 'package:app_rs_dart/ffi/api.dart' show NodeInfo;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/foundation.dart';
import 'package:lexeapp/notifier_ext.dart'
    show AlwaysValueNotifier, LxChangeNotifier;
import 'package:lexeapp/prelude.dart';

/// [AppHandle.nodeInfo] but instrumented with various signals for UI
/// consumption.
class NodeInfoService {
  NodeInfoService({required AppHandle app, void Function(String)? onError})
    : _app = app,
      _onError = onError;

  final AppHandle _app;
  final void Function(String)? _onError;

  bool isDisposed = false;

  /// The most recent [NodeInfo]. `null` if we haven't successfully fetched the
  /// [NodeInfo] yet.
  ValueListenable<NodeInfo?> get nodeInfo => this._nodeInfo;
  final AlwaysValueNotifier<NodeInfo?> _nodeInfo = AlwaysValueNotifier(null);

  /// Notifies after each completed fetch, successful or otherwise.
  Listenable get completed => this._completed;
  final LxChangeNotifier _completed = LxChangeNotifier();

  /// True whenever we're fetching the next nodeInfo.
  ValueListenable<bool> get isFetching => this._isFetching;
  final ValueNotifier<bool> _isFetching = ValueNotifier(false);

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
        this._onError?.call(err.message);
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
