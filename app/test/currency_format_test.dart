import 'package:app_rs_dart/ffi/types.dart' show PaymentDirection;
import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:lexeapp/currency_format.dart' as currency_format;

void assertApproxEq(double actual, double expected, {double eps = 1e-9}) {
  final absDiff = (actual - expected).abs();
  assert(absDiff <= eps, '''Expected numbers to be approximately equal

    error: |$actual - $expected| = $absDiff > ε ($eps)
    ''');
}

void main() {
  test("currency_format.satsToBtc", () {
    assertApproxEq(0.00001234, currency_format.satsToBtc(1234));
  });

  test("currency_format.formatSatsToBtcForUri", () {
    const btc = 100000000;

    expect("0.0", currency_format.formatSatsToBtcForUri(0));
    expect("0.00000001", currency_format.formatSatsToBtcForUri(1));
    expect("0.000025", currency_format.formatSatsToBtcForUri(2500));
    expect("23.000025", currency_format.formatSatsToBtcForUri(23 * btc + 2500));
    expect("1.0", currency_format.formatSatsToBtcForUri(btc));
    expect("0.1", currency_format.formatSatsToBtcForUri(btc ~/ 10));
    expect("0.01", currency_format.formatSatsToBtcForUri(btc ~/ 100));
    expect("0.00123", currency_format.formatSatsToBtcForUri(123000));
    expect("1.00123", currency_format.formatSatsToBtcForUri(btc + 123000));
  });

  test("currency_format.formatSatsAmount", () {
    expect(currency_format.formatSatsAmount(0, locale: "en_US"), "0 sats");
    expect(currency_format.formatSatsAmount(0, locale: "da_DK"), "0 sats");
    expect(currency_format.formatSatsAmount(0, locale: "fr_FR"), "0 sats");

    expect(
      currency_format.formatSatsAmount(73000, locale: "en_US"),
      "73,000 sats",
    );
    expect(
      currency_format.formatSatsAmount(73000, locale: "da_DK"),
      "73.000 sats",
    );
    // \u202f - thousands separator
    expect(
      currency_format.formatSatsAmount(73000, locale: "fr_FR"),
      "73\u202F000 sats",
    );

    expect(
      currency_format.formatSatsAmount(
        73000,
        direction: PaymentDirection.inbound,
        satsSuffix: false,
        locale: "en_US",
      ),
      "+73,000",
    );
    expect(
      currency_format.formatSatsAmount(
        73000,
        direction: PaymentDirection.inbound,
        satsSuffix: false,
        locale: "da_DK",
      ),
      "+73.000",
    );
    expect(
      currency_format.formatSatsAmount(
        73000,
        direction: PaymentDirection.inbound,
        satsSuffix: false,
        locale: "fr_FR",
      ),
      // \u202f - unicode thousands separator
      "+73\u202f000",
    );
  });

  test("currency_format.formatFiatParts", () {
    expect((
      "\$1,234",
      ".57",
    ), currency_format.formatFiatParts(1234.5678, "USD", locale: "en_US"));
    // \xa0 - non-breaking space
    expect((
      "1.234",
      ",57\xa0kr",
    ), currency_format.formatFiatParts(1234.5678, "DKK", locale: "da_DK"));
    // \u202f - unicode thousands separator
    expect((
      "1\u202f234",
      ",57\xa0€",
    ), currency_format.formatFiatParts(1234.5678, "EUR", locale: "fr_FR"));
  });
}
