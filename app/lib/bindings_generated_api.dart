// AUTO GENERATED FILE, DO NOT EDIT.
// Generated by `flutter_rust_bridge`@ 1.82.6.
// ignore_for_file: non_constant_identifier_names, unused_element, duplicate_ignore, directives_ordering, curly_braces_in_flow_control_structures, unnecessary_lambdas, slash_for_doc_comments, prefer_const_literals_to_create_immutables, implicit_dynamic_list_literal, duplicate_import, unused_import, unnecessary_import, prefer_single_quotes, prefer_const_constructors, use_super_parameters, always_use_package_imports, annotate_overrides, invalid_use_of_protected_member, constant_identifier_names, invalid_use_of_internal_member, prefer_is_empty, unnecessary_const

import 'dart:convert';
import 'dart:async';
import 'package:meta/meta.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';
import 'package:freezed_annotation/freezed_annotation.dart' hide protected;
import 'package:collection/collection.dart';

part 'bindings_generated_api.freezed.dart';

abstract class AppRs {
  DeployEnv deployEnvFromStr({required String s, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kDeployEnvFromStrConstMeta;

  Network networkFromStr({required String s, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kNetworkFromStrConstMeta;

  ClientPaymentId genClientPaymentId({dynamic hint});

  FlutterRustBridgeTaskConstMeta get kGenClientPaymentIdConstMeta;

  /// Validate whether `address_str` is a properly formatted bitcoin address. Also
  /// checks that it's valid for the configured bitcoin network.
  ///
  /// The return type is a bit funky: `Option<String>`. `None` means
  /// `address_str` is valid, while `Some(msg)` means it is not (with given
  /// error message). We return in this format to better match the flutter
  /// `FormField` validator API.
  String? formValidateBitcoinAddress(
      {required String addressStr,
      required Network currentNetwork,
      dynamic hint});

  FlutterRustBridgeTaskConstMeta get kFormValidateBitcoinAddressConstMeta;

  /// Validate whether `password` has an appropriate length.
  ///
  /// The return type is a bit funky: `Option<String>`. `None` means
  /// `address_str` is valid, while `Some(msg)` means it is not (with given
  /// error message). We return in this format to better match the flutter
  /// `FormField` validator API.
  String? formValidatePassword({required String password, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kFormValidatePasswordConstMeta;

  /// Resolve a (possible) [`PaymentUri`] string that we just
  /// scanned/pasted into the best [`PaymentMethod`] for us to pay.
  ///
  /// [`PaymentUri`]: payment_uri::PaymentUri
  Future<PaymentMethod> paymentUriResolveBest(
      {required Network network, required String uriStr, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kPaymentUriResolveBestConstMeta;

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

  /// Delete the local persisted `SecretStore` and `RootSeed`.
  ///
  /// WARNING: you will need a backup recovery to use the account afterwards.
  void debugDeleteSecretStore({required Config config, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kDebugDeleteSecretStoreConstMeta;

  /// Delete the local latest_release file.
  void debugDeleteLatestProvisioned({required Config config, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kDebugDeleteLatestProvisionedConstMeta;

  /// Unconditionally panic (for testing).
  Future<void> debugUnconditionalPanic({dynamic hint});

  FlutterRustBridgeTaskConstMeta get kDebugUnconditionalPanicConstMeta;

  /// Unconditionally return Err (for testing).
  Future<void> debugUnconditionalError({dynamic hint});

  FlutterRustBridgeTaskConstMeta get kDebugUnconditionalErrorConstMeta;

  Future<AppHandle?> loadStaticMethodAppHandle(
      {required Config config, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kLoadStaticMethodAppHandleConstMeta;

  Future<AppHandle> restoreStaticMethodAppHandle(
      {required Config config, required String seedPhrase, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kRestoreStaticMethodAppHandleConstMeta;

  Future<AppHandle> signupStaticMethodAppHandle(
      {required Config config,
      required String googleAuthCode,
      required String password,
      dynamic hint});

  FlutterRustBridgeTaskConstMeta get kSignupStaticMethodAppHandleConstMeta;

  Future<NodeInfo> nodeInfoMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kNodeInfoMethodAppHandleConstMeta;

  Future<FiatRates> fiatRatesMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kFiatRatesMethodAppHandleConstMeta;

  Future<void> payOnchainMethodAppHandle(
      {required AppHandle that, required PayOnchainRequest req, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kPayOnchainMethodAppHandleConstMeta;

  Future<EstimateFeeSendOnchainResponse> estimateFeeSendOnchainMethodAppHandle(
      {required AppHandle that,
      required EstimateFeeSendOnchainRequest req,
      dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kEstimateFeeSendOnchainMethodAppHandleConstMeta;

  Future<String> getAddressMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kGetAddressMethodAppHandleConstMeta;

  Future<CreateInvoiceResponse> createInvoiceMethodAppHandle(
      {required AppHandle that,
      required CreateInvoiceRequest req,
      dynamic hint});

  FlutterRustBridgeTaskConstMeta get kCreateInvoiceMethodAppHandleConstMeta;

  Future<PreflightPayInvoiceResponse> preflightPayInvoiceMethodAppHandle(
      {required AppHandle that,
      required PreflightPayInvoiceRequest req,
      dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kPreflightPayInvoiceMethodAppHandleConstMeta;

  Future<void> payInvoiceMethodAppHandle(
      {required AppHandle that, required PayInvoiceRequest req, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kPayInvoiceMethodAppHandleConstMeta;

  /// Delete both the local payment state and the on-disk payment db.
  Future<void> deletePaymentDbMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kDeletePaymentDbMethodAppHandleConstMeta;

  /// Sync the local payment DB to the remote node.
  ///
  /// Returns `true` if any payment changed, so we know whether to reload the
  /// payment list UI.
  Future<bool> syncPaymentsMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kSyncPaymentsMethodAppHandleConstMeta;

  Payment? getPaymentByVecIdxMethodAppHandle(
      {required AppHandle that, required int vecIdx, dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kGetPaymentByVecIdxMethodAppHandleConstMeta;

  ShortPaymentAndIndex? getShortPaymentByScrollIdxMethodAppHandle(
      {required AppHandle that, required int scrollIdx, dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kGetShortPaymentByScrollIdxMethodAppHandleConstMeta;

  ShortPaymentAndIndex? getPendingShortPaymentByScrollIdxMethodAppHandle(
      {required AppHandle that, required int scrollIdx, dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kGetPendingShortPaymentByScrollIdxMethodAppHandleConstMeta;

  ShortPaymentAndIndex? getFinalizedShortPaymentByScrollIdxMethodAppHandle(
      {required AppHandle that, required int scrollIdx, dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kGetFinalizedShortPaymentByScrollIdxMethodAppHandleConstMeta;

  ShortPaymentAndIndex? getPendingNotJunkShortPaymentByScrollIdxMethodAppHandle(
      {required AppHandle that, required int scrollIdx, dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kGetPendingNotJunkShortPaymentByScrollIdxMethodAppHandleConstMeta;

  ShortPaymentAndIndex?
      getFinalizedNotJunkShortPaymentByScrollIdxMethodAppHandle(
          {required AppHandle that, required int scrollIdx, dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kGetFinalizedNotJunkShortPaymentByScrollIdxMethodAppHandleConstMeta;

  int getNumPaymentsMethodAppHandle({required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kGetNumPaymentsMethodAppHandleConstMeta;

  int getNumPendingPaymentsMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kGetNumPendingPaymentsMethodAppHandleConstMeta;

  int getNumFinalizedPaymentsMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kGetNumFinalizedPaymentsMethodAppHandleConstMeta;

  int getNumPendingNotJunkPaymentsMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kGetNumPendingNotJunkPaymentsMethodAppHandleConstMeta;

  int getNumFinalizedNotJunkPaymentsMethodAppHandle(
      {required AppHandle that, dynamic hint});

  FlutterRustBridgeTaskConstMeta
      get kGetNumFinalizedNotJunkPaymentsMethodAppHandleConstMeta;

  Future<void> updatePaymentNoteMethodAppHandle(
      {required AppHandle that, required UpdatePaymentNote req, dynamic hint});

  FlutterRustBridgeTaskConstMeta get kUpdatePaymentNoteMethodAppHandleConstMeta;

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
          {required AppRs bridge,
          required Config config,
          required String googleAuthCode,
          required String password,
          dynamic hint}) =>
      bridge.signupStaticMethodAppHandle(
          config: config,
          googleAuthCode: googleAuthCode,
          password: password,
          hint: hint);

  Future<NodeInfo> nodeInfo({dynamic hint}) => bridge.nodeInfoMethodAppHandle(
        that: this,
      );

  Future<FiatRates> fiatRates({dynamic hint}) =>
      bridge.fiatRatesMethodAppHandle(
        that: this,
      );

  Future<void> payOnchain({required PayOnchainRequest req, dynamic hint}) =>
      bridge.payOnchainMethodAppHandle(
        that: this,
        req: req,
      );

  Future<EstimateFeeSendOnchainResponse> estimateFeeSendOnchain(
          {required EstimateFeeSendOnchainRequest req, dynamic hint}) =>
      bridge.estimateFeeSendOnchainMethodAppHandle(
        that: this,
        req: req,
      );

  Future<String> getAddress({dynamic hint}) => bridge.getAddressMethodAppHandle(
        that: this,
      );

  Future<CreateInvoiceResponse> createInvoice(
          {required CreateInvoiceRequest req, dynamic hint}) =>
      bridge.createInvoiceMethodAppHandle(
        that: this,
        req: req,
      );

  Future<PreflightPayInvoiceResponse> preflightPayInvoice(
          {required PreflightPayInvoiceRequest req, dynamic hint}) =>
      bridge.preflightPayInvoiceMethodAppHandle(
        that: this,
        req: req,
      );

  Future<void> payInvoice({required PayInvoiceRequest req, dynamic hint}) =>
      bridge.payInvoiceMethodAppHandle(
        that: this,
        req: req,
      );

  /// Delete both the local payment state and the on-disk payment db.
  Future<void> deletePaymentDb({dynamic hint}) =>
      bridge.deletePaymentDbMethodAppHandle(
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

  Payment? getPaymentByVecIdx({required int vecIdx, dynamic hint}) =>
      bridge.getPaymentByVecIdxMethodAppHandle(
        that: this,
        vecIdx: vecIdx,
      );

  ShortPaymentAndIndex? getShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      bridge.getShortPaymentByScrollIdxMethodAppHandle(
        that: this,
        scrollIdx: scrollIdx,
      );

  ShortPaymentAndIndex? getPendingShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      bridge.getPendingShortPaymentByScrollIdxMethodAppHandle(
        that: this,
        scrollIdx: scrollIdx,
      );

  ShortPaymentAndIndex? getFinalizedShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      bridge.getFinalizedShortPaymentByScrollIdxMethodAppHandle(
        that: this,
        scrollIdx: scrollIdx,
      );

  ShortPaymentAndIndex? getPendingNotJunkShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      bridge.getPendingNotJunkShortPaymentByScrollIdxMethodAppHandle(
        that: this,
        scrollIdx: scrollIdx,
      );

  ShortPaymentAndIndex? getFinalizedNotJunkShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      bridge.getFinalizedNotJunkShortPaymentByScrollIdxMethodAppHandle(
        that: this,
        scrollIdx: scrollIdx,
      );

  int getNumPayments({dynamic hint}) => bridge.getNumPaymentsMethodAppHandle(
        that: this,
      );

  int getNumPendingPayments({dynamic hint}) =>
      bridge.getNumPendingPaymentsMethodAppHandle(
        that: this,
      );

  int getNumFinalizedPayments({dynamic hint}) =>
      bridge.getNumFinalizedPaymentsMethodAppHandle(
        that: this,
      );

  int getNumPendingNotJunkPayments({dynamic hint}) =>
      bridge.getNumPendingNotJunkPaymentsMethodAppHandle(
        that: this,
      );

  int getNumFinalizedNotJunkPayments({dynamic hint}) =>
      bridge.getNumFinalizedNotJunkPaymentsMethodAppHandle(
        that: this,
      );

  Future<void> updatePaymentNote(
          {required UpdatePaymentNote req, dynamic hint}) =>
      bridge.updatePaymentNoteMethodAppHandle(
        that: this,
        req: req,
      );
}

@freezed
class Balance with _$Balance {
  const factory Balance({
    required int totalSats,
    required int lightningSats,
    required int onchainSats,
  }) = _Balance;
}

/// A unique, client-generated id for payment types (onchain send,
/// ln spontaneous send) that need an extra id for idempotency.
@freezed
class ClientPaymentId with _$ClientPaymentId {
  const factory ClientPaymentId({
    required U8Array32 id,
  }) = _ClientPaymentId;
}

/// Dart-serializable configuration we get from the flutter side.
@freezed
class Config with _$Config {
  const factory Config({
    required DeployEnv deployEnv,
    required Network network,
    required String gatewayUrl,
    required bool useSgx,
    required String baseAppDataDir,
    required bool useMockSecretStore,
  }) = _Config;
}

enum ConfirmationPriority {
  High,
  Normal,
  Background,
}

class CreateInvoiceRequest {
  final int expirySecs;
  final int? amountSats;
  final String? description;

  const CreateInvoiceRequest({
    required this.expirySecs,
    this.amountSats,
    this.description,
  });
}

class CreateInvoiceResponse {
  final Invoice invoice;

  const CreateInvoiceResponse({
    required this.invoice,
  });
}

enum DeployEnv {
  Prod,
  Staging,
  Dev,
}

class EstimateFeeSendOnchainRequest {
  final String address;
  final int amountSats;

  const EstimateFeeSendOnchainRequest({
    required this.address,
    required this.amountSats,
  });
}

class EstimateFeeSendOnchainResponse {
  final FeeEstimate? high;
  final FeeEstimate normal;
  final FeeEstimate background;

  const EstimateFeeSendOnchainResponse({
    this.high,
    required this.normal,
    required this.background,
  });
}

class FeeEstimate {
  final int amountSats;

  const FeeEstimate({
    required this.amountSats,
  });
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

/// A lightning invoice with useful fields parsed out for the flutter frontend.
/// Mirrors the [`LxInvoice`] type.
@freezed
class Invoice with _$Invoice {
  const factory Invoice({
    required String string,
    String? description,
    required int createdAt,
    required int expiresAt,
    int? amountSats,
    required String payeePubkey,
  }) = _Invoice;
}

enum Network {
  Mainnet,
  Testnet,
  Regtest,
}

@freezed
class NodeInfo with _$NodeInfo {
  const factory NodeInfo({
    required String nodePk,
    required String version,
    required String measurement,
    required Balance balance,
  }) = _NodeInfo;
}

/// A potential onchain Bitcoin payment.
@freezed
class Onchain with _$Onchain {
  const factory Onchain({
    required String address,
    int? amountSats,
    String? label,
    String? message,
  }) = _Onchain;
}

/// Mirrors the [`common::api::command::PayInvoiceRequest`] type.
@freezed
class PayInvoiceRequest with _$PayInvoiceRequest {
  const factory PayInvoiceRequest({
    required String invoice,
    int? fallbackAmountSats,
    String? note,
  }) = _PayInvoiceRequest;
}

class PayOnchainRequest {
  final ClientPaymentId cid;
  final String address;
  final int amountSats;
  final ConfirmationPriority priority;
  final String? note;

  const PayOnchainRequest({
    required this.cid,
    required this.address,
    required this.amountSats,
    required this.priority,
    this.note,
  });
}

/// The complete payment info, used in the payment detail page. Mirrors the
/// [`BasicPayment`] type.
@freezed
class Payment with _$Payment {
  const factory Payment({
    required String index,
    required PaymentKind kind,
    required PaymentDirection direction,
    Invoice? invoice,
    String? replacement,
    int? amountSat,
    required int feesSat,
    required PaymentStatus status,
    required String statusStr,
    String? note,
    required int createdAt,
    int? finalizedAt,
  }) = _Payment;
}

enum PaymentDirection {
  Inbound,
  Outbound,
}

enum PaymentKind {
  Onchain,
  Invoice,
  Spontaneous,
}

@freezed
sealed class PaymentMethod with _$PaymentMethod {
  const factory PaymentMethod.onchain(
    Onchain field0,
  ) = PaymentMethod_Onchain;
  const factory PaymentMethod.invoice(
    Invoice field0,
  ) = PaymentMethod_Invoice;
  const factory PaymentMethod.offer() = PaymentMethod_Offer;
}

enum PaymentStatus {
  Pending,
  Completed,
  Failed,
}

/// See [`common::api::command::PreflightPayInvoiceRequest`].
class PreflightPayInvoiceRequest {
  final String invoice;
  final int? fallbackAmountSats;

  const PreflightPayInvoiceRequest({
    required this.invoice,
    this.fallbackAmountSats,
  });
}

/// See [`common::api::command::PreflightPayInvoiceResponse`].
class PreflightPayInvoiceResponse {
  final int amountSats;
  final int feesSats;

  const PreflightPayInvoiceResponse({
    required this.amountSats,
    required this.feesSats,
  });
}

/// Just the info we need to display an entry in the payments list UI.
@freezed
class ShortPayment with _$ShortPayment {
  const factory ShortPayment({
    required String index,
    required PaymentKind kind,
    required PaymentDirection direction,
    int? amountSat,
    required PaymentStatus status,
    String? note,
    required int createdAt,
  }) = _ShortPayment;
}

/// Just a `(usize, ShortPayment)`, but packaged in a struct until
/// `flutter_rust_bridge` stops breaking on tuples.
class ShortPaymentAndIndex {
  final int vecIdx;
  final ShortPayment payment;

  const ShortPaymentAndIndex({
    required this.vecIdx,
    required this.payment,
  });
}

class U8Array32 extends NonGrowableListView<int> {
  static const arraySize = 32;
  U8Array32(Uint8List inner)
      : assert(inner.length == arraySize),
        super(inner);
  U8Array32.unchecked(Uint8List inner) : super(inner);
  U8Array32.init() : super(Uint8List(arraySize));
}

class UpdatePaymentNote {
  final String index;
  final String? note;

  const UpdatePaymentNote({
    required this.index,
    this.note,
  });
}
