//! Collection of [TextInputFormatter]s

import 'dart:convert' show utf8;
import 'dart:math' show min, pow;

import 'package:flutter/services.dart'
    show
        FilteringTextInputFormatter,
        TextEditingValue,
        TextInputFormatter,
        TextSelection;
import 'package:intl/intl.dart' show NumberFormat;

import 'package:lexeapp/result.dart';

/// [AlphaNumericInputFormatter] is a [TextInputFormatter] that restricts input
/// text to alpha-numeric characters (a-z, A-Z, 0-9).
class AlphaNumericInputFormatter extends FilteringTextInputFormatter {
  AlphaNumericInputFormatter() : super.allow(RegExp(r'[a-zA-Z0-9]'));
}

/// [MaxUtf8BytesInputFormatter] is a [TextInputFormatter] that restricts the
/// size of the input to [maxBytes], _after_ the string has been encoded to
/// UTF-8.
///
/// ### Why, God?
///
/// We need restrict the length of e.g. payment notes in _bytes_, but only
/// after they're encoded to UTF-8. Flutter (sadly) chose to use UTF-16 encoded
/// strings.
class MaxUtf8BytesInputFormatter extends TextInputFormatter {
  const MaxUtf8BytesInputFormatter({required this.maxBytes})
    : assert(maxBytes >= 0);

  final int maxBytes;

  @override
  TextEditingValue formatEditUpdate(
    TextEditingValue oldValue,
    TextEditingValue newValue,
  ) {
    if (newValue.text.isEmpty) {
      return newValue;
    }

    final numUtf8Bytes = utf8.encoder.convert(newValue.text).lengthInBytes;
    if (numUtf8Bytes > this.maxBytes) {
      return oldValue;
    }

    return newValue;
  }
}

/// [DecimalInputFormatter] is a [TextInputFormatter] that:
///
/// 1. Only allows inputting digits, plus one decimal separator when
///    [maxDecimalPlaces] > 0.
/// 2. Auto-formats the text field as-you-type so locale-aware grouping
///    separators are added.
///
/// The parsed/formatted value is always an `int` representing the minor units
/// of the decimal value. With [maxDecimalPlaces] == 0 that's whole units; with
/// [maxDecimalPlaces] == 3 the `int` counts thousandths ("12.345" <-> `12345`),
/// which keeps the input exact by never routing it through a float.
///
/// ### Example
///
/// If we start with "123", then type "4", the text field will auto-format to
/// "1,234" (for en_US locale).
///
/// ### Separator behavior
///
/// When the cursor is adjacent to a separator (e.g., "12,|345"):
/// - Backspace just moves the cursor left over the separator without deleting
/// - Delete key just moves the cursor right over the separator without deleting
/// This matches standard text field behavior where separators act as single units.
class DecimalInputFormatter extends TextInputFormatter {
  DecimalInputFormatter({this.maxDecimalPlaces = 0, String? locale})
    : assert(maxDecimalPlaces >= 0),
      formatter = NumberFormat.decimalPatternDigits(
        decimalDigits: 0,
        locale: locale,
      );

  /// Max decimal places the field accepts; 0 means integer-only.
  final int maxDecimalPlaces;
  final NumberFormat formatter;

  /// The [NumberSymbols.DECIMAL_SEP] code unit, computed once (lazily) for the
  /// per-character loops in [formatEditUpdate] and [_calculateCursorPosition].
  late final int decimalSeparatorCodeUnit = this.formatter.symbols.DECIMAL_SEP
      .codeUnitAt(0);

  /// The locale's digit range, bounded by [NumberSymbols.ZERO_DIGIT] and the
  /// nine code units above it. Locales with native digits (eg fa_IR) number
  /// them contiguously, which is how [NumberFormat] localizes digits too.
  late final int zeroCodeUnit = this.formatter.symbols.ZERO_DIGIT.codeUnitAt(0);
  late final int nineCodeUnit = this.zeroCodeUnit + 9;

