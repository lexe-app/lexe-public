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
import 'package:app_rs_dart/ffi/app.dart';
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
import 'package:lexeapp/prelude.dart';

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

/// State needed when we've resolved a "best" [PaymentMethod], but still need
/// to collect an amount from the user.
@immutable
class SendState_NeedAmount implements SendState {
  const SendState_NeedAmount({
    required this.app,
    required this.configNetwork,
    required this.balance,
    required this.cid,
    required this.fiatRate,
    required this.paymentMethod,
  });

  final AppHandle app;
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
    PaymentMethod_Offer(:final field0) => field0.bip321AmountSats,
    PaymentMethod_LnurlPayRequest(:final field0) =>
      field0.minSendableMsat == field0.maxSendableMsat
          ? field0.minSendableMsat ~/ 1000
          : null,
  };

  /// Using the current [PaymentMethod], preflight the payment with the given
  /// amount. An optional [message] can be sent to the recipient.
  Future<FfiResult<SendState_Preflighted>> preflight(
    final int amountSats, {
    final String? message,
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

        final result = await Result.tryFfiAsync(
          () => this.app.preflightPayOnchain(req: req),
        );

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

        final result = await Result.tryFfiAsync(
          () => this.app.preflightPayInvoice(req: req),
        );

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
          amountSats: amountSats,
        );

        final result = await Result.tryFfiAsync(
          () => this.app.preflightPayOffer(req: req),
        );

        switch (result) {
          case Ok(:final ok):
            preflighted = PreflightedPayment_Offer(
              offer: offer,
              amountSats: amountSats,
              preflight: ok,
              message: message,
            );
          case Err(:final err):
            return Err(err);
        }

      case PaymentMethod_LnurlPayRequest(:final field0):
        final lnurlPayRequest = field0;
        final result = await Result.tryFfiAsync(
          () => this.app.resolveLnurlPayRequest(
            req: lnurlPayRequest,
            amountMsats: amountSats * 1000,
            comment: message,
          ),
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

        final preflightResult = await Result.tryFfiAsync(
          () => this.app.preflightPayInvoice(req: req),
        );

        switch (preflightResult) {
          case Ok(:final ok):
            preflighted = PreflightedPayment_Invoice(
              invoice: invoice,
              amountSats: amountSats,
              preflight: ok,
              message: message,
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
        app: this.app,
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
    required this.app,
    required this.configNetwork,
    required this.balance,
    required this.cid,
    required this.fiatRate,
    required this.preflightedPayment,
  });

  final AppHandle app;
  final Network configNetwork;
  final Balance balance;
  final ClientPaymentId cid;
  final ValueListenable<FiatRate?> fiatRate;

  final PreflightedPayment preflightedPayment;

  /// The user is now confirming/sending this payment
  Future<FfiResult<SendFlowResult>> pay(
    final String? personalNote,
    // Only used for Onchain
    final ConfirmationPriority? confPriority,
  ) async {
    final preflighted = this.preflightedPayment;
    return switch (preflighted) {
      PreflightedPayment_Onchain() => await this.payOnchain(
        preflighted,
        personalNote,
        confPriority!,
      ),
      PreflightedPayment_Invoice() => await this.payInvoice(
        preflighted,
        personalNote,
      ),
      PreflightedPayment_Offer() => await this.payOffer(
        preflighted,
        personalNote,
      ),
    };
  }

  Future<FfiResult<SendFlowResult>> payOnchain(
    final PreflightedPayment_Onchain preflighted,
    final String? personalNote,
    final ConfirmationPriority confPriority,
  ) async {
    final req = PayOnchainRequest(
      cid: this.cid,
      address: preflighted.onchain.address,
      amountSats: preflighted.amountSats,
      priority: confPriority,
      personalNote: personalNote,
    );

    final preflight = preflighted.preflight;
    final estimatedFee = switch (confPriority) {
      ConfirmationPriority.high => preflight.high ?? preflight.normal,
      ConfirmationPriority.normal => preflight.normal,
      ConfirmationPriority.background => preflight.background,
    };

    final res = await Result.tryFfiAsync(() => this.app.payOnchain(req: req));
    return res.map(
      (resp) => SendFlowResult(
        payment: Payment(
          index: resp.index,
          kind: const PaymentKind_Onchain(),
          direction: PaymentDirection.outbound,
          status: PaymentStatus.pending,
          statusStr: "syncing from node",
          personalNote: personalNote,

          // Choose some reasonable values until we can get these from the
          // response.

          // TODO(phlip9): get this from resp/index
          createdAt: DateTime.now().toUtc().millisecondsSinceEpoch,
          // TODO(phlip9): get from resp
          amountSats: preflighted.amountSats,
          // TODO(phlip9): get from resp
          feesSats: estimatedFee.amountSats,
        ),
      ),
    );
  }

  Future<FfiResult<SendFlowResult>> payInvoice(
    final PreflightedPayment_Invoice preflighted,
    final String? personalNote,
  ) async {
    final req = PayInvoiceRequest(
      invoice: preflighted.invoice.string,
      fallbackAmountSats: (preflighted.invoice.amountSats == null)
          ? preflighted.amountSats
          : null,
      message: preflighted.message,
      personalNote: personalNote,
    );

    final res = await Result.tryFfiAsync(() => this.app.payInvoice(req: req));
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
          personalNote: personalNote,

          // Choose some reasonable values until we can get these from the
          // response.

          // TODO(phlip9): get from resp/index
          createdAt: DateTime.now().toUtc().millisecondsSinceEpoch,
          // TODO(phlip9): get from resp
          amountSats: preflighted.preflight.amountSats,
          // TODO(phlip9): get from resp
          feesSats: preflighted.preflight.feesSats,
        ),
      ),
    );
  }

  Future<FfiResult<SendFlowResult>> payOffer(
    final PreflightedPayment_Offer preflighted,
    final String? personalNote,
  ) async {
    final req = PayOfferRequest(
      cid: this.cid,
      offer: preflighted.offer.string,
      amountSats: preflighted.amountSats,
      message: preflighted.message,
      personalNote: personalNote,
    );

    final res = await Result.tryFfiAsync(() => this.app.payOffer(req: req));
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
          message: preflighted.message,
          personalNote: personalNote,

          // Choose some reasonable values until we can get these from the
          // response.

          // TODO(phlip9): get from resp/index
          createdAt: DateTime.now().toUtc().millisecondsSinceEpoch,
          // TODO(phlip9): get from resp
          amountSats: preflighted.preflight.amountSats,
          // TODO(phlip9): get from resp
          feesSats: preflighted.preflight.feesSats,
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
    this.message,
    this.sendTo,
  });

  final Invoice invoice;
  final int amountSats;
  final PreflightPayInvoiceResponse preflight;

  /// Message sent to the recipient.
  final String? message;
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
    required this.message,
  });

  final Offer offer;
  final int amountSats;
  final PreflightPayOfferResponse preflight;
  final String? message;

  @override
  PaymentKind kind() => const PaymentKind_Offer();
}
