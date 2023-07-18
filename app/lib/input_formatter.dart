import 'package:flutter/services.dart'
    show TextEditingValue, TextInputFormatter, TextSelection;
import 'package:intl/intl.dart' show NumberFormat;

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
