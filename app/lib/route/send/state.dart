// Send payment flow states and types

// ignore_for_file: camel_case_types

import 'package:flutter/material.dart' show immutable;
import 'package:lexeapp/address_format.dart' as address_format;
import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/bindings_generated_api.dart'
    show
        AppHandle,
        Balance,
        ClientPaymentId,
        Invoice,
        Network,
        Onchain,
        PaymentKind,
        PaymentMethod,
        PaymentMethod_Invoice,
        PaymentMethod_Offer,
        PaymentMethod_Onchain,
        PreflightPayInvoiceRequest,
        PreflightPayInvoiceResponse,
        PreflightPayOnchainRequest,
        PreflightPayOnchainResponse;
import 'package:lexeapp/bindings_generated_api_ext.dart';
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/result.dart';

/// Common context used during any step in the send payment flow.
@immutable
class SendContext {
  const SendContext({
    required this.app,
    required this.configNetwork,
    required this.balance,
    required this.cid,
  });

  final AppHandle app;
  final Network configNetwork;

  /// The current spendable wallet balance, for all payment methods
  /// (LN, onchain).
  final Balance balance;

  /// A unique, client-generated id for payment types (onchain send,
  /// ln spontaneous send) that need an extra id for idempotency.
  final ClientPaymentId cid;

  /// Parse the payment URI (address, invoice, offer, BIP21, LN URI, ...) and
  /// check that it's valid for our current network (mainnet, testnet, ...).
  /// Then, if the payment already has an amount attached, try to preflight it
  /// immediately.
  Future<Result<(SendContext_Preflighted?, SendContext_NeedAmount), String?>>
      resolveAndMaybePreflight(String uriStr) async {
    // Try to parse and resolve the payment URI into a single "best" PaymentMethod.
    // TODO(phlip9): this API should return a bare error enum and flutter should
    // convert that to a human-readable error message (for translations).
    final result = await Result.tryFfiAsync(() async =>
        api.paymentUriResolveBest(network: this.configNetwork, uriStr: uriStr));

    // Check if resolving was successful.
    final PaymentMethod paymentMethod;
    switch (result) {
      case Ok(:final ok):
        paymentMethod = ok;
      case Err(:final err):
        error("Error resolving payment URI: $err");
        return Err(err.message);
    }

    final uriStrShort = address_format.ellipsizeBtcAddress(uriStr);
    info("Resolved input '$uriStrShort' to payment method: $paymentMethod");

    final needAmountSendCtx = SendContext_NeedAmount(
      app: this.app,
      configNetwork: this.configNetwork,
      balance: this.balance,
      cid: this.cid,
      paymentMethod: paymentMethod,
    );

    // Try preflighting the payment if it already has an amount set.
    final int? maybePreflightableAmount =
        needAmountSendCtx.canPreflightImmediately();
    final SendContext_Preflighted? preflightedSendCtx;

    if (maybePreflightableAmount != null) {
      final int amountSats = maybePreflightableAmount;

      final result = await needAmountSendCtx.preflight(amountSats);

      // Check if payment preflight was successful, or show an error message.
      switch (result) {
        case Ok(:final ok):
          preflightedSendCtx = ok;
        case Err(:final err):
          error("Error preflighting payment: $err");
          return Err(err.message);
      }
    } else {
      preflightedSendCtx = null;
    }

    return Ok((preflightedSendCtx, needAmountSendCtx));
  }
}

/// Context needed when we're collecting an amount from the user for a potential
/// [PaymentMethod].
@immutable
class SendContext_NeedAmount extends SendContext {
  const SendContext_NeedAmount({
    required super.app,
    required super.configNetwork,
    required super.balance,
    required super.cid,
    required this.paymentMethod,
  });

  /// The current payment method (onchain send, BOLT11 invoice send, BOLT12
  /// offer send) and associated details, like amount or description.
  ///
  /// If we're just hitting the "Send" button on the main wallet page, this will
  /// be null.
  /// Otherwise, this will be non-null if we're coming from a QR code scan or
  /// URI open.
  final PaymentMethod paymentMethod;