  /// The conversion factor between the int representation and the formatted
  /// representation of the input (eg `12345` <-> "12.345" for
  /// [maxDecimalPlaces] == 3).
  late final int scaleFactor = pow(10, this.maxDecimalPlaces) as int;

  /// A formatter for rendering and parsing decimal places:
  /// - ungrouped
  /// - when rendering, zero-padded to [NumberFormat.minimumIntegerDigits]
  ///   - by default, there is no padding; can be set per-call
  ///
  /// We use [NumberFormat] rather than [int.toString]/[int.parse] for locales
  /// that render with non-ASCII digits (eg fa_IR).
  late final NumberFormat _decimalFormatter = NumberFormat.decimalPatternDigits(
    decimalDigits: 0,
    locale: this.formatter.locale,
  )..turnOffGrouping();

  /// Parse the field [text] into an `int` carrying [numDecimalPlaces] decimal
  /// places, defaulting to [maxDecimalPlaces].
  ///
  /// For example, "12.34" parses to:
  /// - `12340` with [numDecimalPlaces] == 3
  /// - `1234` with [numDecimalPlaces] == 2
  /// - `123` with [numDecimalPlaces] == 1; the extra place is truncated
  ///
  /// Group separators are ignored.
  ///
  /// Throws an exception if the field is empty.
  Result<int, FormatException> tryParse(String text, {int? numDecimalPlaces}) =>
      Result.try_(() {
        // An empty field has no amount.
        if (text.isEmpty) throw const FormatException("empty input");

        final decimalPlaces = numDecimalPlaces ?? this.maxDecimalPlaces;
        final scaleFactor = pow(10, decimalPlaces) as int;

        // Integer mode: let NumberFormat handle grouping (and any sign or
        // decimal, which it truncates).
        if (this.maxDecimalPlaces == 0) {
          final whole = switch (this.formatter.parse(text)) {
            int i => i,
            double d => d.toInt(),
          };
          return whole * scaleFactor;
        }

        // Decimal mode: split whole and decimal, parse each as an integer.
        final parts = text.split(this.formatter.symbols.DECIMAL_SEP);
        if (parts.length > 2) {
          throw const FormatException("multiple decimal separators");
        }
        final whole = parts[0].isEmpty
            ? 0
            : this.formatter.parse(parts[0]).toInt();
        final negative =
            whole < 0 || parts[0].contains(this.formatter.symbols.MINUS_SIGN);
        if (parts.length == 2 && parts[1].length > this.maxDecimalPlaces) {
          throw const FormatException("too many decimal places");
        }
        // Drop the places the caller's representation has no room for.
        final decimalStr = (parts.length == 2)
            ? parts[1].substring(0, min(parts[1].length, decimalPlaces))
            : "";
        // Pad the decimal out to [decimalPlaces]: ".05" with 3 places is `50`,
        // not `5`.
        final decimal = decimalStr.isEmpty
            ? 0
            : this._decimalFormatter.parse(decimalStr).toInt() *
                  (pow(10, decimalPlaces - decimalStr.length) as int);
        final magnitude = (whole.abs() * scaleFactor) + decimal;
        return negative ? -magnitude : magnitude;
      });

