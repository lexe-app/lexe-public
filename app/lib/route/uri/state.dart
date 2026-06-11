// URI-derived payment flow state (branches into both send and claim flows)

// ignore_for_file: camel_case_types

import 'package:app_rs_dart/ffi/api.dart';
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart'
    show ClaimMethod, ClientPaymentId, Network, PaymentMethod;
import 'package:app_rs_dart/ffi/types.ext.dart' show ClaimMethodExt;
import 'package:flutter/foundation.dart';
import 'package:lexeapp/address_format.dart'
    as address_format
    show ellipsizeBtcAddress;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/claim/state.dart'
    show ClaimFlowResult, ClaimState, ClaimState_NeedAmount;
import 'package:lexeapp/route/send/state.dart';

/// The outcome of a successful URI payment flow.
@immutable
sealed class UriFlowResult {
  const UriFlowResult();
}

class UriFlowResult_Send implements UriFlowResult {
  const UriFlowResult_Send(this.sendFlowResult);
  final SendFlowResult sendFlowResult;
}

class UriFlowResult_Claim implements UriFlowResult {
  const UriFlowResult_Claim(this.claimFlowResult);
  final ClaimFlowResult claimFlowResult;
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
    // TODO(nicole): all we check for is amount, so for something like lnurl
    // which accepts comments, the comment gets unconditionally skipped
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

  /// Enter a claim flow, returning the next [ClaimState]
  ClaimState enterClaimFlow(ClaimMethod claimMethod) {
    final needAmountClaimCtx = ClaimState_NeedAmount(
      app: this.app,
      fiatRate: this.fiatRate,
      claimMethod: claimMethod,
    );

    // Check for an amount
    final int? fixedAmount = claimMethod.fixedAmountSats();
    if (fixedAmount == null) {
      // No fixed amount, need user to enter amount.
      return needAmountClaimCtx;
    }

    // Otherwise, we can skip to the confirmation step
    // TODO(nicole): all we check for is amount, so for something like lnurl
    // which accepts messages, the message gets unconditionally skipped
    return needAmountClaimCtx.withAmount(fixedAmount);
  }
}
