//! Collection of [TextInputFormatter]s

import 'dart:convert' show utf8;

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

/// [IntInputFormatter] is a [TextInputFormatter] that:
///
/// 1. Only allows inputting digits
/// 2. Auto-formats the text field as-you-type so locale-aware decimal
///    separators are added.
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
class IntInputFormatter extends TextInputFormatter {
  IntInputFormatter({String? locale})
    : formatter = NumberFormat.decimalPatternDigits(
        decimalDigits: 0,
        locale: locale,
      ),
      super();

  final NumberFormat formatter;

  Result<int, FormatException> tryParse(String text) => Result.try_(
    () => switch (this.formatter.parse(text)) {
      int i => i,
      double d => d.toInt(),
    },
  );

  String formatInt(int value) => this.formatter.format(value);

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

    final num numValue;
    switch (this.tryParse(newValue.text)) {
      case Ok(:final ok):
        numValue = ok;
      case Err():
        // The new value probably added some unrecognized character; just return
        // the old value.
        return oldValue;
    }

    final newText = this.formatter.format(numValue);

    // Calculate the new cursor position
    final int newCursorPosition = _calculateCursorPosition(
      newValue.text,
      newText,
      newValue.selection.baseOffset,
    );

    return TextEditingValue(
      text: newText,
      selection: TextSelection.collapsed(offset: newCursorPosition),
    );
  }

  /// Calculates where the cursor should be positioned after formatting.
  ///
  /// This algorithm preserves cursor position based on the number of digits
  /// before it, not character position. This makes it work across all edit
  /// operations (typing, backspace, delete) and locales (commas, dots, spaces).
  ///
  /// ## Why we don't need to know the operation type:
  ///
  /// Flutter already positions the cursor correctly after any edit:
  /// - Type "5" at position 2 in "1234" → "12534" with cursor at position 3
  /// - Backspace at position 3 in "1234" → "124" with cursor at position 2
  /// - Right delete at position 2 in "1234" → "124" with cursor at position 2
  /// - Select "23" and type "5" in "1234" → "154" with cursor at position 2
  ///
  /// We just maintain this position when adding/removing separators.
  ///
  /// ## Algorithm:
  ///
  /// 1. Count digits before cursor (ignoring separators)
  ///    Example: cursor at position 4 in "1,2534" → 3 digits before
  ///
  /// 2. Format the number: "12534" → "12,534"
  ///
  /// 3. Find where the same digit count occurs in formatted text
  ///    Example: 3 digits in "12,534" → position 4
  ///
  /// Separators are treated as decoration that we skip when counting.
  int _calculateCursorPosition(
    String newText,
    String formattedText,
    int cursorPosition,
  ) {
    // Count digits before cursor in the unformatted text
    int digitsBeforeCursor = 0;
    for (int i = 0; i < cursorPosition && i < newText.length; i++) {
      if (_isDigit(newText.codeUnitAt(i))) {
        digitsBeforeCursor++;
      }
    }

    // Special case: cursor at beginning stays at beginning.
    // Without this block, the loop below never matches 0 (increments before
    // comparing) and cursor incorrectly jumps to end
    if (digitsBeforeCursor == 0) {
      return 0;
    }

    // Find position in formatted text with same number of digits
    int digitCount = 0;
    for (int i = 0; i < formattedText.length; i++) {
      if (_isDigit(formattedText.codeUnitAt(i))) {
        digitCount++;
        if (digitCount == digitsBeforeCursor) {
          return i + 1; // Position cursor after this digit
        }
      }
    }

    // If we couldn't find the position (e.g., cursor was after all digits),
    // place cursor at the end
    return formattedText.length;
  }

  bool _isDigit(int codeUnit) {
    return codeUnit >= 0x30 && codeUnit <= 0x39; // ASCII '0' to '9'
  }
}
