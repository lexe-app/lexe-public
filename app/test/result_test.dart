import 'package:flutter_test/flutter_test.dart' show expect, test;

import 'package:lexeapp/result.dart';

int conjure3() => 3;

void main() {
  test("result : operator == and hashCode", () {
    const Result<int, void> ok1 = Ok(5);
    final int three = conjure3();
    final Result<int, void> ok2 = Ok(2 + three);
    final Result<int, void> ok3 = Ok(4 + three);
    final Result<int, void> ok4 = Ok(4 + three).map((x) => x - 2);

    expect(5, ok1.unwrap());
    expect(5, ok2.unwrap());
    expect(7, ok3.unwrap());
    expect(5, ok4.unwrap());

    assert(ok1 == ok2);
    assert(ok2 != ok3);
    assert(ok2 == ok4);

    assert(ok1.hashCode == ok2.hashCode);
    assert(ok1.hashCode != ok3.hashCode);
    assert(ok1.hashCode == ok4.hashCode);
  });
}
