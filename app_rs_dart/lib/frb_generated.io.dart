// This file is automatically generated, so please do not edit it.
// Generated by `flutter_rust_bridge`@ 2.1.0.

//
// From: `dart_preamble` in `app-rs-codegen/src/lib.rs`
// ignore_for_file: invalid_internal_annotation, always_use_package_imports, directives_ordering, prefer_const_constructors, sort_unnamed_constructors_first
//

// ignore_for_file: unused_import, unused_element, unnecessary_import, duplicate_ignore, invalid_use_of_internal_member, annotate_overrides, non_constant_identifier_names, curly_braces_in_flow_control_structures, prefer_const_literals_to_create_immutables, unused_field

import 'dart:async';
import 'dart:convert';
import 'dart:ffi' as ffi;
import 'ffi/api.dart';
import 'ffi/app.dart';
import 'ffi/debug.dart';
import 'ffi/form.dart';
import 'ffi/logger.dart';
import 'ffi/payment_uri.dart';
import 'ffi/settings.dart';
import 'ffi/types.dart';
import 'frb_generated.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated_io.dart';

abstract class AppRsApiImplPlatform extends BaseApiImpl<AppRsWire> {
  AppRsApiImplPlatform({
    required super.handler,
    required super.wire,
    required super.generalizedFrbRustBinding,
    required super.portManager,
  });

  CrossPlatformFinalizerArg get rust_arc_decrement_strong_count_AppPtr =>
      wire._rust_arc_decrement_strong_count_RustOpaque_AppPtr;

  @protected
  AnyhowException dco_decode_AnyhowException(dynamic raw);

  @protected
  int dco_decode_CastedPrimitive_i_64(dynamic raw);

  @protected
  int dco_decode_CastedPrimitive_u_64(dynamic raw);

  @protected
  int dco_decode_CastedPrimitive_usize(dynamic raw);

  @protected
  App dco_decode_RustOpaque_App(dynamic raw);

  @protected
  RustStreamSink<String> dco_decode_StreamSink_String_Sse(dynamic raw);

  @protected
  String dco_decode_String(dynamic raw);

  @protected
  AppHandle dco_decode_app_handle(dynamic raw);

  @protected
  Balance dco_decode_balance(dynamic raw);

  @protected
  bool dco_decode_bool(dynamic raw);

  @protected
  AppHandle dco_decode_box_autoadd_app_handle(dynamic raw);

  @protected
  Config dco_decode_box_autoadd_config(dynamic raw);

  @protected
  CreateInvoiceRequest dco_decode_box_autoadd_create_invoice_request(
      dynamic raw);

  @protected
  FeeEstimate dco_decode_box_autoadd_fee_estimate(dynamic raw);

  @protected
  Invoice dco_decode_box_autoadd_invoice(dynamic raw);

  @protected
  Onchain dco_decode_box_autoadd_onchain(dynamic raw);

  @protected
  PayInvoiceRequest dco_decode_box_autoadd_pay_invoice_request(dynamic raw);

  @protected
  PayOnchainRequest dco_decode_box_autoadd_pay_onchain_request(dynamic raw);

  @protected
  Payment dco_decode_box_autoadd_payment(dynamic raw);

  @protected
  PaymentIndex dco_decode_box_autoadd_payment_index(dynamic raw);

  @protected
  PreflightPayInvoiceRequest
      dco_decode_box_autoadd_preflight_pay_invoice_request(dynamic raw);

  @protected
  PreflightPayOnchainRequest
      dco_decode_box_autoadd_preflight_pay_onchain_request(dynamic raw);

  @protected
  Settings dco_decode_box_autoadd_settings(dynamic raw);

  @protected
  ShortPaymentAndIndex dco_decode_box_autoadd_short_payment_and_index(
      dynamic raw);

  @protected
  UpdatePaymentNote dco_decode_box_autoadd_update_payment_note(dynamic raw);

  @protected
  ClientPaymentId dco_decode_client_payment_id(dynamic raw);

  @protected
  Config dco_decode_config(dynamic raw);

  @protected
  ConfirmationPriority dco_decode_confirmation_priority(dynamic raw);

  @protected
  CreateInvoiceRequest dco_decode_create_invoice_request(dynamic raw);

