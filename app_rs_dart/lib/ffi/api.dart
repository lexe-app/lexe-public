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
import 'types.dart';
part 'api.freezed.dart';

// These functions are ignored because they are not marked as `pub`: `from_cid_and_response`, `from_id_and_response`, `validate_note`
// These function are ignored because they are on traits that is not defined in current crate (put an empty `#[frb]` on it to unignore): `from`, `from`, `from`, `from`, `from`, `from`, `from`, `from`, `try_from`, `try_from`, `try_from`, `try_from`, `try_from`, `try_from`

@freezed
class Balance with _$Balance {
  const factory Balance({
    required int totalSats,
    required int lightningSats,
    required int onchainSats,
  }) = _Balance;
}

/// See [`common::api::command::CreateInvoiceRequest`].
@freezed
class CreateInvoiceRequest with _$CreateInvoiceRequest {
  const factory CreateInvoiceRequest({
    required int expirySecs,
    int? amountSats,
    String? description,
  }) = _CreateInvoiceRequest;
}

/// See [`common::api::command::CreateInvoiceResponse`].
@freezed
class CreateInvoiceResponse with _$CreateInvoiceResponse {
  const factory CreateInvoiceResponse({
    required Invoice invoice,
  }) = _CreateInvoiceResponse;
}

/// See [`common::api::command::FeeEstimate`].
@freezed
class FeeEstimate with _$FeeEstimate {
  const factory FeeEstimate({
    required int amountSats,
  }) = _FeeEstimate;
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

@freezed
class ListChannelsResponse with _$ListChannelsResponse {
  const factory ListChannelsResponse({
    required List<LxChannelDetails> channels,
  }) = _ListChannelsResponse;
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

/// Mirrors the [`common::api::command::PayInvoiceRequest`] type.
@freezed
class PayInvoiceRequest with _$PayInvoiceRequest {
  const factory PayInvoiceRequest({
    required String invoice,
    int? fallbackAmountSats,
    String? note,
  }) = _PayInvoiceRequest;
}

/// Mirrors [`common::api::command::PayInvoiceResponse`] the type, but enriches
/// the response so we get the full `PaymentIndex`.
@freezed
class PayInvoiceResponse with _$PayInvoiceResponse {
  const factory PayInvoiceResponse({
    required PaymentIndex index,
  }) = _PayInvoiceResponse;
}

/// See [`common::api::command::PayOnchainRequest`].
@freezed
class PayOnchainRequest with _$PayOnchainRequest {
  const factory PayOnchainRequest({
    required ClientPaymentId cid,
    required String address,
    required int amountSats,
    required ConfirmationPriority priority,
    String? note,
  }) = _PayOnchainRequest;
}

/// See [`common::api::command::PayOnchainResponse`].
@freezed
class PayOnchainResponse with _$PayOnchainResponse {
  const factory PayOnchainResponse({
    required PaymentIndex index,
    required String txid,
  }) = _PayOnchainResponse;
}

/// See [`common::api::command::PreflightPayInvoiceRequest`].
@freezed
class PreflightPayInvoiceRequest with _$PreflightPayInvoiceRequest {
  const factory PreflightPayInvoiceRequest({
    required String invoice,
    int? fallbackAmountSats,
  }) = _PreflightPayInvoiceRequest;
}

/// See [`common::api::command::PreflightPayInvoiceResponse`].
@freezed
class PreflightPayInvoiceResponse with _$PreflightPayInvoiceResponse {
  const factory PreflightPayInvoiceResponse({
    required int amountSats,
    required int feesSats,
  }) = _PreflightPayInvoiceResponse;
}

/// See [`common::api::command::PreflightPayOnchainRequest`].
@freezed
class PreflightPayOnchainRequest with _$PreflightPayOnchainRequest {
  const factory PreflightPayOnchainRequest({
    required String address,
    required int amountSats,
  }) = _PreflightPayOnchainRequest;
}

/// See [`common::api::command::PreflightPayOnchainResponse`].
@freezed
class PreflightPayOnchainResponse with _$PreflightPayOnchainResponse {
  const factory PreflightPayOnchainResponse({
    FeeEstimate? high,
    required FeeEstimate normal,
    required FeeEstimate background,
  }) = _PreflightPayOnchainResponse;
}

/// See [`common::api::qs::UpdatePaymentNote`].
@freezed
class UpdatePaymentNote with _$UpdatePaymentNote {
  const factory UpdatePaymentNote({
    required PaymentIndex index,
    String? note,
  }) = _UpdatePaymentNote;
}
