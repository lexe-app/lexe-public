// Claim (pull) payment flow states and types

// ignore_for_file: camel_case_types

/// @docImport 'package:lexeapp/route/send/state.dart';
/// @docImport 'package:lexeapp/route/uri/state.dart';
library;

import 'package:app_rs_dart/ffi/api.dart';
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart'
    show ClaimMethod, ClaimMethod_LnurlWithdraw, LnurlWithdrawRequest, Payment;
import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart' show immutable;
import 'package:lexeapp/prelude.dart';

/// The outcome of a successful claim flow.
///
/// See also: [SendFlowResult]
@immutable
final class ClaimFlowResult {
  const ClaimFlowResult({required this.payment});

  /// We return an "unsynced" Payment from the claim flow, for the user
  /// to view before the app gets synced with the node
  final Payment payment;

  @override
  String toString() => "(${payment.kind}, ${payment.index})";
}

/// States for the inbound claim payment flow.
///
/// [NeedUriState]
/// -> [ClaimState_NeedAmount]
/// -> [ClaimState_NeedConfirm]
/// -> [ClaimFlowResult]
@immutable
sealed class ClaimState {}

/// State we're in if we've resolved a [ClaimMethod] but still need to colect
/// an amount from the user.
class ClaimState_NeedAmount extends ClaimState {
  ClaimState_NeedAmount({
    required this.claimMethod,
    required this.app,
    required this.fiatRate,
  });

  final AppHandle app;
  final ValueListenable<FiatRate?> fiatRate;

  final ClaimMethod claimMethod;

  /// Advance state to [ClaimState_NeedConfirm] with the given info
  ClaimState_NeedConfirm withAmount(
    int amountSats, {
    String? message,
    String? personalNote,
  }) {
    switch (this.claimMethod) {
      case ClaimMethod_LnurlWithdraw(:final httpUrl, :final withdrawRequest):
        return ClaimState_NeedConfirm(
          app: this.app,
          fiatRate: this.fiatRate,
          claimable: ClaimReady_LnurlWithdraw(
            httpUrl: httpUrl,
            withdrawRequest: withdrawRequest,
            amountMsat: amountSats * 1000,
            description: message,
            personalNote: personalNote,
          ),
        );
    }
  }
}

/// State we're in if we have all the info. needed to make the claim,
/// and just need the user to confirm.
/// The user can also edit non-wire info. like the personal note here.
class ClaimState_NeedConfirm extends ClaimState {
  ClaimState_NeedConfirm({
    required this.app,
    required this.claimable,
    required this.fiatRate,
  });

  final AppHandle app;
  final ValueListenable<FiatRate?> fiatRate;

  final ClaimReady claimable;

  Future<FfiResult<ClaimFlowResult>> claim({String? personalNote}) async {
    switch (this.claimable) {
      case ClaimReady_LnurlWithdraw claimParams:
        final newPersonalNote = personalNote ?? claimParams.personalNote;
        final req = WithdrawLnurlRequest(
          withdrawRequest: claimParams.withdrawRequest,
          amountMsat: claimParams.amountMsat,
          description: claimParams.description,
          personalNote: newPersonalNote,
        );
        final result = await Result.tryFfiAsync(
          () => this.app.withdrawLnurl(req: req),
        );
        return result.map((payment) => ClaimFlowResult(payment: payment));
    }
  }
}

/// A request-ready claimable method.
sealed class ClaimReady {}

class ClaimReady_LnurlWithdraw extends ClaimReady {
  ClaimReady_LnurlWithdraw({
    required this.httpUrl,
    required this.withdrawRequest,
    required this.amountMsat,
    this.description,
    this.personalNote,
  });

  final String httpUrl;
  final LnurlWithdrawRequest withdrawRequest;
  final int amountMsat;
  final String? description;
  final String? personalNote;
}
