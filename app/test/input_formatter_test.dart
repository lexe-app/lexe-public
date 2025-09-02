import 'package:flutter/services.dart' show TextEditingValue, TextSelection;
import 'package:flutter_test/flutter_test.dart';
import 'package:lexeapp/input_formatter.dart' show IntInputFormatter;

void main() {
  group('IntInputFormatter', () {
    late IntInputFormatter formatter;

    setUp(() {
      // Use en_US locale for consistent testing with comma separators
      formatter = IntInputFormatter(locale: 'en_US');
    });

    group('cursor position preservation', () {
      test('cursor at beginning stays at beginning', () {
        // Start with "1,234", cursor at position 0
        const oldValue = TextEditingValue(
          text: '1,234',
          selection: TextSelection.collapsed(offset: 0),
        );

        // Type "5" at the beginning
        const newValue = TextEditingValue(
          text: '51,234',
          selection: TextSelection.collapsed(offset: 1),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "51,234" with cursor at position 1 (after the "5")
        expect(result.text, '51,234');
        expect(result.selection.baseOffset, 1);
      });

      test('typing character in middle preserves cursor position', () {
        // Start with "1,234", cursor after "2" (position 3)
        const oldValue = TextEditingValue(
          text: '1,234',
          selection: TextSelection.collapsed(offset: 3),
        );

        // Type "5" after the "2"
        const newValue = TextEditingValue(
          text: '1,2534',
          selection: TextSelection.collapsed(offset: 4),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "12,534" with cursor after "5" (position 4)
        expect(result.text, '12,534');
        expect(result.selection.baseOffset, 4);
      });

      test('cursor at end stays at end', () {
        // Start with "1,234", cursor at end
        const oldValue = TextEditingValue(
          text: '1,234',
          selection: TextSelection.collapsed(offset: 5),
        );

        // Type "5" at the end
        const newValue = TextEditingValue(
          text: '1,2345',
          selection: TextSelection.collapsed(offset: 6),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "12,345" with cursor at end
        expect(result.text, '12,345');
        expect(result.selection.baseOffset, 6);
      });

      test('backspace (left delete) in middle preserves position', () {
        // Start with "12,345", cursor after "3" (position 4)
        const oldValue = TextEditingValue(
          text: '12,345',
          selection: TextSelection.collapsed(offset: 4),
        );

        // Backspace deletes the "3" - cursor moves from position 4 to 3
        const newValue = TextEditingValue(
          text: '12,45',
          selection: TextSelection.collapsed(offset: 3),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "1,245" with cursor after "2" (position 3 including comma)
        // We had 2 digits before cursor in "12,45", so in "1,245" that's after the "2"
        expect(result.text, '1,245');
        expect(result.selection.baseOffset, 3); // After "1,2"
      });

      test('forward delete (Delete key) preserves position', () {
        // Start with "1,234", cursor after "1" (position 1)
        const oldValue = TextEditingValue(
          text: '1,234',
          selection: TextSelection.collapsed(offset: 1),
        );

        // Forward delete from position 1 would skip comma, then delete "2"
        // Result after deleting "2": "134"
        const newValue = TextEditingValue(
          text: '134',
          selection: TextSelection.collapsed(offset: 1),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "134" with cursor still after "1"
        expect(result.text, '134');
        expect(result.selection.baseOffset, 1); // Still after "1"
      });

      test('forward delete at beginning of number', () {
        // Start with "1,234", cursor at beginning (position 0)
        const oldValue = TextEditingValue(
          text: '1,234',
          selection: TextSelection.collapsed(offset: 0),
        );

        // Forward delete removes the "1" leaving ",234" - cursor stays at position 0
        const newValue = TextEditingValue(
          text: ',234',
          selection: TextSelection.collapsed(offset: 0),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "234" with cursor at beginning
        expect(result.text, '234');
        expect(result.selection.baseOffset, 0);
      });

      test('selection replacement preserves cursor position', () {
        // Start with "1,234", select from position 1 to 4 (selecting ",23")
        const oldValue = TextEditingValue(
          text: '1,234',
          selection: TextSelection(baseOffset: 1, extentOffset: 4),
        );

        // Replace selection with "5" - cursor ends up at position 2
        const newValue = TextEditingValue(
          text: '154',
          selection: TextSelection.collapsed(offset: 2),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "154" with cursor after "5"
        expect(result.text, '154');
        expect(result.selection.baseOffset, 2);
      });

      test('selection replacement across separator', () {
        // Start with "12,345", select from position 1 to 5 (selecting "2,34")
        const oldValue = TextEditingValue(
          text: '12,345',
          selection: TextSelection(baseOffset: 1, extentOffset: 5),
        );

        // Replace selection with "99" - cursor ends up at position 3
        const newValue = TextEditingValue(
          text: '1995',
          selection: TextSelection.collapsed(offset: 3),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "1,995" with cursor after second "9"
        expect(result.text, '1,995');
        expect(result.selection.baseOffset, 4); // After "1,99"
      });

      test('forward delete in larger number', () {
        // Start with "123,456", cursor after "3" (position 3)
        const oldValue = TextEditingValue(
          text: '123,456',
          selection: TextSelection.collapsed(offset: 3),
        );

        // Forward delete from position 3 would skip comma, then delete "4"
        // Result after deleting "4": "12356"
        const newValue = TextEditingValue(
          text: '12356',
          selection: TextSelection.collapsed(offset: 3),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "12,356" with cursor still after "3"
        // We had 3 digits before cursor, so position is 4 (after "12,3")
        expect(result.text, '12,356');
        expect(result.selection.baseOffset, 4); // After "12,3"
      });

      test('adding digit that causes separator shift', () {
        // Start with "999", cursor at end
        const oldValue = TextEditingValue(
          text: '999',
          selection: TextSelection.collapsed(offset: 3),
        );

        // Type "9" at the end
        const newValue = TextEditingValue(
          text: '9999',
          selection: TextSelection.collapsed(offset: 4),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "9,999" with cursor at end
        expect(result.text, '9,999');
        expect(result.selection.baseOffset, 5);
      });

      test('removing digit that removes separator', () {
        // Start with "1,000", cursor after the "1" (position 1)
        const oldValue = TextEditingValue(
          text: '1,000',
          selection: TextSelection.collapsed(offset: 1),
        );

        // Backspace deletes the "1" - cursor moves from position 1 to 0
        const newValue = TextEditingValue(
          text: ',000',
          selection: TextSelection.collapsed(offset: 0),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "0" with cursor at beginning
        expect(result.text, '0');
        expect(result.selection.baseOffset, 0);
      });

      test('typing in empty field', () {
        // Start with empty field
        const oldValue = TextEditingValue(
          text: '',
          selection: TextSelection.collapsed(offset: 0),
        );

        // Type "5"
        const newValue = TextEditingValue(
          text: '5',
          selection: TextSelection.collapsed(offset: 1),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should show "5" with cursor at end
        expect(result.text, '5');
        expect(result.selection.baseOffset, 1);
      });

      test('clearing all text returns empty', () {
        // Start with "1,234"
        const oldValue = TextEditingValue(
          text: '1,234',
          selection: TextSelection.collapsed(offset: 5),
        );

        // Clear all text
        const newValue = TextEditingValue(
          text: '',
          selection: TextSelection.collapsed(offset: 0),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should be empty
        expect(result.text, '');
        expect(result.selection.baseOffset, 0);
      });

      test('large number formatting', () {
        // Start with "999,999", cursor at position 3 (after first "9")
        const oldValue = TextEditingValue(
          text: '999,999',
          selection: TextSelection.collapsed(offset: 3),
        );

        // Type "9" at position 3 (before the comma)
        const newValue = TextEditingValue(
          text: '9999,999',
          selection: TextSelection.collapsed(offset: 4),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "9,999,999" with cursor after 4th digit
        expect(result.text, '9,999,999');
        // After 4 digits, accounting for 1 separator: position 5
        expect(result.selection.baseOffset, 5);
      });

      test('pasting formatted number preserves cursor', () {
        // Start with empty
        const oldValue = TextEditingValue(
          text: '',
          selection: TextSelection.collapsed(offset: 0),
        );

        // Paste "12,345" (already formatted)
        const newValue = TextEditingValue(
          text: '12,345',
          selection: TextSelection.collapsed(offset: 6),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should reformat correctly and place cursor at end
        expect(result.text, '12,345');
        expect(result.selection.baseOffset, 6);
      });

      test('invalid input is rejected', () {
        // Start with "123"
        const oldValue = TextEditingValue(
          text: '123',
          selection: TextSelection.collapsed(offset: 3),
        );

        // Try to type "a"
        const newValue = TextEditingValue(
          text: '123a',
          selection: TextSelection.collapsed(offset: 4),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should reject and return old value
        expect(result.text, '123');
        expect(result.selection.baseOffset, 3);
      });

      test('cursor position beyond text length', () {
        // Start with "123"
        const oldValue = TextEditingValue(
          text: '123',
          selection: TextSelection.collapsed(offset: 3),
        );

        // Type "4" but with invalid cursor position
        const newValue = TextEditingValue(
          text: '1234',
          selection: TextSelection.collapsed(offset: 10), // Beyond text length
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "1,234" and place cursor at end
        expect(result.text, '1,234');
        expect(result.selection.baseOffset, 5);
      });
    });

    group('number parsing and formatting', () {
      test('formats integers correctly', () {
        expect(formatter.formatInt(0), '0');
        expect(formatter.formatInt(1), '1');
        expect(formatter.formatInt(12), '12');
        expect(formatter.formatInt(123), '123');
        expect(formatter.formatInt(1234), '1,234');
        expect(formatter.formatInt(12345), '12,345');
        expect(formatter.formatInt(123456), '123,456');
        expect(formatter.formatInt(1234567), '1,234,567');
      });

      test('parses formatted strings correctly', () {
        expect(formatter.tryParse('0').ok, 0);
        expect(formatter.tryParse('1').ok, 1);
        expect(formatter.tryParse('123').ok, 123);
        expect(formatter.tryParse('1,234').ok, 1234);
        expect(formatter.tryParse('12,345').ok, 12345);
        expect(formatter.tryParse('1,234,567').ok, 1234567);
      });

      test('handles malformed input gracefully', () {
        expect(formatter.tryParse('abc').isErr, true);
        // Note: NumberFormat.parse accepts decimals and converts to int
        expect(formatter.tryParse('12.34').ok, 12);
        // Note: NumberFormat.parse accepts negative numbers
        expect(formatter.tryParse('-123').ok, -123);
      });
    });

    group('locale-specific formatting', () {
      test('German locale (dot separator)', () {
        final formatter = IntInputFormatter(locale: 'de_DE');

        // Format with dots as thousand separators
        expect(formatter.formatInt(1234), '1.234');
        expect(formatter.formatInt(1234567), '1.234.567');

        // Cursor position with German formatting
        const oldValue = TextEditingValue(
          text: '1.234',
          selection: TextSelection.collapsed(offset: 3),
        );

        // Type "5" after the "2"
        const newValue = TextEditingValue(
          text: '1.2534',
          selection: TextSelection.collapsed(offset: 4),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format to "12.534" with cursor after "5"
        expect(result.text, '12.534');
        expect(result.selection.baseOffset, 4);
      });

      test('French locale (space separator)', () {
        final formatter = IntInputFormatter(locale: 'fr_FR');

        // Note: French locale uses non-breaking space (U+00A0)
        // We'll check that it contains the expected digits and has separators
        final formatted1234 = formatter.formatInt(1234);
        final formatted1234567 = formatter.formatInt(1234567);

        // Extract just the digits to verify the number is correct
        expect(formatted1234.replaceAll(RegExp(r'[^0-9]'), ''), '1234');
        expect(formatted1234567.replaceAll(RegExp(r'[^0-9]'), ''), '1234567');

        // Verify separators are present (length is greater than digit count)
        expect(formatted1234.length, greaterThan(4));
        expect(formatted1234567.length, greaterThan(7));

        // Test cursor positioning works with space separators
        final oldValue = TextEditingValue(
          text: formatted1234,
          selection: const TextSelection.collapsed(offset: 3),
        );

        // Type "5" - the exact format depends on the locale
        const newValue = TextEditingValue(
          text: '12534',
          selection: TextSelection.collapsed(offset: 3),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should format with space and preserve cursor position logically
        expect(result.text.replaceAll(RegExp(r'[^0-9]'), ''), '12534');
        // Cursor should be after the 3rd digit
        expect(result.selection.baseOffset, greaterThanOrEqualTo(3));
      });

      test('Indian locale (lakh/crore grouping)', () {
        final formatter = IntInputFormatter(locale: 'en_IN');

        // Indian numbering system: 1,00,000 (lakh) and 1,00,00,000 (crore)
        // After the first 3 digits from right, groups are in 2s
        expect(formatter.formatInt(1234), '1,234');
        expect(formatter.formatInt(12345), '12,345');
        expect(formatter.formatInt(123456), '1,23,456');
        expect(formatter.formatInt(1234567), '12,34,567');
        expect(formatter.formatInt(12345678), '1,23,45,678');

        // Test cursor positioning with Indian grouping
        const oldValue = TextEditingValue(
          text: '12,34,567',
          selection: TextSelection.collapsed(offset: 5),
        );

        // Type "8" after the "4"
        const newValue = TextEditingValue(
          text: '12,348,567',
          selection: TextSelection.collapsed(offset: 6),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Should reformat with Indian grouping
        expect(result.text, '1,23,48,567');
        // Cursor should be after the 5th digit (accounting for separators)
        expect(result.selection.baseOffset, greaterThanOrEqualTo(5));
      });

      test('Arabic locale number formatting', () {
        final formatter = IntInputFormatter(locale: 'ar_SA');

        // Arabic uses different separator patterns
        final formatted = formatter.formatInt(1234567);

        // Verify the number is correct regardless of formatting
        expect(formatted.replaceAll(RegExp(r'[^0-9]'), ''), '1234567');

        // Test that cursor positioning still works with Arabic number formatting
        // Note: Numbers are still entered left-to-right even in RTL locales
        // Start with "123", cursor after "12" (position 2)
        const oldValue = TextEditingValue(
          text: '123',
          selection: TextSelection.collapsed(offset: 2),
        );

        // Type "5" after "12" → "1253" with cursor at position 3
        const newValue = TextEditingValue(
          text: '1253',
          selection: TextSelection.collapsed(offset: 3),
        );

        final result = formatter.formatEditUpdate(oldValue, newValue);

        // Verify the number is correct
        expect(result.text.replaceAll(RegExp(r'[^0-9]'), ''), '1253');
        // Cursor should be positioned after 3rd digit
        expect(result.selection.baseOffset, greaterThanOrEqualTo(3));
      });
    });
  });
}
