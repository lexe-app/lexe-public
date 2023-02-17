// AUTO GENERATED FILE, DO NOT EDIT.
// Generated by `flutter_rust_bridge`@ 1.61.1.
// ignore_for_file: non_constant_identifier_names, unused_element, duplicate_ignore, directives_ordering, curly_braces_in_flow_control_structures, unnecessary_lambdas, slash_for_doc_comments, prefer_const_literals_to_create_immutables, implicit_dynamic_list_literal, duplicate_import, unused_import, unnecessary_import, prefer_single_quotes, prefer_const_constructors, use_super_parameters, always_use_package_imports, annotate_overrides, invalid_use_of_protected_member, constant_identifier_names, invalid_use_of_internal_member

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
  Config regtestStaticMethodConfig({dynamic hint}) {
    return _platform.executeSync(FlutterRustBridgeSyncTask(
      callFfi: () => _platform.inner.wire_regtest__static_method__Config(),
      parseSuccessData: _wire2api_config,
      constMeta: kRegtestStaticMethodConfigConstMeta,
      argValues: [],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kRegtestStaticMethodConfigConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "regtest__static_method__Config",
        argNames: [],
      );

  Future<void> testMethodMethodAppHandle(
      {required AppHandle that, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_app_handle(that);
    return _platform.executeNormal(FlutterRustBridgeTask(
      callFfi: (port_) =>
          _platform.inner.wire_test_method__method__AppHandle(port_, arg0),
      parseSuccessData: _wire2api_unit,
      constMeta: kTestMethodMethodAppHandleConstMeta,
      argValues: [that],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kTestMethodMethodAppHandleConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "test_method__method__AppHandle",
        argNames: ["that"],
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

  Future<AppHandle> recoverStaticMethodAppHandle(
      {required Config config, required String seedPhrase, dynamic hint}) {
    var arg0 = _platform.api2wire_box_autoadd_config(config);
    var arg1 = _platform.api2wire_String(seedPhrase);
    return _platform.executeNormal(FlutterRustBridgeTask(
      callFfi: (port_) => _platform.inner
          .wire_recover__static_method__AppHandle(port_, arg0, arg1),
      parseSuccessData: (d) => _wire2api_app_handle(d),
      constMeta: kRecoverStaticMethodAppHandleConstMeta,
      argValues: [config, seedPhrase],
      hint: hint,
    ));
  }

  FlutterRustBridgeTaskConstMeta get kRecoverStaticMethodAppHandleConstMeta =>
      const FlutterRustBridgeTaskConstMeta(
        debugName: "recover__static_method__AppHandle",
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

  void dispose() {
    _platform.dispose();
  }
// Section: wire2api

  AppHandle _wire2api_app_handle(dynamic raw) {
    final arr = raw as List<dynamic>;
    if (arr.length != 1)
      throw Exception('unexpected arr length: expect 1 but see ${arr.length}');
    return AppHandle(
      bridge: this,
      instanceId: _wire2api_i32(arr[0]),
    );
  }

  AppHandle _wire2api_box_autoadd_app_handle(dynamic raw) {
    return _wire2api_app_handle(raw);
  }

  BuildVariant _wire2api_build_variant(dynamic raw) {
    return BuildVariant.values[raw];
  }

  Config _wire2api_config(dynamic raw) {
    final arr = raw as List<dynamic>;
    if (arr.length != 2)
      throw Exception('unexpected arr length: expect 2 but see ${arr.length}');
    return Config(
      bridge: this,
      buildVariant: _wire2api_build_variant(arr[0]),
      network: _wire2api_network(arr[1]),
    );
  }

  int _wire2api_i32(dynamic raw) {
    return raw as int;
  }

  Network _wire2api_network(dynamic raw) {
    return Network.values[raw];
  }

  AppHandle? _wire2api_opt_box_autoadd_app_handle(dynamic raw) {
    return raw == null ? null : _wire2api_box_autoadd_app_handle(raw);
  }

  void _wire2api_unit(dynamic raw) {
    return;
  }
}

// Section: api2wire

@protected
int api2wire_build_variant(BuildVariant raw) {
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

// Section: finalizer

class AppRsPlatform extends FlutterRustBridgeBase<AppRsWire> {
  AppRsPlatform(ffi.DynamicLibrary dylib) : super(AppRsWire(dylib));

// Section: api2wire

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

// Section: api_fill_to_wire

  void _api_fill_to_wire_app_handle(AppHandle apiObj, wire_AppHandle wireObj) {
    wireObj.instance_id = api2wire_i32(apiObj.instanceId);
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
    wireObj.build_variant = api2wire_build_variant(apiObj.buildVariant);
    wireObj.network = api2wire_network(apiObj.network);
  }
}

// ignore_for_file: camel_case_types, non_constant_identifier_names, avoid_positional_boolean_parameters, annotate_overrides, constant_identifier_names

// AUTO GENERATED FILE, DO NOT EDIT.
//
// Generated by `package:ffigen`.

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

  WireSyncReturn wire_regtest__static_method__Config() {
    return _wire_regtest__static_method__Config();
  }

  late final _wire_regtest__static_method__ConfigPtr =
      _lookup<ffi.NativeFunction<WireSyncReturn Function()>>(
          'wire_regtest__static_method__Config');
  late final _wire_regtest__static_method__Config =
      _wire_regtest__static_method__ConfigPtr
          .asFunction<WireSyncReturn Function()>();

  void wire_test_method__method__AppHandle(
    int port_,
    ffi.Pointer<wire_AppHandle> that,
  ) {
    return _wire_test_method__method__AppHandle(
      port_,
      that,
    );
  }

  late final _wire_test_method__method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              ffi.Void Function(ffi.Int64, ffi.Pointer<wire_AppHandle>)>>(
      'wire_test_method__method__AppHandle');
  late final _wire_test_method__method__AppHandle =
      _wire_test_method__method__AppHandlePtr
          .asFunction<void Function(int, ffi.Pointer<wire_AppHandle>)>();

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

  void wire_recover__static_method__AppHandle(
    int port_,
    ffi.Pointer<wire_Config> config,
    ffi.Pointer<wire_uint_8_list> seed_phrase,
  ) {
    return _wire_recover__static_method__AppHandle(
      port_,
      config,
      seed_phrase,
    );
  }

  late final _wire_recover__static_method__AppHandlePtr = _lookup<
          ffi.NativeFunction<
              ffi.Void Function(ffi.Int64, ffi.Pointer<wire_Config>,
                  ffi.Pointer<wire_uint_8_list>)>>(
      'wire_recover__static_method__AppHandle');
  late final _wire_recover__static_method__AppHandle =
      _wire_recover__static_method__AppHandlePtr.asFunction<
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

class _Dart_Handle extends ffi.Opaque {}

class wire_AppHandle extends ffi.Struct {
  @ffi.Int32()
  external int instance_id;
}

class wire_Config extends ffi.Struct {
  @ffi.Int32()
  external int build_variant;

  @ffi.Int32()
  external int network;
}

class wire_uint_8_list extends ffi.Struct {
  external ffi.Pointer<ffi.Uint8> ptr;

  @ffi.Int32()
  external int len;
}

typedef DartPostCObjectFnType = ffi.Pointer<
    ffi.NativeFunction<ffi.Bool Function(DartPort, ffi.Pointer<ffi.Void>)>>;
typedef DartPort = ffi.Int64;
