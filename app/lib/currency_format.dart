// Currency formatting utilities

import 'package:intl/intl.dart' show NumberFormat;

import 'bindings_generated_api.dart' show PaymentDirection;

double satsToBtc(int sats) => sats * 1e-8;

String directionToSign(PaymentDirection direction) =>
    (direction == PaymentDirection.Inbound) ? "+" : "-";

/// Format a bitcoin amount in satoshis.
///
/// * Specify the sign ('+' vs '-') with the `direction`.
/// * Include the " sats" suffix with `satsSuffix: true`.
///
/// ### Examples
///
/// ```dart
/// assert("73,000 sats" == formatSatsAmount(73000));
/// assert("+73,000 sats" == formatSatsAmount(73000, direction: PaymentDirection.inbound));
/// assert("-73,000 sats" == formatSatsAmount(73000, direction: PaymentDirection.outbound));
/// assert("73,000" == formatSatsAmount(73000, satsSuffix: false));
/// ```
String formatSatsAmount(
  int amountSats, {
  PaymentDirection? direction,
  bool satsSuffix = true,
  String? locale,
}) {
  final NumberFormat formatter = NumberFormat.decimalPatternDigits(
    decimalDigits: 0,
    locale: locale,
  );

  final sign = (direction != null) ? directionToSign(direction) : "";

  final suffix = (satsSuffix) ? " sats" : "";

  final amountStr = formatter.format(amountSats);

  return "$sign$amountStr$suffix";
}

/// Format a fiat currency amount.
String formatFiat(
  double amountFiat,
  String fiatName, {
  String? locale,
}) {
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
