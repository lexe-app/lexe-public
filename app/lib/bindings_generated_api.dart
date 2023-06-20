// AUTO GENERATED FILE, DO NOT EDIT.
// Generated by `flutter_rust_bridge`@ 1.75.1.
// ignore_for_file: non_constant_identifier_names, unused_element, duplicate_ignore, directives_ordering, curly_braces_in_flow_control_structures, unnecessary_lambdas, slash_for_doc_comments, prefer_const_literals_to_create_immutables, implicit_dynamic_list_literal, duplicate_import, unused_import, unnecessary_import, prefer_single_quotes, prefer_const_constructors, use_super_parameters, always_use_package_imports, annotate_overrides, invalid_use_of_protected_member, constant_identifier_names, invalid_use_of_internal_member, prefer_is_empty, unnecessary_const

import 'dart:convert';
import 'dart:async';
import 'package:meta/meta.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';
import 'package:freezed_annotation/freezed_annotation.dart' hide protected;

part 'bindings_generated_api.freezed.dart';

abstract class AppRs {
  /// Init the Rust [`tracing`] logger. Also sets the current `RUST_LOG_TX`
  /// instance, which ships Rust logs over to the dart side for printing.
  ///
  /// Since `println!`/stdout gets swallowed on mobile, we ship log messages over
  /// to dart for printing. Otherwise we can't see logs while developing.
  ///
  /// When dart calls this function, it generates a `log_tx` and `log_rx`, then
  /// sends the `log_tx` to Rust while holding on to the `log_rx`. When Rust gets
  /// a new [`tracing`] log event, it enqueues the formatted log onto the
  /// `log_tx`.
  ///
  /// Unlike our other Rust loggers, this init will _not_ panic if a
  /// logger instance is already set. Instead it will just update the
  /// `RUST_LOG_TX`. This funky setup allows us to seamlessly support flutter's
  /// hot restart, which would otherwise try to re-init the logger (and cause a
  /// panic) but we still need to register a new log tx.
  ///
  /// `rust_log`: since env vars don't work well on mobile, we need to ship the
  /// equivalent of `$RUST_LOG` configured at build-time through here.
  Stream<String> initRustLogStream({required String rustLog, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kInitRustLogStreamConstMeta;

  String paymentIndexMethodBasicPayment(
      {required BasicPayment that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kPaymentIndexMethodBasicPaymentConstMeta;

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

  Future<FiatRates> fiatRatesMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kFiatRatesMethodAppHandleConstMeta;

  /// Sync the local payment DB to the remote node.
  ///
  /// Returns `true` if any payment changed, so we know whether to reload the
  /// payment list UI.
  Future<bool> syncPaymentsMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kSyncPaymentsMethodAppHandleConstMeta;

  BasicPayment? getPaymentByScrollIdxMethodAppHandle(
      {required AppHandle that, required int scrollIdx, dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kGetPaymentByScrollIdxMethodAppHandleConstMeta;

  int getNumPaymentsMethodAppHandle({required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kGetNumPaymentsMethodAppHandleConstMeta;

  DropFnType get dropOpaqueApp;
  ShareFnType get shareOpaqueApp;
  OpaqueTypeFinalizer get AppFinalizer;

  DropFnType get dropOpaqueBasicPaymentRs;
  ShareFnType get shareOpaqueBasicPaymentRs;
  OpaqueTypeFinalizer get BasicPaymentRsFinalizer;
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

@sealed
class BasicPaymentRs extends FrbOpaque {
  final AppRs bridge;
  BasicPaymentRs.fromRaw(int ptr, int size, this.bridge)
      : super.unsafe(ptr, size);
  @override
  DropFnType get dropFn => bridge.dropOpaqueBasicPaymentRs;

  @override
  ShareFnType get shareFn => bridge.shareOpaqueBasicPaymentRs;

  @override
  OpaqueTypeFinalizer get staticFinalizer => bridge.BasicPaymentRsFinalizer;
}

/// The `AppHandle` is a Dart representation of an [`App`] instance.
class AppHandle {
  final AppRs bridge;
  final App inner;

  const AppHandle({
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

  Future<FiatRates> fiatRates({dynamic hint}) =>
      bridge.fiatRatesMethodAppHandle(
        that: this,
      );

  /// Sync the local payment DB to the remote node.
  ///
  /// Returns `true` if any payment changed, so we know whether to reload the
  /// payment list UI.
  Future<bool> syncPayments({dynamic hint}) =>
      bridge.syncPaymentsMethodAppHandle(
        that: this,
      );

  BasicPayment? getPaymentByScrollIdx({required int scrollIdx, dynamic hint}) =>
      bridge.getPaymentByScrollIdxMethodAppHandle(
        that: this,
        scrollIdx: scrollIdx,
      );

  int getNumPayments({dynamic hint}) => bridge.getNumPaymentsMethodAppHandle(
        that: this,
      );
}

class BasicPayment {
  final AppRs bridge;
  final BasicPaymentRs inner;

  const BasicPayment({
    required this.bridge,
    required this.inner,
  });

  String paymentIndex({dynamic hint}) => bridge.paymentIndexMethodBasicPayment(
        that: this,
      );
}

/// Dart-serializable configuration we get from the flutter side.
@freezed
class Config with _$Config {
  const factory Config({
    required DeployEnv deployEnv,
    required Network network,
    required String gatewayUrl,
    required bool useSgx,
    required String appDataDir,
    required bool useMockSecretStore,
  }) = _Config;
}

enum DeployEnv {
  Prod,
  Staging,
  Dev,
}

@freezed
class FiatRate with _$FiatRate {
  const factory FiatRate({
    required String fiat,
    required double rate,
  }) = _FiatRate;
}

@freezed
class FiatRates with _$FiatRates {
  const factory FiatRates({
    required int timestampMs,
    required List<FiatRate> rates,
  }) = _FiatRates;
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
