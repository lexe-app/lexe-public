// Send payment flow states and types

// ignore_for_file: camel_case_types

import 'package:app_rs_dart/ffi/api.dart'
    show
        Balance,
        FiatRate,
        PayInvoiceRequest,
        PayOfferRequest,
        PayOnchainRequest,
        PreflightPayInvoiceRequest,
        PreflightPayInvoiceResponse,
        PreflightPayOfferRequest,
        PreflightPayOfferResponse,
        PreflightPayOnchainRequest,
        PreflightPayOnchainResponse;
import 'package:app_rs_dart/ffi/types.dart'
    show
        ClientPaymentId,
        ConfirmationPriority,
        Invoice,
        Network,
        Offer,
        Onchain,
        Payment,
        PaymentDirection,
        PaymentKind,
        PaymentKind_Invoice,
        PaymentKind_Offer,
        PaymentKind_Onchain,
        PaymentMethod,
        PaymentMethod_Invoice,
        PaymentMethod_LnurlPayRequest,
        PaymentMethod_Offer,
        PaymentMethod_Onchain,
        PaymentStatus;
import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart' show immutable;
import 'package:lexeapp/address_format.dart' as address_format;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/service/send_payment_service.dart'
    show SendPaymentService;

/// The outcome of a successful send flow.
@immutable
final class SendFlowResult {
  const SendFlowResult({required this.payment});

  /// We return an "unsynced" Payment from the send flow.
  ///
  /// It's not the canonical version in the node, rather it's generated locally.
  /// After sending a payment, we need to show something reasonable on the
  /// payment page until we actually sync the canonical Payment from the node to
  /// the local DB.
  final Payment payment;

  @override
  String toString() => "(${payment.kind}, ${payment.index})";
}

/// An enum containing all the different major states for the outbound send
/// payment flow (see: `app/lib/route/send/page.dart`).
@immutable
sealed class SendState {}

/// Initial state if we're just beginning a send flow with no extra user input.
@immutable
class SendState_NeedUri implements SendState {
  const SendState_NeedUri({
    required this.paymentService,
    required this.configNetwork,
    required this.balance,
    required this.cid,
    required this.fiatRate,
  });

  final SendPaymentService paymentService;
  final Network configNetwork;
  final Balance balance;
  final ClientPaymentId cid;
  final ValueListenable<FiatRate?> fiatRate;

  /// Parse the payment URI (address, invoice, offer, BIP21, LN URI, ...) and
  /// check that it's valid for our current network (mainnet, testnet, ...).
  /// Then, if the payment already has an amount attached, try to preflight it
  /// immediately.
  Future<Result<SendState, String>> resolveAndMaybePreflight(
    String uriStr,
  ) async {
    // Try to parse and resolve the payment URI into a single "best" PaymentMethod.
    // TODO(phlip9): this API should return a bare error enum and flutter should
    // convert that to a human-readable error message (for translations).
    final result = await this.paymentService.resolveBest(
      network: this.configNetwork,
      uriStr: uriStr,
    );

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
      paymentService: this.paymentService,
      configNetwork: this.configNetwork,
      balance: this.balance,
      cid: this.cid,
      fiatRate: this.fiatRate,
      paymentMethod: paymentMethod,
    );