  int balanceSats() => this.balance.balanceByKind(this.paymentMethod.kind());

  /// Returns Some amount if this payment method already has an amount attached
  /// and can be preflighted immediately.
  int? canPreflightImmediately() => switch (this.paymentMethod) {
        PaymentMethod_Onchain(:final field0) => field0.amountSats,
        PaymentMethod_Invoice(:final field0) => field0.amountSats,
        PaymentMethod_Offer() =>
          throw UnimplementedError("BOLT12 offers unsupported"),
      };

  /// Using the current [PaymentMethod], preflight the payment with the given
  /// amount.
  Future<FfiResult<SendContext_Preflighted>> preflight(
    final int amountSats,
  ) async {
    final paymentMethod = this.paymentMethod;

    final PreflightedPayment preflighted;
    switch (paymentMethod) {
      // Onchain
      case PaymentMethod_Onchain(:final field0):
        final onchain = field0;

        final req = PreflightPayOnchainRequest(
          address: onchain.address,
          amountSats: amountSats,
        );

        final result = await Result.tryFfiAsync(
            () async => this.app.preflightPayOnchain(req: req));

        switch (result) {
          case Ok(:final ok):
            preflighted = PreflightedPayment_Onchain(
                onchain: onchain, amountSats: amountSats, preflight: ok);
          case Err(:final err):
            return Err(err);
        }

      // BOLT11 Invoice
      case PaymentMethod_Invoice(:final field0):
        final invoice = field0;

        final req = PreflightPayInvoiceRequest(
          invoice: invoice.string,
          fallbackAmountSats: amountSats,
        );

        final result = await Result.tryFfiAsync(
            () async => this.app.preflightPayInvoice(req: req));

        switch (result) {
          case Ok(:final ok):
            preflighted = PreflightedPayment_Invoice(
                invoice: invoice, amountSats: amountSats, preflight: ok);
          case Err(:final err):
            return Err(err);
        }

      // BOLT12 Offer
      case PaymentMethod_Offer():
        throw UnimplementedError("BOLT12 offers not supported");
    }

    return Ok(SendContext_Preflighted(
      app: this.app,
      configNetwork: this.configNetwork,
      balance: this.balance,
      cid: this.cid,
      preflightedPayment: preflighted,
    ));
  }
}

/// Context after we've successfully preflighted a payment and are just waiting
/// for the user to confirm (and maybe tweak the note or fee priority).
@immutable
class SendContext_Preflighted extends SendContext {
  const SendContext_Preflighted({
    required super.app,
    required super.configNetwork,
    required super.balance,
    required super.cid,
    required this.preflightedPayment,
  });

  final PreflightedPayment preflightedPayment;

  int balanceSats() =>
      this.balance.balanceByKind(this.preflightedPayment.kind());
}

/// A preflighted [PaymentMethod] -- the user's node has made sure the payment
/// checks out and is ready to send, without actually sending it.
///
/// For example, an [Onchain] payment will check against our balance and get
/// network fee estimates, while an [Invoice] will check our balance and
/// liquidity, then find a route and get a fee estimate.
@immutable
sealed class PreflightedPayment {
  PaymentKind kind();
}

@immutable
class PreflightedPayment_Invoice implements PreflightedPayment {
  const PreflightedPayment_Invoice(
      {required this.invoice,
      required this.amountSats,
      required this.preflight});

  final Invoice invoice;
  final int amountSats;
  final PreflightPayInvoiceResponse preflight;

  @override
  PaymentKind kind() => PaymentKind.Invoice;
}

@immutable
class PreflightedPayment_Onchain implements PreflightedPayment {
  const PreflightedPayment_Onchain({
    required this.onchain,
    required this.amountSats,
    required this.preflight,
  });

  final Onchain onchain;
  final int amountSats;
  final PreflightPayOnchainResponse preflight;

  @override
  PaymentKind kind() => PaymentKind.Onchain;
}

// TODO(phlip9): impl BOLT12 offer
class PreflightedPayment_Offer implements PreflightedPayment {
  const PreflightedPayment_Offer();

  @override
  PaymentKind kind() => throw UnimplementedError();
}
