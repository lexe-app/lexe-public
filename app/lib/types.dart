import 'package:app_rs_dart/ffi/api.dart' show Balance, FiatRate;
import 'package:app_rs_dart/ffi/types.dart' show PaymentKind;
import 'package:freezed_annotation/freezed_annotation.dart' show freezed;
import 'package:lexeapp/currency_format.dart' as currency_format;

// Include code generated by @freezed
part 'types.freezed.dart';

/// A Lexe user has multiple balances. Currently we support on-chain Bitcoin and
/// outbound Lightning channel balances.
enum BalanceKind {
  onchain,
  lightning;

  static BalanceKind fromPaymentKind(final PaymentKind kind) => switch (kind) {
        PaymentKind.onchain => BalanceKind.onchain,
        PaymentKind.invoice || PaymentKind.spontaneous => BalanceKind.lightning,
      };
}

/// The current wallet balances, combined with the current preferred [FiatRate].
@freezed
class BalanceState with _$BalanceState {
  const factory BalanceState({
    required Balance? balanceSats,
    required FiatRate? fiatRate,
  }) = _BalanceState;

  const BalanceState._();

  static BalanceState placeholder =
      const BalanceState(balanceSats: null, fiatRate: null);

  int? totalSats() => this.balanceSats?.totalSats;
  int? lightningSats() => this.balanceSats?.lightningSats;
  int? onchainSats() => this.balanceSats?.onchainSats;

  int? byKindSats(BalanceKind kind) => switch (kind) {
        BalanceKind.onchain => this.onchainSats(),
        BalanceKind.lightning => this.lightningSats(),
      };

  double? totalFiat() => this._convertFiat(this.totalSats());
  double? lightningFiat() => this._convertFiat(this.lightningSats());
  double? onchainFiat() => this._convertFiat(this.onchainSats());

  double? byKindFiat(BalanceKind kind) => switch (kind) {
        BalanceKind.onchain => this.onchainFiat(),
        BalanceKind.lightning => this.lightningFiat(),
      };

  double? _convertFiat(final int? satsBalance) =>
      (satsBalance != null && this.fiatRate != null)
          ? currency_format.satsToBtc(satsBalance) * this.fiatRate!.rate
          : null;
}