    // Try preflighting the payment if it already has an amount set.
    final int? maybePreflightableAmount = needAmountSendCtx
        .canPreflightImmediately();

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
    required this.paymentService,
    required this.configNetwork,
    required this.balance,
    required this.cid,
    required this.fiatRate,
    required this.paymentMethod,
  });

  final SendPaymentService paymentService;
  final Network configNetwork;
  final Balance balance;
  final ClientPaymentId cid;
  final ValueListenable<FiatRate?> fiatRate;

  /// The current payment method (onchain send, BOLT11 invoice send, BOLT12
  /// offer send) and associated details, like amount or description.
  ///
  /// If we're just hitting the "Send" button on the main wallet page, this will
  /// be null.
  /// Otherwise, this will be non-null if we're coming from a QR code scan or
  /// URI open.
  final PaymentMethod paymentMethod;

  /// Returns Some amount if this payment method already has an amount attached
  /// and can be preflighted immediately.
  int? canPreflightImmediately() => switch (this.paymentMethod) {
    PaymentMethod_Onchain(:final field0) => field0.amountSats,
    PaymentMethod_Invoice(:final field0) => field0.amountSats,
    PaymentMethod_Offer(:final field0) => field0.amountSats,
    PaymentMethod_LnurlPayRequest(:final field0) =>
      field0.minSendableMsat == field0.maxSendableMsat
          ? field0.minSendableMsat ~/ 1000
          : null,
  };

  /// Using the current [PaymentMethod], preflight the payment with the given
  /// amount. An optional [payerNote] can be sent to the recipient.
  Future<FfiResult<SendState_Preflighted>> preflight(
    final int amountSats, {
    final String? payerNote,
  }) async {
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

        final result = await this.paymentService.preflightPayOnchain(req: req);

        switch (result) {
          case Ok(:final ok):
            preflighted = PreflightedPayment_Onchain(
              onchain: onchain,
              amountSats: amountSats,
              preflight: ok,
            );
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

        final result = await this.paymentService.preflightPayInvoice(req: req);

        switch (result) {
          case Ok(:final ok):
            preflighted = PreflightedPayment_Invoice(
              invoice: invoice,
              amountSats: amountSats,
              preflight: ok,
            );
          case Err(:final err):
            return Err(err);
        }

      // BOLT12 Offer
      case PaymentMethod_Offer(:final field0):
        final offer = field0;
        final req = PreflightPayOfferRequest(
          cid: this.cid,
          offer: offer.string,
          fallbackAmountSats: (offer.amountSats == null) ? amountSats : null,
        );

        final result = await this.paymentService.preflightPayOffer(req: req);

        switch (result) {
          case Ok(:final ok):
            preflighted = PreflightedPayment_Offer(
              offer: offer,
              amountSats: amountSats,
              preflight: ok,
              payerNote: payerNote,
            );
          case Err(:final err):
            return Err(err);
        }

      case PaymentMethod_LnurlPayRequest(:final field0):
        final lnurlPayRequest = field0;
        final result = await this.paymentService.resolveLnurlPayRequest(
          req: lnurlPayRequest,
          amountMsats: amountSats * 1000,
          comment: payerNote,
        );

        final Invoice invoice;
        switch (result) {
          case Ok(:final ok):
            invoice = ok;
          case Err(:final err):
            return Err(err);
        }

        final req = PreflightPayInvoiceRequest(
          invoice: invoice.string,
          fallbackAmountSats: (invoice.amountSats == null) ? amountSats : null,
        );

        final preflightResult = await this.paymentService.preflightPayInvoice(
          req: req,
        );

        switch (preflightResult) {
          case Ok(:final ok):
            preflighted = PreflightedPayment_Invoice(
              invoice: invoice,
              amountSats: amountSats,
              preflight: ok,
              payerNote: payerNote,
              sendTo:
                  lnurlPayRequest.metadata.email ??
                  lnurlPayRequest.metadata.identifier,
            );
          case Err(:final err):
            return Err(err);
        }
    }

    return Ok(
      SendState_Preflighted(
        paymentService: this.paymentService,
        configNetwork: this.configNetwork,
        balance: this.balance,
        cid: this.cid,
        fiatRate: this.fiatRate,
        preflightedPayment: preflighted,
      ),
    );
  }
}

/// State after we've successfully preflighted a payment and are just waiting
/// for the user to confirm (and maybe tweak the note or fee priority).
@immutable
class SendState_Preflighted implements SendState {
  const SendState_Preflighted({
    required this.paymentService,
    required this.configNetwork,
    required this.balance,
    required this.cid,
    required this.fiatRate,
    required this.preflightedPayment,
  });

  final SendPaymentService paymentService;
  final Network configNetwork;
  final Balance balance;
  final ClientPaymentId cid;
  final ValueListenable<FiatRate?> fiatRate;

  final PreflightedPayment preflightedPayment;

