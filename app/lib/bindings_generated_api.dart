// AUTO GENERATED FILE, DO NOT EDIT.
// Generated by `flutter_rust_bridge`@ 1.61.1.
// ignore_for_file: non_constant_identifier_names, unused_element, duplicate_ignore, directives_ordering, curly_braces_in_flow_control_structures, unnecessary_lambdas, slash_for_doc_comments, prefer_const_literals_to_create_immutables, implicit_dynamic_list_literal, duplicate_import, unused_import, unnecessary_import, prefer_single_quotes, prefer_const_constructors, use_super_parameters, always_use_package_imports, annotate_overrides, invalid_use_of_protected_member, constant_identifier_names, invalid_use_of_internal_member

import 'dart:convert';
import 'dart:async';
import 'package:meta/meta.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';
import 'package:freezed_annotation/freezed_annotation.dart' hide protected;

part 'bindings_generated_api.freezed.dart';

abstract class AppRs {
  Config regtestStaticMethodConfig({dynamic hint});

  FlutterRustBridgeTaskConstMeta get kRegtestStaticMethodConfigConstMeta;

  Future<AppHandle?> loadStaticMethodAppHandle(
      {required Config config, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kLoadStaticMethodAppHandleConstMeta;

  Future<AppHandle> restoreStaticMethodAppHandle(
      {required Config config, required String seedPhrase, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kRestoreStaticMethodAppHandleConstMeta;

  Future<AppHandle> signupStaticMethodAppHandle(
      {required Config config, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kSignupStaticMethodAppHandleConstMeta;

  Future<NodeInfo> nodeInfoMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kNodeInfoMethodAppHandleConstMeta;

  Future<FiatRate> fiatRateMethodAppHandle(
      {required AppHandle that, required String fiat, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kFiatRateMethodAppHandleConstMeta;

  DropFnType get dropOpaqueApp;
  ShareFnType get shareOpaqueApp;
  OpaqueTypeFinalizer get AppFinalizer;
}

@sealed
class App extends FrbOpaque {
  final AppRs bridge;
  App.fromRaw(int ptr, int size, this.bridge) : super.unsafe(ptr, size);
  @override
  DropFnType get dropFn => bridge.dropOpaqueApp;

  @override
  ShareFnType get shareFn => bridge.shareOpaqueApp;

  @override
  OpaqueTypeFinalizer get staticFinalizer => bridge.AppFinalizer;
}

/// The `AppHandle` is a Dart representation of an [`App`] instance.
class AppHandle {
  final AppRs bridge;
  final App inner;

  AppHandle({
    required this.bridge,
    required this.inner,
  });

  static Future<AppHandle?> load(
          {required AppRs bridge, required Config config, dynamic hint}) =>
      bridge.loadStaticMethodAppHandle(config: config, hint: hint);

  static Future<AppHandle> restore(
          {required AppRs bridge,
          required Config config,
          required String seedPhrase,
          dynamic hint}) =>
      bridge.restoreStaticMethodAppHandle(
          config: config, seedPhrase: seedPhrase, hint: hint);

  static Future<AppHandle> signup(
          {required AppRs bridge, required Config config, dynamic hint}) =>
      bridge.signupStaticMethodAppHandle(config: config, hint: hint);

  Future<NodeInfo> nodeInfo({dynamic hint}) => bridge.nodeInfoMethodAppHandle(
        that: this,
      );

  Future<FiatRate> fiatRate({required String fiat, dynamic hint}) =>
      bridge.fiatRateMethodAppHandle(
        that: this,
        fiat: fiat,
      );
}

/// Dart-serializable configuration we get from the flutter side.
class Config {
  final AppRs bridge;
  final DeployEnv deployEnv;
  final Network network;

  Config({
    required this.bridge,
    required this.deployEnv,
    required this.network,
  });

  static Config regtest({required AppRs bridge, dynamic hint}) =>
      bridge.regtestStaticMethodConfig(hint: hint);
}

enum DeployEnv {
  Prod,
  Staging,
  Dev,
}

@freezed
class FiatRate with _$FiatRate {
  const factory FiatRate({
    required int timestampMs,
    required double rate,
  }) = _FiatRate;
}

enum Network {
  Bitcoin,
  Testnet,
  Regtest,
}

@freezed
class NodeInfo with _$NodeInfo {
  const factory NodeInfo({
    required String nodePk,
    required int localBalanceMsat,
  }) = _NodeInfo;
}
