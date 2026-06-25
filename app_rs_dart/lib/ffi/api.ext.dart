/// Extension methods on Dart/Rust FFI types.
library;

import 'package:app_rs_dart/ffi/api.dart' show Balance, FiatRate, FiatRates;
import 'package:app_rs_dart/ffi/types.dart' show PaymentRail;
import 'package:app_rs_dart/ffi/types.ext.dart';
import 'package:collection/collection.dart';

//
// Balance
//

extension BalanceExt on Balance {
  int maxSendableByRail(final PaymentRail rail) =>
      (rail.isLightning()) ? this.lightningMaxSendableSats : this.onchainSats;
}

//
// FiatRates
//

extension FiatRatesExt on FiatRates {
  FiatRate? findByFiat(String fiatName) =>
      this.rates.firstWhereOrNull((rate) => rate.fiat == fiatName);
}