  /// Format an `int` carrying [numDecimalPlaces] decimal places (defaulting to
  /// [maxDecimalPlaces]) into a grouped, decimal string: `12340` with 3 places
  /// formats to "12.34" (trailing decimal zeros trimmed). Inverse of
  /// [tryParse].
  ///
  /// In line with [tryParse], places the field can't render are truncated:
  /// - `1234` with 4 [numDecimalPlaces] & 3 [maxDecimalPlaces] --> 0.123
  // TODO(nicole): permissive due to precedent; should we fail when [value]
  // needs more than [maxDecimalPlaces] to render?
  String formatInt(int value, {int? numDecimalPlaces}) {
    numDecimalPlaces ??= this.maxDecimalPlaces;
    final scaleFactor = pow(10, numDecimalPlaces) as int;

    // Place the sign manually if the whole part is 0. No locale places the
    // sign after the digits, so prefixing is safe.
    final whole = value ~/ scaleFactor;
    final sign = (value < 0 && whole == 0)
        ? this.formatter.symbols.MINUS_SIGN
        : "";
    final wholeStr = "$sign${this.formatter.format(whole)}";
    var decimal = value.remainder(scaleFactor).abs();

    // Drop the places the field has no room for.
    if (numDecimalPlaces > this.maxDecimalPlaces) {
      decimal ~/= pow(10, numDecimalPlaces - this.maxDecimalPlaces) as int;
      numDecimalPlaces = this.maxDecimalPlaces;
    }
    if (decimal == 0) return wholeStr;

    // Count the number of decimal places.
    var decimalPlaces = numDecimalPlaces;
    while (decimal % 10 == 0) {
      decimal ~/= 10;
      decimalPlaces -= 1;
    }
    this._decimalFormatter.minimumIntegerDigits = decimalPlaces;
    final decimalStr = this._decimalFormatter.format(decimal);
    return "$wholeStr${this.formatter.symbols.DECIMAL_SEP}$decimalStr";
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
    // `newValue.text: "1,2345.6"`. Since [_scanAnchors] ignores decorative group
    // separators, we can `_formatCleaned(_scanAnchors(text))` to reformat the
    // input text — regrouping the whole part, keeping the decimal verbatim.
    // Unfortunately, we can't just do formatInt(tryParse(text)) because that
    // would lose in-progress input like "12." or trailing zeros.
    final scan = this._scanAnchors(newValue.text);
    if (scan.isErr) return oldValue;
    final (cleaned, decimalSepPos) = scan.unwrap();

    // Enforce the decimal-place cap.
    if (decimalSepPos != null &&
        (cleaned.length - decimalSepPos - 1 > this.maxDecimalPlaces ||
            this.maxDecimalPlaces == 0)) {
      return oldValue;
    }

    final newText = this._formatCleaned(cleaned, decimalSepPos);

    // Calculate the new cursor position
    final int newCursorPosition = this._calculateCursorPosition(
      newValue.text,
      newText,
      newValue.selection.baseOffset,
      decimalSepPos,
    );

    return TextEditingValue(
      text: newText,
      selection: TextSelection.collapsed(offset: newCursorPosition),
    );
  }

  /// Scan [text], keeping only anchors: digits & a single decimal separator.
  ///
  /// Returns the cleaned string and the decimal separator's index within it.
  /// Enforcing the [maxDecimalPlaces] digit cap is the caller's job.
  ///
  /// If multiple decimal separators are found, returns [Err].
  Result<(String, int?), FormatException> _scanAnchors(String text) {
    final buffer = StringBuffer();
    int? decimalSepPos;
    for (var i = 0; i < text.length; i++) {
      final codeUnit = text.codeUnitAt(i);
      if (codeUnit == this.decimalSeparatorCodeUnit) {
        if (decimalSepPos != null) {
          return Err(const FormatException("multiple decimal separators"));
        }
        decimalSepPos = buffer.length;
        buffer.writeCharCode(codeUnit);
      } else if (codeUnit >= this.zeroCodeUnit &&
          codeUnit <= this.nineCodeUnit) {
        buffer.writeCharCode(codeUnit);
      }
    }
    return Ok((buffer.toString(), decimalSepPos));
  }

  /// Take the output of [_scanAnchors] and format the string number.
  String _formatCleaned(String cleaned, int? decimalSepPos) {
    final wholeDigits = (decimalSepPos == null)
        ? cleaned
        : cleaned.substring(0, decimalSepPos);
    // Digits go through [NumberFormat] rather than [int.parse], which only
    // reads ASCII, so that native-digit locales survive the round trip.
    final wholeStr = this.formatter.format(
      wholeDigits.isEmpty ? 0 : this.formatter.parse(wholeDigits).toInt(),
    );
    if (decimalSepPos == null) return wholeStr;

    final decimal = cleaned.substring(decimalSepPos + 1);
    return "$wholeStr${this.formatter.symbols.DECIMAL_SEP}$decimal";
  }