  /// The user is now confirming/sending this payment
  Future<FfiResult<SendFlowResult>> pay(
    final String? note,
    // Only used for Onchain
    final ConfirmationPriority? confPriority,
  ) async {
    final preflighted = this.preflightedPayment;
    return switch (preflighted) {
      PreflightedPayment_Onchain() => await this.payOnchain(
        preflighted,
        note,
        confPriority!,
      ),
      PreflightedPayment_Invoice() => await this.payInvoice(preflighted, note),
      PreflightedPayment_Offer() => await this.payOffer(preflighted, note),
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

    final preflight = preflighted.preflight;
    final estimatedFee = switch (confPriority) {
      ConfirmationPriority.high => preflight.high ?? preflight.normal,
      ConfirmationPriority.normal => preflight.normal,
      ConfirmationPriority.background => preflight.background,
    };

    final res = await this.paymentService.payOnchain(req: req);
    return res.map(
      (resp) => SendFlowResult(
        payment: Payment(
          index: resp.index,
          kind: const PaymentKind_Onchain(),
          direction: PaymentDirection.outbound,
          status: PaymentStatus.pending,
          statusStr: "syncing from node",
          note: note,

          // Choose some reasonable values until we can get these from the
          // response.

          // TODO(phlip9): get this from resp/index
          createdAt: DateTime.now().toUtc().millisecondsSinceEpoch,
          // TODO(phlip9): get from resp
          amountSat: preflighted.amountSats,
          // TODO(phlip9): get from resp
          feesSat: estimatedFee.amountSats,
        ),
      ),
    );
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

    final res = await this.paymentService.payInvoice(req: req);
    return res.map(
      (resp) => SendFlowResult(
        payment: Payment(
          index: resp.index,
          kind: const PaymentKind_Invoice(),
          direction: PaymentDirection.outbound,
          status: PaymentStatus.pending,
          statusStr: "syncing from node",
          invoice: preflighted.invoice,
          description: preflighted.invoice.description,
          note: note,

          // Choose some reasonable values until we can get these from the
          // response.

          // TODO(phlip9): get from resp/index
          createdAt: DateTime.now().toUtc().millisecondsSinceEpoch,
          // TODO(phlip9): get from resp
          amountSat: preflighted.preflight.amountSats,
          // TODO(phlip9): get from resp
          feesSat: preflighted.preflight.feesSats,
        ),
      ),
    );
  }

  Future<FfiResult<SendFlowResult>> payOffer(
    final PreflightedPayment_Offer preflighted,
    final String? note,
  ) async {
    final req = PayOfferRequest(
      cid: this.cid,
      offer: preflighted.offer.string,
      fallbackAmountSats: (preflighted.offer.amountSats == null)
          ? preflighted.amountSats
          : null,
      note: note,
      payerNote: preflighted.payerNote,
    );

    final res = await this.paymentService.payOffer(req: req);
    return res.map(
      (resp) => SendFlowResult(
        payment: Payment(
          index: resp.index,
          kind: const PaymentKind_Offer(),
          direction: PaymentDirection.outbound,
          status: PaymentStatus.pending,
          statusStr: "syncing from node",
          offer: preflighted.offer,
          description: preflighted.offer.description,
          note: note,
          payerNote: preflighted.payerNote,

          // Choose some reasonable values until we can get these from the
          // response.

          // TODO(phlip9): get from resp/index
          createdAt: DateTime.now().toUtc().millisecondsSinceEpoch,
          // TODO(phlip9): get from resp
          amountSat: preflighted.preflight.amountSats,
          // TODO(phlip9): get from resp
          feesSat: preflighted.preflight.feesSats,
        ),
      ),
    );
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
  const PreflightedPayment_Invoice({
    required this.invoice,
    required this.amountSats,
    required this.preflight,
    this.payerNote,
    this.sendTo,
  });

  final Invoice invoice;
  final int amountSats;
  final PreflightPayInvoiceResponse preflight;

  /// Message sent to the recipient, stored locally as the note.
  final String? payerNote;
  final String? sendTo;

  @override
  PaymentKind kind() => const PaymentKind_Invoice();
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
  PaymentKind kind() => const PaymentKind_Onchain();
}

class PreflightedPayment_Offer implements PreflightedPayment {
  const PreflightedPayment_Offer({
    required this.offer,
    required this.amountSats,
    required this.preflight,
    this.payerNote,
  });

  final Offer offer;
  final int amountSats;
  final PreflightPayOfferResponse preflight;
  final String? payerNote;

  @override
  PaymentKind kind() => const PaymentKind_Offer();
}
