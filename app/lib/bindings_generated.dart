// AUTO GENERATED FILE, DO NOT EDIT.
// Generated by `flutter_rust_bridge`@ 1.78.0.
// ignore_for_file: non_constant_identifier_names, unused_element, duplicate_ignore, directives_ordering, curly_braces_in_flow_control_structures, unnecessary_lambdas, slash_for_doc_comments, prefer_const_literals_to_create_immutables, implicit_dynamic_list_literal, duplicate_import, unused_import, unnecessary_import, prefer_single_quotes, prefer_const_constructors, use_super_parameters, always_use_package_imports, annotate_overrides, invalid_use_of_protected_member, constant_identifier_names, invalid_use_of_internal_member, prefer_is_empty, unnecessary_const

import "bindings_generated_api.dart";
import 'dart:convert';
import 'dart:async';
import 'package:meta/meta.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';

import 'dart:convert';
import 'dart:async';
import 'package:meta/meta.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';

import 'dart:ffi' as ffi;

class AppRsImpl implements AppRs {
  final AppRsPlatform _platform;
  factory AppRsImpl(ExternalLibrary dylib) =>
      AppRsImpl.raw(AppRsPlatform(dylib));

  /// Only valid on web/WASM platforms.
  factory AppRsImpl.wasm(FutureOr<WasmModule> module) =>
      AppRsImpl(module as ExternalLibrary);
  AppRsImpl.raw(this._platform);
  String? formValidateBitcoinAddress(
      {required String addressStr,
      required Network currentNetwork,
      dynamic hint}) {
    var arg0 = _platform.api2wire_String(addressStr);
    var arg1 = api2wire_network(currentNetwork);
    return _platform.executeSync(FlutterRustBridgeSyncTask(
      callFfi: () =>
          _platform.inner.wire_form_validate_bitcoin_address(arg0, arg1),
      parseSuccessData: _wire2api_opt_String,
      constMeta: kFormValidateBitcoinAddressConstMeta,
      argValues: [addressStr, currentNetwork],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kFormValidateBitcoinAddressConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "form_validate_bitcoin_address",
        argNames: ["addressStr", "currentNetwork"],
      );

  Stream<String> initRustLogStream({required String rustLog, dynamic hint}) {
    var arg0 = _platform.api2wire_String(rustLog);
    return _platform.executeStream(FlutterRustBridgeTask(
      callFfi: (port_) =>
          _platform.inner.wire_init_rust_log_stream(port_, arg0),
      parseSuccessData: _wire2api_String,
      constMeta: kInitRustLogStreamConstMeta,
      argValues: [rustLog],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kInitRustLogStreamConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "init_rust_log_stream",
        argNames: ["rustLog"],
      );

  Future<AppHandle?> loadStaticMethodAppHandle(
      {required Config config, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_config(config);
    return _platform.executeNormal(FlutterRustBridgeTask(
      callFfi: (port_) =>
          _platform.inner.wire_load__static_method__AppHandle(port_, arg0),
      parseSuccessData: _wire2api_opt_box_autoadd_app_handle,
      constMeta: kLoadStaticMethodAppHandleConstMeta,
      argValues: [config],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kLoadStaticMethodAppHandleConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "load__static_method__AppHandle",
        argNames: ["config"],
      );

  Future<AppHandle> restoreStaticMethodAppHandle(
      {required Config config, required String seedPhrase, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_config(config);
    var arg1 = _platform.api2wire_String(seedPhrase);
    return _platform.executeNormal(FlutterRustBridgeTask(
      callFfi: (port_) => _platform.inner
          .wire_restore__static_method__AppHandle(port_, arg0, arg1),
      parseSuccessData: (d) => _wire2api_app_handle(d),
      constMeta: kRestoreStaticMethodAppHandleConstMeta,
      argValues: [config, seedPhrase],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kRestoreStaticMethodAppHandleConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "restore__static_method__AppHandle",
        argNames: ["config", "seedPhrase"],
      );

  Future<AppHandle> signupStaticMethodAppHandle(
      {required Config config, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_config(config);
    return _platform.executeNormal(FlutterRustBridgeTask(
      callFfi: (port_) =>
          _platform.inner.wire_signup__static_method__AppHandle(port_, arg0),
      parseSuccessData: (d) => _wire2api_app_handle(d),
      constMeta: kSignupStaticMethodAppHandleConstMeta,
      argValues: [config],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kSignupStaticMethodAppHandleConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "signup__static_method__AppHandle",
        argNames: ["config"],
      );

  Future<NodeInfo> nodeInfoMethodAppHandle(
      {required AppHandle that, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_app_handle(that);
    return _platform.executeNormal(FlutterRustBridgeTask(
      callFfi: (port_) =>
          _platform.inner.wire_node_info__method__AppHandle(port_, arg0),
      parseSuccessData: _wire2api_node_info,
      constMeta: kNodeInfoMethodAppHandleConstMeta,
      argValues: [that],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kNodeInfoMethodAppHandleConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "node_info__method__AppHandle",
        argNames: ["that"],
      );

  Future<FiatRates> fiatRatesMethodAppHandle(
      {required AppHandle that, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_app_handle(that);
    return _platform.executeNormal(FlutterRustBridgeTask(
      callFfi: (port_) =>
          _platform.inner.wire_fiat_rates__method__AppHandle(port_, arg0),
      parseSuccessData: _wire2api_fiat_rates,
      constMeta: kFiatRatesMethodAppHandleConstMeta,
      argValues: [that],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kFiatRatesMethodAppHandleConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "fiat_rates__method__AppHandle",
        argNames: ["that"],
      );

  Future<bool> syncPaymentsMethodAppHandle(
      {required AppHandle that, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_app_handle(that);
    return _platform.executeNormal(FlutterRustBridgeTask(
      callFfi: (port_) =>
          _platform.inner.wire_sync_payments__method__AppHandle(port_, arg0),
      parseSuccessData: _wire2api_bool,
      constMeta: kSyncPaymentsMethodAppHandleConstMeta,
      argValues: [that],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kSyncPaymentsMethodAppHandleConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "sync_payments__method__AppHandle",
        argNames: ["that"],
      );

  ShortPayment? getPaymentByScrollIdxMethodAppHandle(
      {required AppHandle that, required int scrollIdx, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_app_handle(that);
    var arg1 = api2wire_usize(scrollIdx);
    return _platform.executeSync(FlutterRustBridgeSyncTask(
      callFfi: () => _platform.inner
          .wire_get_payment_by_scroll_idx__method__AppHandle(arg0, arg1),
      parseSuccessData: _wire2api_opt_box_autoadd_short_payment,
      constMeta: kGetPaymentByScrollIdxMethodAppHandleConstMeta,
      argValues: [that, scrollIdx],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta
      get kGetPaymentByScrollIdxMethodAppHandleConstMeta =>
          const FlutterRustBridgeTaskConstMeta(
            debugName: "get_payment_by_scroll_idx__method__AppHandle",
            argNames: ["that", "scrollIdx"],
          );

  ShortPayment? getPendingPaymentByScrollIdxMethodAppHandle(
      {required AppHandle that, required int scrollIdx, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_app_handle(that);
    var arg1 = api2wire_usize(scrollIdx);
    return _platform.executeSync(FlutterRustBridgeSyncTask(
      callFfi: () => _platform.inner
          .wire_get_pending_payment_by_scroll_idx__method__AppHandle(
              arg0, arg1),
      parseSuccessData: _wire2api_opt_box_autoadd_short_payment,
      constMeta: kGetPendingPaymentByScrollIdxMethodAppHandleConstMeta,
      argValues: [that, scrollIdx],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta
      get kGetPendingPaymentByScrollIdxMethodAppHandleConstMeta =>
          const FlutterRustBridgeTaskConstMeta(
            debugName: "get_pending_payment_by_scroll_idx__method__AppHandle",
            argNames: ["that", "scrollIdx"],
          );

  ShortPayment? getFinalizedPaymentByScrollIdxMethodAppHandle(
      {required AppHandle that, required int scrollIdx, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_app_handle(that);
    var arg1 = api2wire_usize(scrollIdx);
    return _platform.executeSync(FlutterRustBridgeSyncTask(
      callFfi: () => _platform.inner
          .wire_get_finalized_payment_by_scroll_idx__method__AppHandle(
              arg0, arg1),
      parseSuccessData: _wire2api_opt_box_autoadd_short_payment,
      constMeta: kGetFinalizedPaymentByScrollIdxMethodAppHandleConstMeta,
      argValues: [that, scrollIdx],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta
      get kGetFinalizedPaymentByScrollIdxMethodAppHandleConstMeta =>
          const FlutterRustBridgeTaskConstMeta(
            debugName: "get_finalized_payment_by_scroll_idx__method__AppHandle",
            argNames: ["that", "scrollIdx"],
          );

  int getNumPaymentsMethodAppHandle({required AppHandle that, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_app_handle(that);
    return _platform.executeSync(FlutterRustBridgeSyncTask(
      callFfi: () =>
          _platform.inner.wire_get_num_payments__method__AppHandle(arg0),
      parseSuccessData: _wire2api_usize,
      constMeta: kGetNumPaymentsMethodAppHandleConstMeta,
      argValues: [that],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kGetNumPaymentsMethodAppHandleConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "get_num_payments__method__AppHandle",
        argNames: ["that"],
      );

  int getNumPendingPaymentsMethodAppHandle(
      {required AppHandle that, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_app_handle(that);
    return _platform.executeSync(FlutterRustBridgeSyncTask(
      callFfi: () => _platform.inner
          .wire_get_num_pending_payments__method__AppHandle(arg0),
      parseSuccessData: _wire2api_usize,
      constMeta: kGetNumPendingPaymentsMethodAppHandleConstMeta,
      argValues: [that],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta
      get kGetNumPendingPaymentsMethodAppHandleConstMeta =>
          const FlutterRustBridgeTaskConstMeta(
            debugName: "get_num_pending_payments__method__AppHandle",
            argNames: ["that"],
          );

  int getNumFinalizedPaymentsMethodAppHandle(
      {required AppHandle that, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_app_handle(that);
    return _platform.executeSync(FlutterRustBridgeSyncTask(
      callFfi: () => _platform.inner
          .wire_get_num_finalized_payments__method__AppHandle(arg0),
      parseSuccessData: _wire2api_usize,
      constMeta: kGetNumFinalizedPaymentsMethodAppHandleConstMeta,
      argValues: [that],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta
      get kGetNumFinalizedPaymentsMethodAppHandleConstMeta =>
          const FlutterRustBridgeTaskConstMeta(
            debugName: "get_num_finalized_payments__method__AppHandle",
            argNames: ["that"],
          );

  DropFnType get dropOpaqueApp => _platform.inner.drop_opaque_App;
  ShareFnType get shareOpaqueApp => _platform.inner.share_opaque_App;
  OpaqueTypeFinalizer get AppFinalizer => _platform.AppFinalizer;

  void dispose() {
    _platform.dispose();
  }
// Section: wire2api

  App _wire2api_App(dynamic raw) {
    return App.fromRaw(raw[0], raw[1], this);
  }

  String _wire2api_String(dynamic raw) {
    return raw as String;
  }

  AppHandle _wire2api_app_handle(dynamic raw) {
    final arr = raw as List<dynamic>;
    if (arr.length != 1)
      throw Exception('unexpected arr length: expect 1 but see ${arr.length}');
    return AppHandle(
      bridge: this,
      inner: _wire2api_App(arr[0]),
    );
  }

  bool _wire2api_bool(dynamic raw) {
    return raw as bool;
  }

  AppHandle _wire2api_box_autoadd_app_handle(dynamic raw) {
    return _wire2api_app_handle(raw);
  }

  ShortPayment _wire2api_box_autoadd_short_payment(dynamic raw) {
    return _wire2api_short_payment(raw);
  }

  int _wire2api_box_autoadd_u64(dynamic raw) {
    return _wire2api_u64(raw);
  }

  double _wire2api_f64(dynamic raw) {
    return raw as double;
  }

  FiatRate _wire2api_fiat_rate(dynamic raw) {
    final arr = raw as List<dynamic>;
    if (arr.length != 2)
      throw Exception('unexpected arr length: expect 2 but see ${arr.length}');
    return FiatRate(
      fiat: _wire2api_String(arr[0]),
      rate: _wire2api_f64(arr[1]),
    );
  }

  FiatRates _wire2api_fiat_rates(dynamic raw) {
    final arr = raw as List<dynamic>;
    if (arr.length != 2)
      throw Exception('unexpected arr length: expect 2 but see ${arr.length}');
    return FiatRates(
      timestampMs: _wire2api_i64(arr[0]),
      rates: _wire2api_list_fiat_rate(arr[1]),
    );
  }

  int _wire2api_i32(dynamic raw) {
    return raw as int;
  }

  int _wire2api_i64(dynamic raw) {
    return castInt(raw);
  }

  List<FiatRate> _wire2api_list_fiat_rate(dynamic raw) {
    return (raw as List<dynamic>).map(_wire2api_fiat_rate).toList();
  }

  NodeInfo _wire2api_node_info(dynamic raw) {
    final arr = raw as List<dynamic>;
    if (arr.length != 2)
      throw Exception('unexpected arr length: expect 2 but see ${arr.length}');
    return NodeInfo(
      nodePk: _wire2api_String(arr[0]),
      localBalanceMsat: _wire2api_u64(arr[1]),
    );
  }

  String? _wire2api_opt_String(dynamic raw) {
    return raw == null ? null : _wire2api_String(raw);
  }

  AppHandle? _wire2api_opt_box_autoadd_app_handle(dynamic raw) {
    return raw == null ? null : _wire2api_box_autoadd_app_handle(raw);
  }

  ShortPayment? _wire2api_opt_box_autoadd_short_payment(dynamic raw) {
    return raw == null ? null : _wire2api_box_autoadd_short_payment(raw);
  }

  int? _wire2api_opt_box_autoadd_u64(dynamic raw) {
    return raw == null ? null : _wire2api_box_autoadd_u64(raw);
  }

  PaymentDirection _wire2api_payment_direction(dynamic raw) {
    return PaymentDirection.values[raw as int];
  }

  PaymentKind _wire2api_payment_kind(dynamic raw) {
    return PaymentKind.values[raw as int];
  }

  PaymentStatus _wire2api_payment_status(dynamic raw) {
    return PaymentStatus.values[raw as int];
  }

  ShortPayment _wire2api_short_payment(dynamic raw) {
    final arr = raw as List<dynamic>;
    if (arr.length != 7)
      throw Exception('unexpected arr length: expect 7 but see ${arr.length}');
    return ShortPayment(
      index: _wire2api_String(arr[0]),
      kind: _wire2api_payment_kind(arr[1]),
      direction: _wire2api_payment_direction(arr[2]),
      amountSat: _wire2api_opt_box_autoadd_u64(arr[3]),
      status: _wire2api_payment_status(arr[4]),
      note: _wire2api_opt_String(arr[5]),
      createdAt: _wire2api_i64(arr[6]),
    );
  }

  int _wire2api_u64(dynamic raw) {
    return castInt(raw);
  }

  int _wire2api_u8(dynamic raw) {
    return raw as int;
  }

  Uint8List _wire2api_uint_8_list(dynamic raw) {
    return raw as Uint8List;
  }

  int _wire2api_usize(dynamic raw) {
    return castInt(raw);
  }
}

// Section: api2wire

@protected
bool api2wire_bool(bool raw) {
  return raw;
}

@protected
int api2wire_deploy_env(DeployEnv raw) {
  return api2wire_i32(raw.index);
}

@protected
int api2wire_i32(int raw) {
  return raw;
}

@protected
int api2wire_network(Network raw) {
  return api2wire_i32(raw.index);
}

@protected
int api2wire_u8(int raw) {
  return raw;
}

@protected
int api2wire_usize(int raw) {
  return raw;
}
// Section: finalizer

class AppRsPlatform extends FlutterRustBridgeBase<AppRsWire> {
  AppRsPlatform(ffi.DynamicLibrary dylib) : super(AppRsWire(dylib));

// Section: api2wire

  @protected
  wire_App api2wire_App(App raw) {
    final ptr = inner.new_App();
    _api_fill_to_wire_App(raw, ptr);
    return ptr;
  }

  @protected
  ffi.Pointer<wire_uint_8_list> api2wire_String(String raw) {
    return api2wire_uint_8_list(utf8.encoder.convert(raw));
  }

  @protected
  ffi.Pointer<wire_AppHandle> api2wire_box_autoadd_app_handle(AppHandle raw) {
    final ptr = inner.new_box_autoadd_app_handle_0();
    _api_fill_to_wire_app_handle(raw, ptr.ref);
    return ptr;
  }

  @protected
  ffi.Pointer<wire_Config> api2wire_box_autoadd_config(Config raw) {
    final ptr = inner.new_box_autoadd_config_0();
    _api_fill_to_wire_config(raw, ptr.ref);
    return ptr;
  }

  @protected
  ffi.Pointer<wire_uint_8_list> api2wire_uint_8_list(Uint8List raw) {
    final ans = inner.new_uint_8_list_0(raw.length);
    ans.ref.ptr.asTypedList(raw.length).setAll(0, raw);
    return ans;
  }

// Section: finalizer

  late final OpaqueTypeFinalizer _AppFinalizer =
      OpaqueTypeFinalizer(inner._drop_opaque_AppPtr);
  OpaqueTypeFinalizer get AppFinalizer => _AppFinalizer;
// Section: api_fill_to_wire

  void _api_fill_to_wire_App(App apiObj, wire_App wireObj) {
    wireObj.ptr = apiObj.shareOrMove();
  }

  void _api_fill_to_wire_app_handle(AppHandle apiObj, wire_AppHandle wireObj) {
    wireObj.inner = api2wire_App(apiObj.inner);
  }

  void _api_fill_to_wire_box_autoadd_app_handle(
      AppHandle apiObj, ffi.Pointer<wire_AppHandle> wireObj) {
    _api_fill_to_wire_app_handle(apiObj, wireObj.ref);
  }

  void _api_fill_to_wire_box_autoadd_config(
      Config apiObj, ffi.Pointer<wire_Config> wireObj) {
    _api_fill_to_wire_config(apiObj, wireObj.ref);
  }

  void _api_fill_to_wire_config(Config apiObj, wire_Config wireObj) {
    wireObj.deploy_env = api2wire_deploy_env(apiObj.deployEnv);
    wireObj.network = api2wire_network(apiObj.network);
    wireObj.gateway_url = api2wire_String(apiObj.gatewayUrl);
    wireObj.use_sgx = api2wire_bool(apiObj.useSgx);
    wireObj.app_data_dir = api2wire_String(apiObj.appDataDir);
    wireObj.use_mock_secret_store = api2wire_bool(apiObj.useMockSecretStore);
  }
}

// ignore_for_file: camel_case_types, non_constant_identifier_names, avoid_positional_boolean_parameters, annotate_overrides, constant_identifier_names

// AUTO GENERATED FILE, DO NOT EDIT.
//
// Generated by `package:ffigen`.
// ignore_for_file: type=lint

/// generated by flutter_rust_bridge
class AppRsWire implements FlutterRustBridgeWireBase {
  @internal
  late final dartApi = DartApiDl(init_frb_dart_api_dl);

  /// Holds the symbol lookup function.
  final ffi.Pointer<T> Function<T extends ffi.NativeType>(String symbolName)
      _lookup;

  /// The symbols are looked up in [dynamicLibrary].
  AppRsWire(ffi.DynamicLibrary dynamicLibrary)
      : _lookup = dynamicLibrary.lookup;

  /// The symbols are looked up with [lookup].
  AppRsWire.fromLookup(
      ffi.Pointer<T> Function<T extends ffi.NativeType>(String symbolName)
          lookup)
      : _lookup = lookup;

  void store_dart_post_cobject(
    DartPostCObjectFnType ptr,
  ) {
    return _store_dart_post_cobject(
      ptr,
    );
  }

  late final _store_dart_post_cobjectPtr =
      _lookup<ffi.NativeFunction<ffi.Void Function(DartPostCObjectFnType)>>(
          'store_dart_post_cobject');
  late final _store_dart_post_cobject = _store_dart_post_cobjectPtr
      .asFunction<void Function(DartPostCObjectFnType)>();

  Object get_dart_object(
    int ptr,
  ) {
    return _get_dart_object(
      ptr,
    );
  }

  late final _get_dart_objectPtr =
      _lookup<ffi.NativeFunction<ffi.Handle Function(ffi.UintPtr)>>(
          'get_dart_object');
  late final _get_dart_object =
      _get_dart_objectPtr.asFunction<Object Function(int)>();

  void drop_dart_object(
    int ptr,
  ) {
    return _drop_dart_object(
      ptr,
    );
  }

  late final _drop_dart_objectPtr =
      _lookup<ffi.NativeFunction<ffi.Void Function(ffi.UintPtr)>>(
          'drop_dart_object');
  late final _drop_dart_object =
      _drop_dart_objectPtr.asFunction<void Function(int)>();

  int new_dart_opaque(
    Object handle,
  ) {
    return _new_dart_opaque(
      handle,
    );
  }

  late final _new_dart_opaquePtr =
      _lookup<ffi.NativeFunction<ffi.UintPtr Function(ffi.Handle)>>(
          'new_dart_opaque');
  late final _new_dart_opaque =
      _new_dart_opaquePtr.asFunction<int Function(Object)>();

  int init_frb_dart_api_dl(
    ffi.Pointer<ffi.Void> obj,
  ) {
    return _init_frb_dart_api_dl(
      obj,
    );
  }

  late final _init_frb_dart_api_dlPtr =
      _lookup<ffi.NativeFunction<ffi.IntPtr Function(ffi.Pointer<ffi.Void>)>>(
          'init_frb_dart_api_dl');
  late final _init_frb_dart_api_dl = _init_frb_dart_api_dlPtr
      .asFunction<int Function(ffi.Pointer<ffi.Void>)>();

  WireSyncReturn wire_form_validate_bitcoin_address(
    ffi.Pointer<wire_uint_8_list> address_str,
    int current_network,
  ) {
    return _wire_form_validate_bitcoin_address(
      address_str,
      current_network,
    );
  }

  late final _wire_form_validate_bitcoin_addressPtr = _lookup<
      ffi.NativeFunction<
          WireSyncReturn Function(ffi.Pointer<wire_uint_8_list>,
              ffi.Int32)>>('wire_form_validate_bitcoin_address');
  late final _wire_form_validate_bitcoin_address =
      _wire_form_validate_bitcoin_addressPtr.asFunction<
          WireSyncReturn Function(ffi.Pointer<wire_uint_8_list>, int)>();

  void wire_init_rust_log_stream(
    int port_,
    ffi.Pointer<wire_uint_8_list> rust_log,
  ) {
    return _wire_init_rust_log_stream(
      port_,
      rust_log,
    );
  }

  late final _wire_init_rust_log_streamPtr = _lookup<
      ffi.NativeFunction<
          ffi.Void Function(ffi.Int64,
              ffi.Pointer<wire_uint_8_list>)>>('wire_init_rust_log_stream');
  late final _wire_init_rust_log_stream = _wire_init_rust_log_streamPtr
      .asFunction<void Function(int, ffi.Pointer<wire_uint_8_list>)>();

  void wire_load__static_method__AppHandle(
    int port_,
    ffi.Pointer<wire_Config> config,
  ) {
    return _wire_load__static_method__AppHandle(
      port_,
      config,
    );
  }

  late final _wire_load__static_method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              ffi.Void Function(ffi.Int64, ffi.Pointer<wire_Config>)>>(
      'wire_load__static_method__AppHandle');
  late final _wire_load__static_method__AppHandle =
      _wire_load__static_method__AppHandlePtr
          .asFunction<void Function(int, ffi.Pointer<wire_Config>)>();

  void wire_restore__static_method__AppHandle(
    int port_,
    ffi.Pointer<wire_Config> config,
    ffi.Pointer<wire_uint_8_list> seed_phrase,
  ) {
    return _wire_restore__static_method__AppHandle(
      port_,
      config,
      seed_phrase,
    );
  }

  late final _wire_restore__static_method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              ffi.Void Function(ffi.Int64, ffi.Pointer<wire_Config>,
                  ffi.Pointer<wire_uint_8_list>)>>(
      'wire_restore__static_method__AppHandle');
  late final _wire_restore__static_method__AppHandle =
      _wire_restore__static_method__AppHandlePtr.asFunction<
          void Function(
              int, ffi.Pointer<wire_Config>, ffi.Pointer<wire_uint_8_list>)>();

  void wire_signup__static_method__AppHandle(
    int port_,
    ffi.Pointer<wire_Config> config,
  ) {
    return _wire_signup__static_method__AppHandle(
      port_,
      config,
    );
  }

  late final _wire_signup__static_method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              ffi.Void Function(ffi.Int64, ffi.Pointer<wire_Config>)>>(
      'wire_signup__static_method__AppHandle');
  late final _wire_signup__static_method__AppHandle =
      _wire_signup__static_method__AppHandlePtr
          .asFunction<void Function(int, ffi.Pointer<wire_Config>)>();

  void wire_node_info__method__AppHandle(
    int port_,
    ffi.Pointer<wire_AppHandle> that,
  ) {
    return _wire_node_info__method__AppHandle(
      port_,
      that,
    );
  }

  late final _wire_node_info__method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              ffi.Void Function(ffi.Int64, ffi.Pointer<wire_AppHandle>)>>(
      'wire_node_info__method__AppHandle');
  late final _wire_node_info__method__AppHandle =
      _wire_node_info__method__AppHandlePtr
          .asFunction<void Function(int, ffi.Pointer<wire_AppHandle>)>();

  void wire_fiat_rates__method__AppHandle(
    int port_,
    ffi.Pointer<wire_AppHandle> that,
  ) {
    return _wire_fiat_rates__method__AppHandle(
      port_,
      that,
    );
  }

  late final _wire_fiat_rates__method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              ffi.Void Function(ffi.Int64, ffi.Pointer<wire_AppHandle>)>>(
      'wire_fiat_rates__method__AppHandle');
  late final _wire_fiat_rates__method__AppHandle =
      _wire_fiat_rates__method__AppHandlePtr
          .asFunction<void Function(int, ffi.Pointer<wire_AppHandle>)>();

  void wire_sync_payments__method__AppHandle(
    int port_,
    ffi.Pointer<wire_AppHandle> that,
  ) {
    return _wire_sync_payments__method__AppHandle(
      port_,
      that,
    );
  }

  late final _wire_sync_payments__method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              ffi.Void Function(ffi.Int64, ffi.Pointer<wire_AppHandle>)>>(
      'wire_sync_payments__method__AppHandle');
  late final _wire_sync_payments__method__AppHandle =
      _wire_sync_payments__method__AppHandlePtr
          .asFunction<void Function(int, ffi.Pointer<wire_AppHandle>)>();

  WireSyncReturn wire_get_payment_by_scroll_idx__method__AppHandle(
    ffi.Pointer<wire_AppHandle> that,
    int scroll_idx,
  ) {
    return _wire_get_payment_by_scroll_idx__method__AppHandle(
      that,
      scroll_idx,
    );
  }

  late final _wire_get_payment_by_scroll_idx__method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              WireSyncReturn Function(
                  ffi.Pointer<wire_AppHandle>, ffi.UintPtr)>>(
      'wire_get_payment_by_scroll_idx__method__AppHandle');
  late final _wire_get_payment_by_scroll_idx__method__AppHandle =
      _wire_get_payment_by_scroll_idx__method__AppHandlePtr.asFunction<
          WireSyncReturn Function(ffi.Pointer<wire_AppHandle>, int)>();

  WireSyncReturn wire_get_pending_payment_by_scroll_idx__method__AppHandle(
    ffi.Pointer<wire_AppHandle> that,
    int scroll_idx,
  ) {
    return _wire_get_pending_payment_by_scroll_idx__method__AppHandle(
      that,
      scroll_idx,
    );
  }

  late final _wire_get_pending_payment_by_scroll_idx__method__AppHandlePtr =
      _lookup<
              ffi.NativeFunction<
                  WireSyncReturn Function(
                      ffi.Pointer<wire_AppHandle>, ffi.UintPtr)>>(
          'wire_get_pending_payment_by_scroll_idx__method__AppHandle');
  late final _wire_get_pending_payment_by_scroll_idx__method__AppHandle =
      _wire_get_pending_payment_by_scroll_idx__method__AppHandlePtr.asFunction<
          WireSyncReturn Function(ffi.Pointer<wire_AppHandle>, int)>();

  WireSyncReturn wire_get_finalized_payment_by_scroll_idx__method__AppHandle(
    ffi.Pointer<wire_AppHandle> that,
    int scroll_idx,
  ) {
    return _wire_get_finalized_payment_by_scroll_idx__method__AppHandle(
      that,
      scroll_idx,
    );
  }

  late final _wire_get_finalized_payment_by_scroll_idx__method__AppHandlePtr =
      _lookup<
              ffi.NativeFunction<
                  WireSyncReturn Function(
                      ffi.Pointer<wire_AppHandle>, ffi.UintPtr)>>(
          'wire_get_finalized_payment_by_scroll_idx__method__AppHandle');
  late final _wire_get_finalized_payment_by_scroll_idx__method__AppHandle =
      _wire_get_finalized_payment_by_scroll_idx__method__AppHandlePtr
          .asFunction<
              WireSyncReturn Function(ffi.Pointer<wire_AppHandle>, int)>();

  WireSyncReturn wire_get_num_payments__method__AppHandle(
    ffi.Pointer<wire_AppHandle> that,
  ) {
    return _wire_get_num_payments__method__AppHandle(
      that,
    );
  }

  late final _wire_get_num_payments__method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              WireSyncReturn Function(ffi.Pointer<wire_AppHandle>)>>(
      'wire_get_num_payments__method__AppHandle');
  late final _wire_get_num_payments__method__AppHandle =
      _wire_get_num_payments__method__AppHandlePtr
          .asFunction<WireSyncReturn Function(ffi.Pointer<wire_AppHandle>)>();

  WireSyncReturn wire_get_num_pending_payments__method__AppHandle(
    ffi.Pointer<wire_AppHandle> that,
  ) {
    return _wire_get_num_pending_payments__method__AppHandle(
      that,
    );
  }

  late final _wire_get_num_pending_payments__method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              WireSyncReturn Function(ffi.Pointer<wire_AppHandle>)>>(
      'wire_get_num_pending_payments__method__AppHandle');
  late final _wire_get_num_pending_payments__method__AppHandle =
      _wire_get_num_pending_payments__method__AppHandlePtr
          .asFunction<WireSyncReturn Function(ffi.Pointer<wire_AppHandle>)>();

  WireSyncReturn wire_get_num_finalized_payments__method__AppHandle(
    ffi.Pointer<wire_AppHandle> that,
  ) {
    return _wire_get_num_finalized_payments__method__AppHandle(
      that,
    );
  }

  late final _wire_get_num_finalized_payments__method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              WireSyncReturn Function(ffi.Pointer<wire_AppHandle>)>>(
      'wire_get_num_finalized_payments__method__AppHandle');
  late final _wire_get_num_finalized_payments__method__AppHandle =
      _wire_get_num_finalized_payments__method__AppHandlePtr
          .asFunction<WireSyncReturn Function(ffi.Pointer<wire_AppHandle>)>();

  wire_App new_App() {
    return _new_App();
  }

  late final _new_AppPtr =
      _lookup<ffi.NativeFunction<wire_App Function()>>('new_App');
  late final _new_App = _new_AppPtr.asFunction<wire_App Function()>();

  ffi.Pointer<wire_AppHandle> new_box_autoadd_app_handle_0() {
    return _new_box_autoadd_app_handle_0();
  }

  late final _new_box_autoadd_app_handle_0Ptr =
      _lookup<ffi.NativeFunction<ffi.Pointer<wire_AppHandle> Function()>>(
          'new_box_autoadd_app_handle_0');
  late final _new_box_autoadd_app_handle_0 = _new_box_autoadd_app_handle_0Ptr
      .asFunction<ffi.Pointer<wire_AppHandle> Function()>();

  ffi.Pointer<wire_Config> new_box_autoadd_config_0() {
    return _new_box_autoadd_config_0();
  }

  late final _new_box_autoadd_config_0Ptr =
      _lookup<ffi.NativeFunction<ffi.Pointer<wire_Config> Function()>>(
          'new_box_autoadd_config_0');
  late final _new_box_autoadd_config_0 = _new_box_autoadd_config_0Ptr
      .asFunction<ffi.Pointer<wire_Config> Function()>();

  ffi.Pointer<wire_uint_8_list> new_uint_8_list_0(
    int len,
  ) {
    return _new_uint_8_list_0(
      len,
    );
  }

  late final _new_uint_8_list_0Ptr = _lookup<
      ffi.NativeFunction<
          ffi.Pointer<wire_uint_8_list> Function(
              ffi.Int32)>>('new_uint_8_list_0');
  late final _new_uint_8_list_0 = _new_uint_8_list_0Ptr
      .asFunction<ffi.Pointer<wire_uint_8_list> Function(int)>();

  void drop_opaque_App(
    ffi.Pointer<ffi.Void> ptr,
  ) {
    return _drop_opaque_App(
      ptr,
    );
  }

  late final _drop_opaque_AppPtr =
      _lookup<ffi.NativeFunction<ffi.Void Function(ffi.Pointer<ffi.Void>)>>(
          'drop_opaque_App');
  late final _drop_opaque_App =
      _drop_opaque_AppPtr.asFunction<void Function(ffi.Pointer<ffi.Void>)>();

  ffi.Pointer<ffi.Void> share_opaque_App(
    ffi.Pointer<ffi.Void> ptr,
  ) {
    return _share_opaque_App(
      ptr,
    );
  }

  late final _share_opaque_AppPtr = _lookup<
      ffi.NativeFunction<
          ffi.Pointer<ffi.Void> Function(
              ffi.Pointer<ffi.Void>)>>('share_opaque_App');
  late final _share_opaque_App = _share_opaque_AppPtr
      .asFunction<ffi.Pointer<ffi.Void> Function(ffi.Pointer<ffi.Void>)>();

  void free_WireSyncReturn(
    WireSyncReturn ptr,
  ) {
    return _free_WireSyncReturn(
      ptr,
    );
  }

  late final _free_WireSyncReturnPtr =
      _lookup<ffi.NativeFunction<ffi.Void Function(WireSyncReturn)>>(
          'free_WireSyncReturn');
  late final _free_WireSyncReturn =
      _free_WireSyncReturnPtr.asFunction<void Function(WireSyncReturn)>();
}

final class _Dart_Handle extends ffi.Opaque {}

final class wire_uint_8_list extends ffi.Struct {
  external ffi.Pointer<ffi.Uint8> ptr;

  @ffi.Int32()
  external int len;
}

final class wire_Config extends ffi.Struct {
  @ffi.Int32()
  external int deploy_env;

  @ffi.Int32()
  external int network;

  external ffi.Pointer<wire_uint_8_list> gateway_url;

  @ffi.Bool()
  external bool use_sgx;

  external ffi.Pointer<wire_uint_8_list> app_data_dir;

  @ffi.Bool()
  external bool use_mock_secret_store;
}

final class wire_App extends ffi.Struct {
  external ffi.Pointer<ffi.Void> ptr;
}

final class wire_AppHandle extends ffi.Struct {
  external wire_App inner;
}

typedef DartPostCObjectFnType = ffi.Pointer<
    ffi.NativeFunction<
        ffi.Bool Function(DartPort port_id, ffi.Pointer<ffi.Void> message)>>;
typedef DartPort = ffi.Int64;