  /// Calculates where the cursor should be positioned after formatting.
  ///
  /// We define "anchors" as the characters that matter for cursor positioning:
  /// - digits
  /// - the decimal separator
  ///
  /// This algorithm preserves cursor position based on the number of anchors
  /// before it, not character position. This makes it work across all edit
  /// operations (typing, backspace, delete) and locales (commas, dots, spaces).
  ///
  /// ## Why we don't need to know the operation type:
  ///
  /// In most cases, the cursor positions correctly after any edit:
  /// - "12|34"  --(type "5")-> "125|34"
  /// - "123|4"  -(backspace)-> "12|4"
  /// - "12|34"  ---(delete)--> "12|4"
  /// - "1[23]4" --(type "5")-> "15|4"
  ///
  /// Due to a no-leading-zeroes rule, the position is off if one is introduced:
  /// - "|1234" --(type "0")--> "0|1234"  -(trimmed)-> "|1234"
  /// - "0|.12" --(type "3")--> "03|.12"  -(trimmed)-> "3|.12"
  /// - "|1234" -(paste "09")-> "09|1234" -(trimmed)-> "9|1234"
  /// It suffices to offset cursor position if there is a new leading zero.
  ///
  /// We just maintain this position when adding/removing group separators.
  ///
  /// ## Algorithm:
  ///
  /// 1. Count anchors before cursor (ignoring group separators and leading 0)
  ///    Example: "1,25|34" → 3 digits before
  ///    Example: "01|2,534" → 1 digit before
  ///
  /// 2. Format the number: "012534" → "12,534"
  ///
  /// 3. Find where the same digit count occurs in formatted text
  ///    Example: 3 digits in "12,534" → "12,5|34"
  ///    Example: 1 digit in "12,534" → "1|2,534"
  ///
  /// Separators are treated as decoration that we skip when counting.
  int _calculateCursorPosition(
    String newText,
    String formattedText,
    int cursorPosition,
    int? decimalSepPos,
  ) {
    // Count anchors before cursor in the unformatted text,
    // ignoring leading zeroes.
    int anchorsBeforeCursor = 0;
    bool seenNonZero = false;
    for (int i = 0; i < cursorPosition && i < newText.length; i++) {
      if (this._isAnchor(newText.codeUnitAt(i))) {
        if (seenNonZero) {
          anchorsBeforeCursor++;
        } else if (newText.codeUnitAt(i) != this.zeroCodeUnit ||
            decimalSepPos == 1) {
          // Skip leading zeros (the formatter trims them), except a lone "0"
          // whole part before a decimal (decimalSepPos == 1), which is shown.
          seenNonZero = true;
          anchorsBeforeCursor++;
        }
      }
    }

    // If ".123", an extra leading 0 gets added after formatting.
    if (decimalSepPos == 0) {
      anchorsBeforeCursor++;
    }

    // Special case: cursor at beginning stays at beginning.
    // Without this block, the loop below never matches 0 (increments before
    // comparing) and cursor incorrectly jumps to end
    if (anchorsBeforeCursor == 0) {
      return 0;
    }

    // Find position in formatted text with same number of anchors
    int anchorCount = 0;
    for (int i = 0; i < formattedText.length; i++) {
      if (this._isAnchor(formattedText.codeUnitAt(i))) {
        anchorCount++;
        if (anchorCount == anchorsBeforeCursor) {
          return i + 1; // Position cursor after this anchor
        }
      }
    }

    // If we couldn't find the position (e.g., cursor was after all digits),
    // place cursor at the end
    return formattedText.length;
  }

  /// A digit, or (in decimal mode) the decimal separator: characters whose
  /// position the cursor tracks, unlike decorative grouping separators.
  bool _isAnchor(int codeUnit) =>
      (codeUnit >= this.zeroCodeUnit && codeUnit <= this.nineCodeUnit) ||
      (this.maxDecimalPlaces > 0 && codeUnit == this.decimalSeparatorCodeUnit);
}
