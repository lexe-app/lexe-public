import 'package:flutter/services.dart' show TextEditingValue, TextSelection;
import 'package:flutter_test/flutter_test.dart';
import 'package:lexeapp/input_formatter.dart' show DecimalInputFormatter;

void main() {
  group('DecimalInputFormatter', () {
    group('integer formatter (maxDecimalPlaces: 0)', () {
      late DecimalInputFormatter formatter;

      setUp(() {
        // Use en_US locale for consistent testing with comma separators
        formatter = DecimalInputFormatter(locale: 'en_US');
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
            selection: TextSelection.collapsed(
              offset: 10,
            ), // Beyond text length
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
          final formatter = DecimalInputFormatter(locale: 'de_DE');

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
          final formatter = DecimalInputFormatter(locale: 'fr_FR');

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
          final formatter = DecimalInputFormatter(locale: 'en_IN');

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
          final formatter = DecimalInputFormatter(locale: 'ar_SA');

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

      group('separator handling', () {
        test('rejects a typed decimal separator', () {
          const oldValue = TextEditingValue(
            text: '12',
            selection: TextSelection.collapsed(offset: 2),
          );
          const newValue = TextEditingValue(
            text: '12.',
            selection: TextSelection.collapsed(offset: 3),
          );
          // Rejected -> keeps the old value.
          final result = formatter.formatEditUpdate(oldValue, newValue);
          expect(result.text, '12');
          expect(result.selection.baseOffset, 2);
        });

        test('rejects a pasted decimal amount, rather than scaling it', () {
          const oldValue = TextEditingValue(
            text: '',
            selection: TextSelection.collapsed(offset: 0),
          );
          const newValue = TextEditingValue(
            text: '1.5',
            selection: TextSelection.collapsed(offset: 3),
          );
          expect(formatter.formatEditUpdate(oldValue, newValue).text, '');
        });

        test('still drops group separators', () {
          const oldValue = TextEditingValue(
            text: '1,234',
            selection: TextSelection.collapsed(offset: 5),
          );
          const newValue = TextEditingValue(
            text: '1,2345',
            selection: TextSelection.collapsed(offset: 6),
          );
          expect(formatter.formatEditUpdate(oldValue, newValue).text, '12,345');
        });

        // The separator check keys off the locale's decimal separator, so verify
        // de_DE (where "." groups and "," is the decimal) treats them oppositely.
        test('de_DE groups "." but rejects the "," decimal separator', () {
          final formatter = DecimalInputFormatter(locale: 'de_DE');

          expect(
            formatter
                .formatEditUpdate(
                  const TextEditingValue(text: '1.234'),
                  const TextEditingValue(text: '1.2345'),
                )
                .text,
            '12.345',
          );
          expect(
            formatter
                .formatEditUpdate(
                  const TextEditingValue(text: '12'),
                  const TextEditingValue(text: '12,'),
                )
                .text,
            '12',
          );
        });
      });

      // numDecimalPlaces pins the caller's representation, independent of this
      // field's decimal-place cap.
      group('numDecimalPlaces', () {
        test('tryParse scales the output to numDecimalPlaces', () {
          final formatter = DecimalInputFormatter(locale: 'en_US');

          expect(formatter.tryParse('5', numDecimalPlaces: 3).ok, 5000);
          // A too-precise input truncates, as NumberFormat.parse always has.
          expect(formatter.tryParse('1.5', numDecimalPlaces: 3).ok, 1000);
        });

        test('formatInt truncates what the field cannot render', () {
          final formatter = DecimalInputFormatter(locale: 'en_US');

          expect(formatter.formatInt(5000, numDecimalPlaces: 3), '5');
          // A 0-place field has nowhere to put the remainder.
          expect(formatter.formatInt(5500, numDecimalPlaces: 3), '5');
        });

        test('round-trips through an independent representation', () {
          final formatter = DecimalInputFormatter(locale: 'en_US');

          for (final value in [0, 1000, 5000, 123456000]) {
            final text = formatter.formatInt(value, numDecimalPlaces: 3);
            expect(
              formatter.tryParse(text, numDecimalPlaces: 3).ok,
              value,
              reason: '$value',
            );
          }
        });
      });
    });

    group('decimal formatter (maxDecimalPlaces: 3)', () {
      late DecimalInputFormatter formatter;

      setUp(() {
        formatter = DecimalInputFormatter(maxDecimalPlaces: 3, locale: 'en_US');
      });

      test('as-you-type preserves an in-progress trailing separator', () {
        // Typing "." after "12" keeps "12." so the user can keep going, and
        // the cursor lands after the separator.
        const oldValue = TextEditingValue(
          text: '12',
          selection: TextSelection.collapsed(offset: 2),
        );
        const newValue = TextEditingValue(
          text: '12.',
          selection: TextSelection.collapsed(offset: 3),
        );
        final result = formatter.formatEditUpdate(oldValue, newValue);
        expect(result.text, '12.');
        expect(result.selection.baseOffset, 3);
      });

      test('as-you-type groups the whole part but keeps the decimal', () {
        const oldValue = TextEditingValue(
          text: '100.5',
          selection: TextSelection.collapsed(offset: 5),
        );
        const newValue = TextEditingValue(
          text: '1000.5',
          selection: TextSelection.collapsed(offset: 4),
        );
        expect(formatter.formatEditUpdate(oldValue, newValue).text, '1,000.5');
      });

      test('as-you-type rejects a fourth decimal place', () {
        const oldValue = TextEditingValue(
          text: '12.345',
          selection: TextSelection.collapsed(offset: 6),
        );
        const newValue = TextEditingValue(
          text: '12.3456',
          selection: TextSelection.collapsed(offset: 7),
        );
        // Rejected -> keeps the old value.
        expect(formatter.formatEditUpdate(oldValue, newValue).text, '12.345');
      });

      test('as-you-type rejects a second decimal separator', () {
        const oldValue = TextEditingValue(
          text: '12.3',
          selection: TextSelection.collapsed(offset: 4),
        );
        const newValue = TextEditingValue(
          text: '12.3.',
          selection: TextSelection.collapsed(offset: 5),
        );
        expect(formatter.formatEditUpdate(oldValue, newValue).text, '12.3');
      });

      test('typing "." after a lone "0" keeps the shown 0 (issue 5)', () {
        const oldValue = TextEditingValue(
          text: '0',
          selection: TextSelection.collapsed(offset: 1),
        );
        const newValue = TextEditingValue(
          text: '0.',
          selection: TextSelection.collapsed(offset: 2),
        );
        final result = formatter.formatEditUpdate(oldValue, newValue);
        expect(result.text, '0.');
        // Cursor lands after the separator, not reset before the "0".
        expect(result.selection.baseOffset, 2);
      });

      test('builds a decimal after the "0." separator', () {
        const oldValue = TextEditingValue(
          text: '0.',
          selection: TextSelection.collapsed(offset: 2),
        );
        const newValue = TextEditingValue(
          text: '0.5',
          selection: TextSelection.collapsed(offset: 3),
        );
        expect(formatter.formatEditUpdate(oldValue, newValue).text, '0.5');
      });

      test('trims a leading zero typed before a non-zero whole part', () {
        // "0.12", insert "3" after the "0" -> "03.12" -> "3.12" (cursor "3|").
        const oldValue = TextEditingValue(
          text: '0.12',
          selection: TextSelection.collapsed(offset: 1),
        );
        const newValue = TextEditingValue(
          text: '03.12',
          selection: TextSelection.collapsed(offset: 2),
        );
        final result = formatter.formatEditUpdate(oldValue, newValue);
        expect(result.text, '3.12');
        expect(result.selection.baseOffset, 1);
      });

      test('tryParse scales the decimal into the smallest unit', () {
        expect(formatter.tryParse('0').ok, 0);
        expect(formatter.tryParse('12').ok, 12000);
        expect(formatter.tryParse('1.5').ok, 1500);
        expect(formatter.tryParse('1.05').ok, 1050);
        expect(formatter.tryParse('12.345').ok, 12345);
        expect(formatter.tryParse('0.001').ok, 1);
        expect(formatter.tryParse('1,000.5').ok, 1000500);
      });

      test('tryParse rejects too many decimal places', () {
        expect(formatter.tryParse('1.2345').isErr, true);
      });

      // numDecimalPlaces pins the caller's representation, independent of
      // maxDecimalPlaces: the field's cap and the int's representation are
      // separate.
      group('numDecimalPlaces', () {
        test('tryParse scales the output to numDecimalPlaces', () {
          final formatter = DecimalInputFormatter(
            maxDecimalPlaces: 3,
            locale: 'en_US',
          );

          expect(formatter.tryParse('1.5', numDecimalPlaces: 3).ok, 1500);
          expect(formatter.tryParse('1.5', numDecimalPlaces: 6).ok, 1500000);
          expect(formatter.tryParse('12', numDecimalPlaces: 0).ok, 12);
        });

        // The worked example on [DecimalInputFormatter.tryParse].
        // TODO(nicole): remove?
        test('tryParse truncates places the output cannot hold', () {
          final formatter = DecimalInputFormatter(
            maxDecimalPlaces: 3,
            locale: 'en_US',
          );

          expect(formatter.tryParse('12.34', numDecimalPlaces: 3).ok, 12340);
          expect(formatter.tryParse('12.34', numDecimalPlaces: 2).ok, 1234);
          expect(formatter.tryParse('12.34', numDecimalPlaces: 1).ok, 123);
          expect(formatter.tryParse('12.34', numDecimalPlaces: 0).ok, 12);
          expect(formatter.tryParse('1.05', numDecimalPlaces: 1).ok, 10);
        });

        test('tryParse still enforces maxDecimalPlaces', () {
          final formatter = DecimalInputFormatter(
            maxDecimalPlaces: 3,
            locale: 'en_US',
          );

          expect(formatter.tryParse('1.2345', numDecimalPlaces: 4).isErr, true);
        });

        test('formatInt reads the input at numDecimalPlaces', () {
          final formatter = DecimalInputFormatter(
            maxDecimalPlaces: 3,
            locale: 'en_US',
          );

          expect(formatter.formatInt(1500, numDecimalPlaces: 3), '1.5');
          expect(formatter.formatInt(1500000, numDecimalPlaces: 6), '1.5');
          expect(formatter.formatInt(12, numDecimalPlaces: 0), '12');
        });

        test('formatInt truncates what the field cannot render', () {
          final formatter = DecimalInputFormatter(
            maxDecimalPlaces: 3,
            locale: 'en_US',
          );

          // 1234 at 4 places is 0.1234, one place past the field's 3.
          expect(formatter.formatInt(1234, numDecimalPlaces: 4), '0.123');
          // 12340 at 4 places is 1.234, which fits once trimmed.
          expect(formatter.formatInt(12340, numDecimalPlaces: 4), '1.234');
        });
      });

      // Both modes agree that an empty field has no amount.
      test('tryParse rejects an empty string', () {
        expect(formatter.tryParse('').isErr, true);
        expect(DecimalInputFormatter(locale: 'en_US').tryParse('').isErr, true);
      });

      test('formatInt renders the smallest unit as a decimal', () {
        expect(formatter.formatInt(0), '0');
        expect(formatter.formatInt(12000), '12');
        expect(formatter.formatInt(1500), '1.5');
        expect(formatter.formatInt(1050), '1.05');
        expect(formatter.formatInt(12345), '12.345');
        expect(formatter.formatInt(1), '0.001');
        expect(formatter.formatInt(1000500), '1,000.5');
      });

      test('tryParse and formatInt round-trip', () {
        for (final value in [0, 1, 1050, 1500, 12345, 1000500]) {
          expect(formatter.tryParse(formatter.formatInt(value)).ok, value);
        }
      });

      // Check that negative signs are rendered correctly
      test('formatInt and tryParse preserve the sign', () {
        expect(formatter.formatInt(-1), '-0.001');
        expect(formatter.formatInt(-50), '-0.05');
        expect(formatter.formatInt(-1050), '-1.05');
        expect(formatter.formatInt(-1000500), '-1,000.5');
        expect(formatter.tryParse('-0.05').ok, -50);
        expect(formatter.tryParse('-1,000.5').ok, -1000500);
      });

      // For each locale in [_decimalLocaleCases]: Check that the formatter
      // outputs the locale's digits and separators, and reads them back
      // unchanged.
      group('locales', () {
        for (final testCase in _decimalLocaleCases) {
          test(testCase.name, () {
            final formatter = DecimalInputFormatter(
              maxDecimalPlaces: 3,
              locale: testCase.locale,
            );
            expect(
              _testValues.map(formatter.formatInt),
              testCase.expectedValues,
              reason: testCase.locale,
            );
            // What [DecimalInputFormatter.formatInt] renders must survive
            // [DecimalInputFormatter.formatEditUpdate] untouched.
            for (final value in _testValues) {
              final formatted = formatter.formatInt(value);
              expect(
                formatter
                    .formatEditUpdate(
                      TextEditingValue.empty,
                      TextEditingValue(
                        text: formatted,
                        selection: TextSelection.collapsed(
                          offset: formatted.length,
                        ),
                      ),
                    )
                    .text,
                formatted,
                reason: '${testCase.locale}: $value',
              );
            }
            // Negatives too: their sign is locale-specific (U+2212 in fi, an
            // LRM-prefixed "-" in ar), so only the round-trip is asserted.
            for (final value in [
              ..._testValues,
              ..._testValues.map((value) => -value),
            ]) {
              expect(
                formatter.tryParse(formatter.formatInt(value)).ok,
                value,
                reason: '${testCase.locale}: $value',
              );
            }
          });
        }
      });
    });
  });
}

/// Amounts formatted by every case in [_decimalLocaleCases], in msat-like
/// thousandths: zero, sub-unit, mixed, grouped, and multi-group.
const _testValues = [0, 1, 50, 1050, 12345, 1000500, 123456789];

/// A locale's expected [DecimalInputFormatter.formatInt] output for
/// [_testValues], with `maxDecimalPlaces: 3`.
typedef _DecimalLocaleCase = ({
  String name,
  String locale,
  List<String> expectedValues,
});

const List<_DecimalLocaleCase> _decimalLocaleCases = [
  (
    name: 'en_US: "," groups, "." decimal',
    locale: 'en_US',
    expectedValues: [
      '0',
      '0.001',
      '0.05',
      '1.05',
      '12.345',
      '1,000.5',
      '123,456.789',
    ],
  ),
  (
    // The separators swap roles, so a de_DE "1.000,5" must not parse as en_US's
    // "1.0005".
    name: 'de_DE: "." groups, "," decimal',
    locale: 'de_DE',
    expectedValues: [
      '0',
      '0,001',
      '0,05',
      '1,05',
      '12,345',
      '1.000,5',
      '123.456,789',
    ],
  ),
  (
    // Narrow no-break space (U+202F) groups: invisible, and not the plain space
    // a hand-written test would type.
    name: 'fr_FR: U+202F groups, "," decimal',
    locale: 'fr_FR',
    expectedValues: [
      '0',
      '0,001',
      '0,05',
      '1,05',
      '12,345',
      '1 000,5',
      '123 456,789',
    ],
  ),
  (
    name: 'de_CH: U+2019 groups, "." decimal',
    locale: 'de_CH',
    expectedValues: [
      '0',
      '0.001',
      '0.05',
      '1.05',
      '12.345',
      '1’000.5',
      '123’456.789',
    ],
  ),
  (
    // Lakh/crore grouping: groups are 2 digits wide above the first 3, so the
    // separator positions aren't a fixed period.
    name: 'en_IN: lakh/crore grouping',
    locale: 'en_IN',
    expectedValues: [
      '0',
      '0.001',
      '0.05',
      '1.05',
      '12.345',
      '1,000.5',
      '1,23,456.789',
    ],
  ),
  (
    // Locales whose ZERO_DIGIT isn't ASCII "0" render in native digits
    // throughout — whole part, decimal, and separators alike.
    name: 'bn_BD: native digits',
    locale: 'bn_BD',
    expectedValues: [
      '০',
      '০.০০১',
      '০.০৫',
      '১.০৫',
      '১২.৩৪৫',
      '১,০০০.৫',
      '১,২৩,৪৫৬.৭৮৯',
    ],
  ),
  (
    // Native digits plus non-ASCII separators (U+066B decimal, U+066C group).
    // As-you-type editing can't re-enter this text (it only scans ASCII
    // digits), so such locales are display-only for now.
    name: 'fa_IR: native digits and separators',
    locale: 'fa_IR',
    expectedValues: [
      '۰',
      '۰٫۰۰۱',
      '۰٫۰۵',
      '۱٫۰۵',
      '۱۲٫۳۴۵',
      '۱٬۰۰۰٫۵',
      '۱۲۳٬۴۵۶٫۷۸۹',
    ],
  ),
];
