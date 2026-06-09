import 'package:app_rs_dart/ffi/api.dart';
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart'
    show ClaimMethod, ClientPaymentId, Network, PaymentMethod;
import 'package:flutter/foundation.dart';
import 'package:lexeapp/address_format.dart'
    as address_format
    show ellipsizeBtcAddress;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/send/state.dart';

/// The outcome of a successful URI payment flow.
@immutable
final class UriFlowResult {
  const UriFlowResult({required this.sendFlowResult});
  // TODO(nicole): augment into claim/payment result sum type when adding claim flow
  final SendFlowResult sendFlowResult;
}

/// Initial state if we're beginning a URI-based flow with no extra user input.
///
/// Advances to [SendState] via [enterSendFlow]
class NeedUriState {
  NeedUriState({
    required this.app,
    required this.configNetwork,
    required this.balance,
    required this.cid,
    required this.fiatRate,
  });
  final AppHandle app;
  final Network configNetwork;
  final Balance balance;
  final ClientPaymentId cid;
  final ValueListenable<FiatRate?> fiatRate;

  Future<Result<(PaymentMethod?, ClaimMethod?), String>> resolve(
    String uriStr,
  ) async {
    // Parse and resolve the URI
    // TODO(phlip9): this API should return a bare error enum and flutter should
    // convert that to a human-readable error message (for translations).
    final result = await Result.tryFfiAsync(
      () => this.app.resolveBest(uriStr: uriStr, network: this.configNetwork),
    );

    // Check if resolving was successful.
    final PaymentMethod? paymentMethod;
    final ClaimMethod? claimMethod;
    switch (result) {
      case Ok(:final ok):
        paymentMethod = ok.$1;
        claimMethod = ok.$2;
      case Err(:final err):
        error("Error resolving URI: $err");
        return Err(err.message);
    }

    final uriStrShort = address_format.ellipsizeBtcAddress(uriStr);
    info(
      "Resolved input '$uriStrShort' to payment method: $paymentMethod and claim method: $claimMethod",
    );

    return Ok((paymentMethod, claimMethod));
  }

  /// Preflight the payment if possible and return the next [SendState]
  Future<Result<SendState, String>> enterSendFlow(
    PaymentMethod paymentMethod,
  ) async {
    final needAmountSendCtx = SendState_NeedAmount(
      app: this.app,
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
