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
