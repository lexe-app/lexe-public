import 'package:flutter_test/flutter_test.dart' show test, expect;

import 'package:lexeapp/bindings.dart' show api;

void main() {
  test("bindings work", () {
    expect(api.hello(), "hello!");
  });
}
