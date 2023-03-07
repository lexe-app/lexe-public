import 'package:flutter_test/flutter_test.dart' show expect, test;

import 'package:lexeapp/cfg.dart' as cfg;

void main() {
  test("cfg.debug is always true in unit tests", () {
    expect(cfg.debug, true);
  });

  test("cfg.test is always true in unit tests", () {
    expect(cfg.test, true);
  });
}
