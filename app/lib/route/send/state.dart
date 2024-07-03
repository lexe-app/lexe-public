// Send payment flow states and types

// ignore_for_file: camel_case_types

import 'package:flutter/material.dart' show immutable;
import 'package:lexeapp/address_format.dart' as address_format;
import 'package:lexeapp/app_rs/ffi/ffi.dart'
    show
        AppHandle,
        Balance,
        ClientPaymentId,
        ConfirmationPriority,
        Invoice,
        Network,
        Onchain,
        PayInvoiceRequest,
        PayOnchainRequest,
        PaymentIndex,
        PaymentKind,
        PaymentMethod,
        PaymentMethod_Invoice,
        PaymentMethod_Offer,
        PaymentMethod_Onchain,
        PreflightPayInvoiceRequest,
        PreflightPayInvoiceResponse,
        PreflightPayOnchainRequest,
        PreflightPayOnchainResponse,
        paymentUriResolveBest;
import 'package:lexeapp/app_rs/ffi/ffi.ext.dart';
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/result.dart';

/// The outcome of a successful send flow.
@immutable
final class SendFlowResult {
  const SendFlowResult({required this.kind, required this.index});

  final PaymentKind kind;
  final PaymentIndex index;

  @override
  String toString() => "($kind, $index)";
}

/// An enum containing all the different major states for the outbound send
/// payment flow (see: `app/lib/route/send/page.dart`).
@immutable
sealed class SendState {}

/// Initial state if we're just beginning a send flow with no extra user input.
@immutable
class SendState_NeedUri implements SendState {
  const SendState_NeedUri({
    required this.app,
    required this.configNetwork,
    required this.balance,
    required this.cid,
  });

  final AppHandle app;
  final Network configNetwork;
  final Balance balance;
  final ClientPaymentId cid;

  /// Parse the payment URI (address, invoice, offer, BIP21, LN URI, ...) and
  /// check that it's valid for our current network (mainnet, testnet, ...).
  /// Then, if the payment already has an amount attached, try to preflight it
  /// immediately.
  Future<Result<SendState, String>> resolveAndMaybePreflight(
      String uriStr) async {
    // Try to parse and resolve the payment URI into a single "best" PaymentMethod.
    // TODO(phlip9): this API should return a bare error enum and flutter should
    // convert that to a human-readable error message (for translations).
    final result = await Result.tryFfiAsync(() async =>
        paymentUriResolveBest(network: this.configNetwork, uriStr: uriStr));

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

    final needAmountSendCtx = SendState_NeedAmount(
      app: this.app,
      configNetwork: this.configNetwork,
      balance: this.balance,
      cid: this.cid,
      paymentMethod: paymentMethod,
    );

    // Try preflighting the payment if it already has an amount set.
    final int? maybePreflightableAmount =
        needAmountSendCtx.canPreflightImmediately();

    // Can't preflight yet, need user to enter amount.
    if (maybePreflightableAmount == null) return Ok(needAmountSendCtx);

    // Preflight payment
    final int amountSats = maybePreflightableAmount;
    return (await needAmountSendCtx.preflight(amountSats)).mapErr((err) {
      error("Error preflighting payment: $err");
      return err.message;
    });
  }
}

/// State needed when we've resolved a "best" [PaymentMethod], but still need
/// to collect an amount from the user.
@immutable
class SendState_NeedAmount implements SendState {
  const SendState_NeedAmount({
    required this.app,
    required this.configNetwork,
    required this.balance,
    required this.cid,
    required this.paymentMethod,
  });

  final AppHandle app;
  final Network configNetwork;
  final Balance balance;
  final ClientPaymentId cid;

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
  Future<FfiResult<SendState_Preflighted>> preflight(
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
          fallbackAmountSats: (invoice.amountSats == null) ? amountSats : null,
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

    return Ok(SendState_Preflighted(
      app: this.app,
      configNetwork: this.configNetwork,
      balance: this.balance,
      cid: this.cid,
      preflightedPayment: preflighted,
    ));
  }
}

/// State after we've successfully preflighted a payment and are just waiting
/// for the user to confirm (and maybe tweak the note or fee priority).
@immutable
class SendState_Preflighted implements SendState {
  const SendState_Preflighted({
    required this.app,
    required this.configNetwork,
    required this.balance,
    required this.cid,
    required this.preflightedPayment,
  });

  final AppHandle app;
  final Network configNetwork;
  final Balance balance;
  final ClientPaymentId cid;

  final PreflightedPayment preflightedPayment;

  int balanceSats() =>
      this.balance.balanceByKind(this.preflightedPayment.kind());

  /// The user is now confirming/sending this payment
  Future<FfiResult<SendFlowResult>> pay(
    final String? note,
    // Only used for Onchain
    final ConfirmationPriority? confPriority,
  ) async {
    final preflighted = this.preflightedPayment;
    return switch (preflighted) {
      PreflightedPayment_Onchain() =>
        await this.payOnchain(preflighted, note, confPriority!),
      PreflightedPayment_Invoice() => await this.payInvoice(preflighted, note),
      PreflightedPayment_Offer() =>
        throw UnimplementedError("BOLT12 offers are unsupported"),
    };
  }

  Future<FfiResult<SendFlowResult>> payOnchain(
    final PreflightedPayment_Onchain preflighted,
    final String? note,
    final ConfirmationPriority confPriority,
  ) async {
    final req = PayOnchainRequest(
      cid: this.cid,
      address: preflighted.onchain.address,
      amountSats: preflighted.amountSats,
      priority: confPriority,
      note: note,
    );

    return (await Result.tryFfiAsync(() async => this.app.payOnchain(req: req)))
        .map((resp) => SendFlowResult(
              kind: PaymentKind.onchain,
              index: resp.index,
            ));
  }

  Future<FfiResult<SendFlowResult>> payInvoice(
    final PreflightedPayment_Invoice preflighted,
    final String? note,
  ) async {
    final req = PayInvoiceRequest(
      invoice: preflighted.invoice.string,
      fallbackAmountSats: (preflighted.invoice.amountSats == null)
          ? preflighted.amountSats
          : null,
      note: note,
    );

    return (await Result.tryFfiAsync(() async => this.app.payInvoice(req: req)))
        .map((resp) => SendFlowResult(
              kind: PaymentKind.invoice,
              index: resp.index,
            ));
  }
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
  PaymentKind kind() => PaymentKind.invoice;
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
  PaymentKind kind() => PaymentKind.onchain;
}

// TODO(phlip9): impl BOLT12 offer
class PreflightedPayment_Offer implements PreflightedPayment {
  const PreflightedPayment_Offer();

  @override
  PaymentKind kind() => throw UnimplementedError();
}
