// AUTO GENERATED FILE, DO NOT EDIT.
// Generated by `flutter_rust_bridge`@ 1.61.1.
// ignore_for_file: non_constant_identifier_names, unused_element, duplicate_ignore, directives_ordering, curly_braces_in_flow_control_structures, unnecessary_lambdas, slash_for_doc_comments, prefer_const_literals_to_create_immutables, implicit_dynamic_list_literal, duplicate_import, unused_import, unnecessary_import, prefer_single_quotes, prefer_const_constructors, use_super_parameters, always_use_package_imports, annotate_overrides, invalid_use_of_protected_member, constant_identifier_names, invalid_use_of_internal_member

import 'dart:convert';
import 'dart:async';
import 'package:meta/meta.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';

abstract class AppRs {
  Config regtestStaticMethodConfig({dynamic hint});

  FlutterRustBridgeTaskConstMeta get kRegtestStaticMethodConfigConstMeta;

  Future<AppHandle?> loadStaticMethodAppHandle(
      {required Config config, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kLoadStaticMethodAppHandleConstMeta;

  Future<AppHandle> recoverStaticMethodAppHandle(
      {required Config config, required String seedPhrase, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kRecoverStaticMethodAppHandleConstMeta;

  Future<AppHandle> signupStaticMethodAppHandle(
      {required Config config, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kSignupStaticMethodAppHandleConstMeta;

  Future<void> testMethodMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kTestMethodMethodAppHandleConstMeta;

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

/// The `AppHandle` is a Dart representation of a current [`App`] instance.
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

  static Future<AppHandle> recover(
          {required AppRs bridge,
          required Config config,
          required String seedPhrase,
          dynamic hint}) =>
      bridge.recoverStaticMethodAppHandle(
          config: config, seedPhrase: seedPhrase, hint: hint);

  static Future<AppHandle> signup(
          {required AppRs bridge, required Config config, dynamic hint}) =>
      bridge.signupStaticMethodAppHandle(config: config, hint: hint);

  Future<void> testMethod({dynamic hint}) => bridge.testMethodMethodAppHandle(
        that: this,
      );
}

enum BuildVariant {
  Production,
  Staging,
  Development,
}

class Config {
  final AppRs bridge;
  final BuildVariant buildVariant;
  final Network network;

  Config({
    required this.bridge,
    required this.buildVariant,
    required this.network,
  });

  static Config regtest({required AppRs bridge, dynamic hint}) =>
      bridge.regtestStaticMethodConfig(hint: hint);
}

enum Network {
  Bitcoin,
  Testnet,
  Regtest,
}
