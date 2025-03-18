/// Extension methods on Dart/Rust FFI types.
library;

import 'package:app_rs_dart/ffi/api.dart' show Balance, FiatRate, FiatRates;
import 'package:app_rs_dart/ffi/types.dart' show PaymentKind;
import 'package:collection/collection.dart';

//
// Balance
//

extension BalanceExt on Balance {
  int balanceByKind(final PaymentKind kind) => switch (kind) {
        PaymentKind.onchain => this.onchainSats,
        PaymentKind.invoice => this.lightningSats,
        PaymentKind.spontaneous => this.lightningSats,
      };

  int balanceMaxSendableByKind(final PaymentKind kind) => switch (kind) {
        PaymentKind.onchain => this.onchainSats,
        PaymentKind.invoice => this.lightningMaxSendableSats,
        PaymentKind.spontaneous => this.lightningMaxSendableSats,
      };
}

//
// FiatRates
//

extension FiatRatesExt on FiatRates {
  FiatRate? findByFiat(String fiatName) =>
      this.rates.firstWhereOrNull((rate) => rate.fiat == fiatName);
}
