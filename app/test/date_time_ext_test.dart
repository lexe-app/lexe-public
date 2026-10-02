import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:lexeapp/date_time_ext.dart' show DateTimeExt;

void main() {
  test("DateTimeExt.addMonths", () {
    expect(
      DateTime(2026, 9, 21, 15, 30, 5).addMonths(1),
      DateTime(2026, 10, 21, 15, 30, 5),
    );
    // Rolls into the next year.
    expect(DateTime(2026, 9, 21).addMonths(12), DateTime(2027, 9, 21));
    // Overflowing days roll into the next month.
    expect(DateTime(2026, 1, 31).addMonths(1), DateTime(2026, 3, 3));
    // Stays in UTC.
    expect(DateTime.utc(2026, 9, 21).addMonths(1), DateTime.utc(2026, 10, 21));
  });
}
