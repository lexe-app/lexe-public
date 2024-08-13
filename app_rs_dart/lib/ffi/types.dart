// This file is automatically generated, so please do not edit it.
// Generated by `flutter_rust_bridge`@ 2.2.0.

//
// From: `dart_preamble` in `app-rs-codegen/src/lib.rs`
// ignore_for_file: invalid_internal_annotation, always_use_package_imports, directives_ordering, prefer_const_constructors, sort_unnamed_constructors_first
//

// ignore_for_file: invalid_use_of_internal_member, unused_import, unnecessary_import

import '../frb_generated.dart';
import 'app.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:freezed_annotation/freezed_annotation.dart' hide protected;
part 'types.freezed.dart';

// These function are ignored because they are on traits that is not defined in current crate (put an empty `#[frb]` on it to unignore): `assert_receiver_is_total_eq`, `clone`, `clone`, `eq`, `fmt`, `fmt`, `from`, `from`, `from`, `from`, `from`, `from`, `from`, `from`, `from`, `from`, `from`, `from`, `from`, `from`, `from`, `from`, `try_from`, `try_from`

/// A unique, client-generated id for payment types (onchain send,
/// ln spontaneous send) that need an extra id for idempotency.
@freezed
class ClientPaymentId with _$ClientPaymentId {
  const ClientPaymentId._();
  const factory ClientPaymentId({
    required U8Array32 id,
  }) = _ClientPaymentId;
  static ClientPaymentId gen() =>
      AppRs.instance.api.crateFfiTypesClientPaymentIdGen();
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
  high,
  normal,
  background,
  ;
}

/// See [`common::env::DeployEnv`]
enum DeployEnv {
  dev,
  staging,
  prod,
  ;

  static DeployEnv fromStr({required String s}) =>
      AppRs.instance.api.crateFfiTypesDeployEnvFromStr(s: s);
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

/// See [`common::ln::network::LxNetwork`]
enum Network {
  mainnet,
  testnet,
  regtest,
  ;

  static Network fromStr({required String s}) =>
      AppRs.instance.api.crateFfiTypesNetworkFromStr(s: s);
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

/// The complete payment info, used in the payment detail page. Mirrors the
/// [`BasicPaymentRs`] type.
@freezed
class Payment with _$Payment {
  const factory Payment({
    required PaymentIndex index,
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
  inbound,
  outbound,
  ;
}

/// See [`common::ln::payments::PaymentIndex`].
@freezed
class PaymentIndex with _$PaymentIndex {
  const factory PaymentIndex({
    required String field0,
  }) = _PaymentIndex;
}

enum PaymentKind {
  onchain,
  invoice,
  spontaneous,
  ;
}

@freezed
sealed class PaymentMethod with _$PaymentMethod {
  const PaymentMethod._();

  const factory PaymentMethod.onchain(
    Onchain field0,
  ) = PaymentMethod_Onchain;
  const factory PaymentMethod.invoice(
    Invoice field0,
  ) = PaymentMethod_Invoice;
  const factory PaymentMethod.offer() = PaymentMethod_Offer;
}

enum PaymentStatus {
  pending,
  completed,
  failed,
  ;
}

/// Just the info we need to display an entry in the payments list UI.
@freezed
class ShortPayment with _$ShortPayment {
  const factory ShortPayment({
    required PaymentIndex index,
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

  @override
  int get hashCode => vecIdx.hashCode ^ payment.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ShortPaymentAndIndex &&
          runtimeType == other.runtimeType &&
          vecIdx == other.vecIdx &&
          payment == other.payment;
}