  @protected
  CreateInvoiceResponse dco_decode_create_invoice_response(dynamic raw);

  @protected
  DeployEnv dco_decode_deploy_env(dynamic raw);

  @protected
  double dco_decode_f_64(dynamic raw);

  @protected
  FeeEstimate dco_decode_fee_estimate(dynamic raw);

  @protected
  FiatRate dco_decode_fiat_rate(dynamic raw);

  @protected
  FiatRates dco_decode_fiat_rates(dynamic raw);

  @protected
  int dco_decode_i_32(dynamic raw);

  @protected
  PlatformInt64 dco_decode_i_64(dynamic raw);

  @protected
  Invoice dco_decode_invoice(dynamic raw);

  @protected
  List<FiatRate> dco_decode_list_fiat_rate(dynamic raw);

  @protected
  Uint8List dco_decode_list_prim_u_8_strict(dynamic raw);

  @protected
  Network dco_decode_network(dynamic raw);

  @protected
  NodeInfo dco_decode_node_info(dynamic raw);

  @protected
  Onchain dco_decode_onchain(dynamic raw);

  @protected
  int? dco_decode_opt_CastedPrimitive_i_64(dynamic raw);

  @protected
  int? dco_decode_opt_CastedPrimitive_u_64(dynamic raw);

  @protected
  int? dco_decode_opt_CastedPrimitive_usize(dynamic raw);

  @protected
  String? dco_decode_opt_String(dynamic raw);

  @protected
  AppHandle? dco_decode_opt_box_autoadd_app_handle(dynamic raw);

  @protected
  FeeEstimate? dco_decode_opt_box_autoadd_fee_estimate(dynamic raw);

  @protected
  Invoice? dco_decode_opt_box_autoadd_invoice(dynamic raw);

  @protected
  Payment? dco_decode_opt_box_autoadd_payment(dynamic raw);

  @protected
  ShortPaymentAndIndex? dco_decode_opt_box_autoadd_short_payment_and_index(
      dynamic raw);

  @protected
  PayInvoiceRequest dco_decode_pay_invoice_request(dynamic raw);

  @protected
  PayInvoiceResponse dco_decode_pay_invoice_response(dynamic raw);

  @protected
  PayOnchainRequest dco_decode_pay_onchain_request(dynamic raw);

  @protected
  PayOnchainResponse dco_decode_pay_onchain_response(dynamic raw);

  @protected
  Payment dco_decode_payment(dynamic raw);

  @protected
  PaymentDirection dco_decode_payment_direction(dynamic raw);

  @protected
  PaymentIndex dco_decode_payment_index(dynamic raw);

  @protected
  PaymentKind dco_decode_payment_kind(dynamic raw);

  @protected
  PaymentMethod dco_decode_payment_method(dynamic raw);

  @protected
  PaymentStatus dco_decode_payment_status(dynamic raw);

  @protected
  PreflightPayInvoiceRequest dco_decode_preflight_pay_invoice_request(
      dynamic raw);

  @protected
  PreflightPayInvoiceResponse dco_decode_preflight_pay_invoice_response(
      dynamic raw);

  @protected
  PreflightPayOnchainRequest dco_decode_preflight_pay_onchain_request(
      dynamic raw);

  @protected
  PreflightPayOnchainResponse dco_decode_preflight_pay_onchain_response(
      dynamic raw);

  @protected
  Settings dco_decode_settings(dynamic raw);

  @protected
  ShortPayment dco_decode_short_payment(dynamic raw);

  @protected
  ShortPaymentAndIndex dco_decode_short_payment_and_index(dynamic raw);

  @protected
  int dco_decode_u_32(dynamic raw);

  @protected
  BigInt dco_decode_u_64(dynamic raw);

  @protected
  int dco_decode_u_8(dynamic raw);

  @protected
  U8Array32 dco_decode_u_8_array_32(dynamic raw);

  @protected
  void dco_decode_unit(dynamic raw);

  @protected
  UpdatePaymentNote dco_decode_update_payment_note(dynamic raw);

  @protected
  BigInt dco_decode_usize(dynamic raw);

  @protected
  AnyhowException sse_decode_AnyhowException(SseDeserializer deserializer);

