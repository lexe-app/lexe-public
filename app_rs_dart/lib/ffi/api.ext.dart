/// Extension methods on Dart/Rust FFI types.
library;

import 'package:app_rs_dart/ffi/api.dart' show Balance;
import 'package:app_rs_dart/ffi/types.dart' show PaymentKind;

//
// Balance
//

extension BalanceExt on Balance {
  int balanceByKind(final PaymentKind kind) => switch (kind) {
        PaymentKind.onchain => this.onchainSats,
        PaymentKind.invoice => this.lightningSats,
        PaymentKind.spontaneous => this.lightningSats,
      };
}
