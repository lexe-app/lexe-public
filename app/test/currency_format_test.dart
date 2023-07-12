import 'package:flutter_test/flutter_test.dart' show expect, test;

import 'package:lexeapp/bindings_generated_api.dart' show PaymentDirection;
import 'package:lexeapp/currency_format.dart' as currency_format;

void assertApproxEq(double actual, double expected, {double eps = 1e-9}) {
  final absDiff = (actual - expected).abs();
  assert(
    absDiff <= eps,
    '''Expected numbers to be approximately equal

    error: |$actual - $expected| = $absDiff > ε ($eps)
    ''',
  );
}

void main() {
  test("currency_format.satsToBtc", () {
    assertApproxEq(0.00001234, currency_format.satsToBtc(1234));
  });

  test("currency_format.formatSatsAmount", () {
    expect(currency_format.formatSatsAmount(0, locale: "en_US"), "0 sats");
    expect(currency_format.formatSatsAmount(0, locale: "da_DK"), "0 sats");
    expect(currency_format.formatSatsAmount(0, locale: "fr_FR"), "0 sats");

    expect(currency_format.formatSatsAmount(73000, locale: "en_US"),
        "73,000 sats");
    expect(currency_format.formatSatsAmount(73000, locale: "da_DK"),
        "73.000 sats");
    // \u202f - thousands separator
    expect(currency_format.formatSatsAmount(73000, locale: "fr_FR"),
        "73\u202F000 sats");

    expect(
      currency_format.formatSatsAmount(
        73000,
        direction: PaymentDirection.Inbound,
        satsSuffix: false,
        locale: "en_US",
      ),
      "+73,000",
    );
    expect(
      currency_format.formatSatsAmount(
        73000,
        direction: PaymentDirection.Inbound,
        satsSuffix: false,
        locale: "da_DK",
      ),
      "+73.000",
    );
    expect(
      currency_format.formatSatsAmount(
        73000,
        direction: PaymentDirection.Inbound,
        satsSuffix: false,
        locale: "fr_FR",
      ),
      // \u202f - unicode thousands separator
      "+73\u202f000",
    );
  });

  test("currency_format.formatFiatParts", () {
    expect(("\$1,234", ".57"),
        currency_format.formatFiatParts(1234.5678, "USD", locale: "en_US"));
    // \xa0 - non-breaking space
    expect(("1.234", ",57\xa0kr"),
        currency_format.formatFiatParts(1234.5678, "DKK", locale: "da_DK"));
    // \u202f - unicode thousands separator
    expect(("1\u202f234", ",57\xa0€"),
        currency_format.formatFiatParts(1234.5678, "EUR", locale: "fr_FR"));
  });
}
