import 'package:app_rs_dart/ffi/types.dart' show PaymentKind;

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
