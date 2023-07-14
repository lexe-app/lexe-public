// Currency formatting utilities

import 'package:flutter/services.dart'
    show TextEditingValue, TextInputFormatter, TextSelection;
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

/// `IntInputFormatter` is a [TextInputFormatter] that:
///
/// 1. Only allows inputting digits
/// 2. Auto-formats the text field as-you-type so locale-aware decimal
///    separators are added.
///
/// ### Example
///
/// If we start with "123", then type "4", the text field will auto-format to
/// "1,234" (for en_US locale).
class IntInputFormatter extends TextInputFormatter {
  IntInputFormatter({String? locale})
      : formatter = NumberFormat.decimalPatternDigits(
          decimalDigits: 0,
          locale: locale,
        ),
        super();

  final NumberFormat formatter;

  int? tryParse(String text) {
    try {
      switch (this.formatter.parse(text)) {
        case int i:
          return i;
        case double d:
          return d.toInt();
      }
    } on FormatException {
      return null;
    }
  }

  @override
  TextEditingValue formatEditUpdate(
    TextEditingValue oldValue,
    TextEditingValue newValue,
  ) {
    if (newValue.text.isEmpty) {
      return newValue;
    }

    // As the user is typing, we'll get something like
    // `newValue.text: "1,2345"`. Fortunately, `parse` just kinda ignores all
    // decimal separators (?) so we can just `format(parse(text))` to
    // "properly" format the input text.
    final maybeNumValue = this.tryParse(newValue.text);
    if (maybeNumValue == null) {
      // The new value probably added some unrecognized character; just return
      // the old value.
      return oldValue;
    }
    final num numValue = maybeNumValue;

    final newText = this.formatter.format(numValue);

    return TextEditingValue(
      text: newText,
      // TODO(phlip9): This will always force the input cursor to the end of the
      // text field. In theory, we could be smarter and correctly compute the
      // updated cursor location when the user is editing the middle of the
      // text. Black-box decimal formatting makes this hard though.
      selection: TextSelection.collapsed(offset: newText.length),
    );
  }
}