  @protected
  int sse_decode_CastedPrimitive_i_64(SseDeserializer deserializer);

  @protected
  int sse_decode_CastedPrimitive_u_64(SseDeserializer deserializer);

  @protected
  int sse_decode_CastedPrimitive_usize(SseDeserializer deserializer);

  @protected
  App sse_decode_RustOpaque_App(SseDeserializer deserializer);

  @protected
  RustStreamSink<String> sse_decode_StreamSink_String_Sse(
      SseDeserializer deserializer);

  @protected
  String sse_decode_String(SseDeserializer deserializer);

  @protected
  AppHandle sse_decode_app_handle(SseDeserializer deserializer);

  @protected
  Balance sse_decode_balance(SseDeserializer deserializer);

  @protected
  bool sse_decode_bool(SseDeserializer deserializer);

  @protected
  AppHandle sse_decode_box_autoadd_app_handle(SseDeserializer deserializer);

  @protected
  Config sse_decode_box_autoadd_config(SseDeserializer deserializer);

  @protected
  CreateInvoiceRequest sse_decode_box_autoadd_create_invoice_request(
      SseDeserializer deserializer);

  @protected
  FeeEstimate sse_decode_box_autoadd_fee_estimate(SseDeserializer deserializer);

  @protected
  Invoice sse_decode_box_autoadd_invoice(SseDeserializer deserializer);

  @protected
  Onchain sse_decode_box_autoadd_onchain(SseDeserializer deserializer);

  @protected
  PayInvoiceRequest sse_decode_box_autoadd_pay_invoice_request(
      SseDeserializer deserializer);

  @protected
  PayOnchainRequest sse_decode_box_autoadd_pay_onchain_request(
      SseDeserializer deserializer);

  @protected
  Payment sse_decode_box_autoadd_payment(SseDeserializer deserializer);

  @protected
  PaymentIndex sse_decode_box_autoadd_payment_index(
      SseDeserializer deserializer);

  @protected
  PreflightPayInvoiceRequest
      sse_decode_box_autoadd_preflight_pay_invoice_request(
          SseDeserializer deserializer);

  @protected
  PreflightPayOnchainRequest
      sse_decode_box_autoadd_preflight_pay_onchain_request(
          SseDeserializer deserializer);

  @protected
  Settings sse_decode_box_autoadd_settings(SseDeserializer deserializer);

  @protected
  ShortPaymentAndIndex sse_decode_box_autoadd_short_payment_and_index(
      SseDeserializer deserializer);

  @protected
  UpdatePaymentNote sse_decode_box_autoadd_update_payment_note(
      SseDeserializer deserializer);

  @protected
  ClientPaymentId sse_decode_client_payment_id(SseDeserializer deserializer);

  @protected
  Config sse_decode_config(SseDeserializer deserializer);

  @protected
  ConfirmationPriority sse_decode_confirmation_priority(
      SseDeserializer deserializer);

  @protected
  CreateInvoiceRequest sse_decode_create_invoice_request(
      SseDeserializer deserializer);

  @protected
  CreateInvoiceResponse sse_decode_create_invoice_response(
      SseDeserializer deserializer);

  @protected
  DeployEnv sse_decode_deploy_env(SseDeserializer deserializer);

  @protected
  double sse_decode_f_64(SseDeserializer deserializer);

  @protected
  FeeEstimate sse_decode_fee_estimate(SseDeserializer deserializer);

  @protected
  FiatRate sse_decode_fiat_rate(SseDeserializer deserializer);

  @protected
  FiatRates sse_decode_fiat_rates(SseDeserializer deserializer);

  @protected
  int sse_decode_i_32(SseDeserializer deserializer);

  @protected
  PlatformInt64 sse_decode_i_64(SseDeserializer deserializer);

  @protected
  Invoice sse_decode_invoice(SseDeserializer deserializer);

  @protected
  List<FiatRate> sse_decode_list_fiat_rate(SseDeserializer deserializer);

  @protected
  Uint8List sse_decode_list_prim_u_8_strict(SseDeserializer deserializer);

  @protected
  Network sse_decode_network(SseDeserializer deserializer);

  @protected
  NodeInfo sse_decode_node_info(SseDeserializer deserializer);

