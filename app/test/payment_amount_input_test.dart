import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:intl/intl.dart' show Intl;
import 'package:lexeapp/components.dart'
    show PaymentAmountInput, PaymentAmountInputState;
import 'package:lexeapp/result.dart' show Ok, Result;

void main() {
  setUpAll(() => Intl.defaultLocale = 'en_US');

  group('exposes typed amounts as msat', () {
    testWidgets('whole-sat mode scales sats up to msat', (tester) async {
      final key = await pumpAmountInput(tester);

      await type(tester, '2');
      expect(key.currentState!.msat.value, 2000);

      await type(tester, '1234');
      expect(key.currentState!.msat.value, 1234000);
    });

    testWidgets('decimal mode reads sub-sat precision', (tester) async {
      final key = await pumpAmountInput(tester, decimal: true);

      await type(tester, '1.5');
      expect(key.currentState!.msat.value, 1500);

      await type(tester, '0.001');
      expect(key.currentState!.msat.value, 1);

      await type(tester, '12.345');
      expect(key.currentState!.msat.value, 12345);
    });

    testWidgets('whole-sat mode rejects the decimal separator', (tester) async {
      final key = await pumpAmountInput(tester);
      await type(tester, '5');
      await type(tester, '5.5');

      expect(fieldText(tester), '5');
      expect(key.currentState!.msat.value, 5000);
    });
  });

  group('seeds from initialMsatValue', () {
    // `(initialMsatValue, displayed text, exposed msat)`.
    const wholeSatSeeds = <(int?, String, int)>[
      (null, '0', 0),
      (0, '0', 0),
      (1000, '1', 1000),
      (1500, '1', 1000),
      (123456789, '123,456', 123456000),
    ];
    for (final (initial, text, msat) in wholeSatSeeds) {
      testWidgets('whole-sat mode: $initial msat', (tester) async {
        final key = await pumpAmountInput(tester, initialMsatValue: initial);
        expect(fieldText(tester), text);
        expect(key.currentState!.msat.value, msat);
      });
    }

    // `(initialMsatValue, displayed text, exposed msat)`.
    const decimalSeeds = <(int?, String, int)>[
      (null, '0', 0),
      (1, '0.001', 1),
      (1500, '1.5', 1500),
      (12345, '12.345', 12345),
      (1000500, '1,000.5', 1000500),
    ];
    for (final (initial, text, msat) in decimalSeeds) {
      testWidgets('decimal mode: $initial msat', (tester) async {
        final key = await pumpAmountInput(
          tester,
          decimal: true,
          initialMsatValue: initial,
        );
        expect(fieldText(tester), text);
        expect(key.currentState!.msat.value, msat);
      });
    }

    testWidgets('re-entering the displayed text leaves msat unchanged', (
      tester,
    ) async {
      for (final decimal in [false, true]) {
        for (final initial in [0, 1, 1500, 12345, 1000500, 123456789]) {
          final key = await pumpAmountInput(
            tester,
            decimal: decimal,
            initialMsatValue: initial,
          );
          final seeded = key.currentState!.msat.value;

          await type(tester, fieldText(tester));

          expect(
            key.currentState!.msat.value,
            seeded,
            reason: 'decimal: $decimal, initialMsatValue: $initial',
          );
        }
      }
    });
  });

  group('an empty field has no amount', () {
    testWidgets('whole-sat mode', (tester) async {
      final key = await pumpAmountInput(tester);
      await type(tester, '5');
      await type(tester, '');

      expect(key.currentState!.msat.value, isNull);
    });

    testWidgets('decimal mode', (tester) async {
      final key = await pumpAmountInput(tester, decimal: true);
      await type(tester, '5');
      await type(tester, '');

      expect(key.currentState!.msat.value, isNull);
    });
  });

  testWidgets('onMsatAmountChanged reports what msat reports', (tester) async {
    int? reportedMsat;
    final key = await pumpAmountInput(
      tester,
      onMsatAmountChanged: (msat) => reportedMsat = msat,
    );

    for (final text in ['1', '12', '120', '']) {
      await type(tester, text);
      expect(
        reportedMsat,
        key.currentState!.msat.value,
        reason: 'typed "$text"',
      );
    }
  });

  testWidgets('validate is handed msat, not the field value', (tester) async {
    int? validateMsatAmount;
    final key = await pumpAmountInput(
      tester,
      validate: ({required int msat}) {
        validateMsatAmount = msat;
        return const Ok(());
      },
    );

    // The field shows sats; validate must be handed the msat equivalent.
    for (final (text, msat) in [('2', 2000), ('1234', 1234000), ('0', 0)]) {
      // Reset value from previous loop.
      validateMsatAmount = null;
      await type(tester, text);

      expect(key.currentState!.validate(), isTrue, reason: 'typed "$text"');
      expect(validateMsatAmount, msat, reason: 'typed "$text"');
    }
  });

  group('rejects zero when allowZero is false', () {
    testWidgets('whole-sat mode', (tester) async {
      final key = await pumpAmountInput(tester, allowZero: false);
      await type(tester, '0');

      expect(key.currentState!.validate(), isFalse);
    });

    testWidgets('decimal mode', (tester) async {
      final key = await pumpAmountInput(
        tester,
        allowZero: false,
        decimal: true,
      );
      await type(tester, '0');

      expect(key.currentState!.validate(), isFalse);
    });
  });

  group('rejects an empty field when allowEmpty is false', () {
    testWidgets('whole-sat mode', (tester) async {
      final key = await pumpAmountInput(tester, allowEmpty: false);
      await type(tester, '');

      expect(key.currentState!.validate(), isFalse);
    });

    testWidgets('decimal mode', (tester) async {
      final key = await pumpAmountInput(
        tester,
        allowEmpty: false,
        decimal: true,
      );
      await type(tester, '');

      expect(key.currentState!.validate(), isFalse);
    });
  });
}

/// Mount a [PaymentAmountInput], returning the key its state is read through.
Future<GlobalKey<PaymentAmountInputState>> pumpAmountInput(
  WidgetTester tester, {
  bool decimal = false,
  bool allowEmpty = true,
  bool allowZero = true,
  int? initialMsatValue,
  ValueChanged<int?>? onMsatAmountChanged,
  Result<(), String> Function({required int msat})? validate,
}) async {
  final key = GlobalKey<PaymentAmountInputState>();
  await tester.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: PaymentAmountInput(
          key: key,
          allowEmpty: allowEmpty,
          allowZero: allowZero,
          decimal: decimal,
          initialMsatValue: initialMsatValue,
          onMsatAmountChanged: onMsatAmountChanged,
          validate: validate,
        ),
      ),
    ),
  );
  return key;
}

/// The text the amount field is currently displaying.
String fieldText(WidgetTester tester) =>
    tester.widget<TextField>(find.byType(TextField)).controller!.text;

/// Replace the field's contents, as a paste would.
Future<void> type(WidgetTester tester, String text) async {
  await tester.enterText(find.byType(TextField), text);
  await tester.pump();
}
