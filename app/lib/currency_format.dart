// Currency formatting utilities

import 'package:app_rs_dart/ffi/types.dart' show PaymentDirection;
import 'package:intl/intl.dart' show NumberFormat;

const int satsPerBtc = 100000000; // 1e8, 100 million sats

double satsToBtc(int sats) => sats * 1e-8;

String directionToSign(PaymentDirection direction) => switch (direction) {
  PaymentDirection.inbound => "+",
  PaymentDirection.outbound => "-",
  PaymentDirection.info => "",
};

(int, int) divRem(int numerator, int denominator) =>
    (numerator ~/ denominator, numerator.remainder(denominator));

/// Format an amount in satoshis as a decimal amount in BTC, without losing
/// precision, and optionally padding the decimal portion with zeroes.
///
/// We use this fn and not `int.toStringAsPrecision(8)` since we absolutely
/// cannot lose precision to float conversion. Esp. for cases where we're not
/// just displaying some already approximate fiat converted value.
///
/// This formatting function doesn't take a locale since it's used to format
/// amounts for BIP21 URIs.
///
/// Unpadded:
///             0 =>  "0.0"
///             1 =>  "0.00000001"
///         2,500 =>  "0.000025"
///   100,000,000 =>  "1.0"
/// 2,300,000,000 => "23.0"
/// 2,300,056,700 => "23.000567"
///
/// Padded:
///             0 =>  "0.00000000"
///             1 =>  "0.00000001"
///         2,500 =>  "0.00002500"
///   100,000,000 =>  "1.00000000"
/// 2,300,000,000 => "23.00000000"
/// 2,300,056,700 => "23.00056700"
String formatSatsToBtcForUri(int sats, {bool padded = false}) {
  final (wholeBtc, satsFrac) = divRem(sats.abs(), satsPerBtc);

  final wholeBtcStr = wholeBtc.toString();
  final satsFracUnpadded = satsFrac.toString();
  final satsFracPadded = satsFracUnpadded.padLeft(8, "0");

  if (padded) {
    return "$wholeBtcStr.$satsFracPadded";
  }

  // Need to trim off any trailing zeroes from the decimal portion.
  //
  // ex: "00123000" => "00123"
  // ex: "10000000" => "1"
  // ex: "00000001" => "00000001"
  // ex: "00000000" => "0"

  // Find the index of the rightmost, non-zero digit, or null if none.
  int? rightIdxNonZero;
  for (var idx = satsFracPadded.length - 1; idx >= 0; idx--) {
    if (satsFracPadded[idx] != "0") {
      rightIdxNonZero = idx;
      break;
    }
  }

  final String satsFracStr;
  if (rightIdxNonZero == null) {
    satsFracStr = "0";
  } else {
    // cut off all the trailing zeroes.
    satsFracStr = satsFracPadded.substring(0, rightIdxNonZero + 1);
  }

  return "$wholeBtcStr.$satsFracStr";
}

/// Format a bitcoin amount in satoshis.
///
/// * Specify the sign ('+' vs '-') with the `direction`.
/// * Include the "₿" symbol with `bitcoinSymbol: true`, positioned according
///   to locale conventions (prefix for en_US, suffix for fr_FR, etc.).
///
/// ### Examples
///
/// ```dart
/// assert("₿73,000" == formatSatsAmount(73000, locale: "en_US"));
/// assert("73 000 ₿" == formatSatsAmount(73000, locale: "fr_FR"));
/// assert("+₿73,000" == formatSatsAmount(73000, direction: PaymentDirection.inbound, locale: "en_US"));
/// assert("-₿73,000" == formatSatsAmount(73000, direction: PaymentDirection.outbound, locale: "en_US"));
/// assert("+73 000 ₿" == formatSatsAmount(73000, direction: PaymentDirection.inbound, locale: "fr_FR"));
/// assert("-73 000 ₿" == formatSatsAmount(73000, direction: PaymentDirection.outbound, locale: "fr_FR"));
/// assert("73,000" == formatSatsAmount(73000, bitcoinSymbol: false, locale: "en_US"));
/// assert("73 000" == formatSatsAmount(73000, bitcoinSymbol: false, locale: "fr_FR"));
/// ```
String formatSatsAmount(
  int amountSats, {
  PaymentDirection? direction,
  bool bitcoinSymbol = true,
  String? locale,
}) {
  final sign = (direction != null) ? directionToSign(direction) : "";

  if (bitcoinSymbol) {
    // Use currency formatting to position the ₿ symbol correctly for the locale
    // `symbol` directly sets what symbol to use in place of the currency
    final NumberFormat currencyFormatter = NumberFormat.currency(
      locale: locale,
      symbol: "₿",
      decimalDigits: 0,
    );
    final amountStr = currencyFormatter.format(amountSats);
    return "$sign$amountStr";
  } else {
    // Just format the number without any currency symbol
    final NumberFormat formatter = NumberFormat.decimalPatternDigits(
      decimalDigits: 0,
      locale: locale,
    );
    final amountStr = formatter.format(amountSats);
    return "$sign$amountStr";
  }
}

/// Format a fiat currency amount.
String formatFiat(double amountFiat, String fiatName, {String? locale}) {
  final NumberFormat currencyFormatter = NumberFormat.simpleCurrency(
    name: fiatName,
    locale: locale,
  );
  return currencyFormatter.format(amountFiat);
}

/// Format a fiat currency amount, but return the whole and fractional values
/// separately.
///
/// ### Examples
///
/// ```dart
/// assert(("$123", ".46") == formatFiatParts(123.4567, "USD"));
/// ```
(String, String) formatFiatParts(
  double amountFiat,
  String fiatName, {
  String? locale,
}) {
  final NumberFormat currencyFormatter = NumberFormat.simpleCurrency(
    name: fiatName,
    locale: locale,
  );
  final amountStr = currencyFormatter.format(amountFiat);

  final decimalSeparator = currencyFormatter.symbols.DECIMAL_SEP;
  final maybeDecimalIdx = amountStr.lastIndexOf(decimalSeparator);

  // ex: amountFiat = 123.45679
  //     amountStrWhole = "$123"
  //     amountStrFract= ".46"
  final String amountStrWhole;
  final String amountStrFract;

  if (maybeDecimalIdx >= 0) {
    amountStrWhole = amountStr.substring(0, maybeDecimalIdx);
    amountStrFract = amountStr.substring(maybeDecimalIdx);
  } else {
    amountStrWhole = amountStr;
    amountStrFract = "";
  }

  return (amountStrWhole, amountStrFract);
}