  @protected
  Onchain sse_decode_onchain(SseDeserializer deserializer);

  @protected
  int? sse_decode_opt_CastedPrimitive_i_64(SseDeserializer deserializer);

  @protected
  int? sse_decode_opt_CastedPrimitive_u_64(SseDeserializer deserializer);

  @protected
  int? sse_decode_opt_CastedPrimitive_usize(SseDeserializer deserializer);

  @protected
  String? sse_decode_opt_String(SseDeserializer deserializer);

  @protected
  AppHandle? sse_decode_opt_box_autoadd_app_handle(
      SseDeserializer deserializer);

  @protected
  FeeEstimate? sse_decode_opt_box_autoadd_fee_estimate(
      SseDeserializer deserializer);

  @protected
  Invoice? sse_decode_opt_box_autoadd_invoice(SseDeserializer deserializer);

  @protected
  Payment? sse_decode_opt_box_autoadd_payment(SseDeserializer deserializer);

  @protected
  ShortPaymentAndIndex? sse_decode_opt_box_autoadd_short_payment_and_index(
      SseDeserializer deserializer);

  @protected
  PayInvoiceRequest sse_decode_pay_invoice_request(
      SseDeserializer deserializer);

  @protected
  PayInvoiceResponse sse_decode_pay_invoice_response(
      SseDeserializer deserializer);

  @protected
  PayOnchainRequest sse_decode_pay_onchain_request(
      SseDeserializer deserializer);

  @protected
  PayOnchainResponse sse_decode_pay_onchain_response(
      SseDeserializer deserializer);

  @protected
  Payment sse_decode_payment(SseDeserializer deserializer);

  @protected
  PaymentDirection sse_decode_payment_direction(SseDeserializer deserializer);

  @protected
  PaymentIndex sse_decode_payment_index(SseDeserializer deserializer);

  @protected
  PaymentKind sse_decode_payment_kind(SseDeserializer deserializer);

  @protected
  PaymentMethod sse_decode_payment_method(SseDeserializer deserializer);

  @protected
  PaymentStatus sse_decode_payment_status(SseDeserializer deserializer);

  @protected
  PreflightPayInvoiceRequest sse_decode_preflight_pay_invoice_request(
      SseDeserializer deserializer);

  @protected
  PreflightPayInvoiceResponse sse_decode_preflight_pay_invoice_response(
      SseDeserializer deserializer);

  @protected
  PreflightPayOnchainRequest sse_decode_preflight_pay_onchain_request(
      SseDeserializer deserializer);

  @protected
  PreflightPayOnchainResponse sse_decode_preflight_pay_onchain_response(
      SseDeserializer deserializer);

  @protected
  Settings sse_decode_settings(SseDeserializer deserializer);

  @protected
  ShortPayment sse_decode_short_payment(SseDeserializer deserializer);

  @protected
  ShortPaymentAndIndex sse_decode_short_payment_and_index(
      SseDeserializer deserializer);

  @protected
  int sse_decode_u_32(SseDeserializer deserializer);

  @protected
  BigInt sse_decode_u_64(SseDeserializer deserializer);

  @protected
  int sse_decode_u_8(SseDeserializer deserializer);

  @protected
  U8Array32 sse_decode_u_8_array_32(SseDeserializer deserializer);

  @protected
  void sse_decode_unit(SseDeserializer deserializer);

  @protected
  UpdatePaymentNote sse_decode_update_payment_note(
      SseDeserializer deserializer);

  @protected
  BigInt sse_decode_usize(SseDeserializer deserializer);

  @protected
  void sse_encode_AnyhowException(
      AnyhowException self, SseSerializer serializer);

  @protected
  void sse_encode_CastedPrimitive_i_64(int self, SseSerializer serializer);

  @protected
  void sse_encode_CastedPrimitive_u_64(int self, SseSerializer serializer);

  @protected
  void sse_encode_CastedPrimitive_usize(int self, SseSerializer serializer);

  @protected
  void sse_encode_RustOpaque_App(App self, SseSerializer serializer);

  @protected
  void sse_encode_StreamSink_String_Sse(
      RustStreamSink<String> self, SseSerializer serializer);

  @protected
  void sse_encode_String(String self, SseSerializer serializer);

