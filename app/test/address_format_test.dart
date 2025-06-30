import 'package:flutter_test/flutter_test.dart' show expect, test;

import 'package:lexeapp/address_format.dart' as address_format;

void main() {
  test("address_format.ellipsizeBtcAddress", () {
    expect(
      address_format.ellipsizeBtcAddress(
        "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
      ),
      "bc1qw508\u2026v8f3t4",
    );

    const address = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";

    for (var i = 0; i <= address.length; i += 1) {
      final prefix = address.substring(0, i);
      assert(address_format.ellipsizeBtcAddress(prefix).length <= 15);
    }
  });
}