  @protected
  void sse_encode_app_handle(AppHandle self, SseSerializer serializer);

  @protected
  void sse_encode_balance(Balance self, SseSerializer serializer);

  @protected
  void sse_encode_bool(bool self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_app_handle(
      AppHandle self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_config(Config self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_create_invoice_request(
      CreateInvoiceRequest self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_fee_estimate(
      FeeEstimate self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_invoice(Invoice self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_onchain(Onchain self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_pay_invoice_request(
      PayInvoiceRequest self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_pay_onchain_request(
      PayOnchainRequest self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_payment(Payment self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_payment_index(
      PaymentIndex self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_preflight_pay_invoice_request(
      PreflightPayInvoiceRequest self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_preflight_pay_onchain_request(
      PreflightPayOnchainRequest self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_settings(Settings self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_short_payment_and_index(
      ShortPaymentAndIndex self, SseSerializer serializer);

  @protected
  void sse_encode_box_autoadd_update_payment_note(
      UpdatePaymentNote self, SseSerializer serializer);

  @protected
  void sse_encode_client_payment_id(
      ClientPaymentId self, SseSerializer serializer);

  @protected
  void sse_encode_config(Config self, SseSerializer serializer);

  @protected
  void sse_encode_confirmation_priority(
      ConfirmationPriority self, SseSerializer serializer);

  @protected
  void sse_encode_create_invoice_request(
      CreateInvoiceRequest self, SseSerializer serializer);

  @protected
  void sse_encode_create_invoice_response(
      CreateInvoiceResponse self, SseSerializer serializer);

  @protected
  void sse_encode_deploy_env(DeployEnv self, SseSerializer serializer);

  @protected
  void sse_encode_f_64(double self, SseSerializer serializer);

  @protected
  void sse_encode_fee_estimate(FeeEstimate self, SseSerializer serializer);

  @protected
  void sse_encode_fiat_rate(FiatRate self, SseSerializer serializer);

  @protected
  void sse_encode_fiat_rates(FiatRates self, SseSerializer serializer);

  @protected
  void sse_encode_i_32(int self, SseSerializer serializer);

  @protected
  void sse_encode_i_64(PlatformInt64 self, SseSerializer serializer);

  @protected
  void sse_encode_invoice(Invoice self, SseSerializer serializer);

  @protected
  void sse_encode_list_fiat_rate(List<FiatRate> self, SseSerializer serializer);

  @protected
  void sse_encode_list_prim_u_8_strict(
      Uint8List self, SseSerializer serializer);

  @protected
  void sse_encode_network(Network self, SseSerializer serializer);

  @protected
  void sse_encode_node_info(NodeInfo self, SseSerializer serializer);

  @protected
  void sse_encode_onchain(Onchain self, SseSerializer serializer);

  @protected
  void sse_encode_opt_CastedPrimitive_i_64(int? self, SseSerializer serializer);

  @protected
  void sse_encode_opt_CastedPrimitive_u_64(int? self, SseSerializer serializer);

  @protected
  void sse_encode_opt_CastedPrimitive_usize(
      int? self, SseSerializer serializer);

  @protected
  void sse_encode_opt_String(String? self, SseSerializer serializer);

  @protected
  void sse_encode_opt_box_autoadd_app_handle(
      AppHandle? self, SseSerializer serializer);

  @protected
  void sse_encode_opt_box_autoadd_fee_estimate(
      FeeEstimate? self, SseSerializer serializer);

  @protected
  void sse_encode_opt_box_autoadd_invoice(
      Invoice? self, SseSerializer serializer);

  @protected
  void sse_encode_opt_box_autoadd_payment(
      Payment? self, SseSerializer serializer);

  @protected
  void sse_encode_opt_box_autoadd_short_payment_and_index(
      ShortPaymentAndIndex? self, SseSerializer serializer);

  @protected
  void sse_encode_pay_invoice_request(
      PayInvoiceRequest self, SseSerializer serializer);

  @protected
  void sse_encode_pay_invoice_response(
      PayInvoiceResponse self, SseSerializer serializer);

  @protected
  void sse_encode_pay_onchain_request(
      PayOnchainRequest self, SseSerializer serializer);

  @protected
  void sse_encode_pay_onchain_response(
      PayOnchainResponse self, SseSerializer serializer);

  @protected
  void sse_encode_payment(Payment self, SseSerializer serializer);

  @protected
  void sse_encode_payment_direction(
      PaymentDirection self, SseSerializer serializer);

  @protected
  void sse_encode_payment_index(PaymentIndex self, SseSerializer serializer);

  @protected
  void sse_encode_payment_kind(PaymentKind self, SseSerializer serializer);

  @protected
  void sse_encode_payment_method(PaymentMethod self, SseSerializer serializer);

  @protected
  void sse_encode_payment_status(PaymentStatus self, SseSerializer serializer);

  @protected
  void sse_encode_preflight_pay_invoice_request(
      PreflightPayInvoiceRequest self, SseSerializer serializer);

  @protected
  void sse_encode_preflight_pay_invoice_response(
      PreflightPayInvoiceResponse self, SseSerializer serializer);

  @protected
  void sse_encode_preflight_pay_onchain_request(
      PreflightPayOnchainRequest self, SseSerializer serializer);

  @protected
  void sse_encode_preflight_pay_onchain_response(
      PreflightPayOnchainResponse self, SseSerializer serializer);

  @protected
  void sse_encode_settings(Settings self, SseSerializer serializer);

  @protected
  void sse_encode_short_payment(ShortPayment self, SseSerializer serializer);

  @protected
  void sse_encode_short_payment_and_index(
      ShortPaymentAndIndex self, SseSerializer serializer);

  @protected
  void sse_encode_u_32(int self, SseSerializer serializer);

  @protected
  void sse_encode_u_64(BigInt self, SseSerializer serializer);

  @protected
  void sse_encode_u_8(int self, SseSerializer serializer);

  @protected
  void sse_encode_u_8_array_32(U8Array32 self, SseSerializer serializer);

  @protected
  void sse_encode_unit(void self, SseSerializer serializer);

  @protected
  void sse_encode_update_payment_note(
      UpdatePaymentNote self, SseSerializer serializer);

  @protected
  void sse_encode_usize(BigInt self, SseSerializer serializer);
}

// Section: wire_class

class AppRsWire implements BaseWire {
  factory AppRsWire.fromExternalLibrary(ExternalLibrary lib) =>
      AppRsWire(lib.ffiDynamicLibrary);

  /// Holds the symbol lookup function.
  final ffi.Pointer<T> Function<T extends ffi.NativeType>(String symbolName)
      _lookup;

  /// The symbols are looked up in [dynamicLibrary].
  AppRsWire(ffi.DynamicLibrary dynamicLibrary)
      : _lookup = dynamicLibrary.lookup;

  void rust_arc_increment_strong_count_RustOpaque_App(
    ffi.Pointer<ffi.Void> ptr,
  ) {
    return _rust_arc_increment_strong_count_RustOpaque_App(
      ptr,
    );
  }

  late final _rust_arc_increment_strong_count_RustOpaque_AppPtr =
      _lookup<ffi.NativeFunction<ffi.Void Function(ffi.Pointer<ffi.Void>)>>(
          'frbgen_app_rs_dart_rust_arc_increment_strong_count_RustOpaque_App');
  late final _rust_arc_increment_strong_count_RustOpaque_App =
      _rust_arc_increment_strong_count_RustOpaque_AppPtr
          .asFunction<void Function(ffi.Pointer<ffi.Void>)>();

  void rust_arc_decrement_strong_count_RustOpaque_App(
    ffi.Pointer<ffi.Void> ptr,
  ) {
    return _rust_arc_decrement_strong_count_RustOpaque_App(
      ptr,
    );
  }

  late final _rust_arc_decrement_strong_count_RustOpaque_AppPtr =
      _lookup<ffi.NativeFunction<ffi.Void Function(ffi.Pointer<ffi.Void>)>>(
          'frbgen_app_rs_dart_rust_arc_decrement_strong_count_RustOpaque_App');
  late final _rust_arc_decrement_strong_count_RustOpaque_App =
      _rust_arc_decrement_strong_count_RustOpaque_AppPtr
          .asFunction<void Function(ffi.Pointer<ffi.Void>)>();
}
