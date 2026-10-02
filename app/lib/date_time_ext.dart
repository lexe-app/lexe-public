/// [DateTime] extension methods
library;

extension DateTimeExt on DateTime {
  /// Add calendar months. Overflowing days roll into the next month, e.g.
  /// Jan 31 + 1 month = Mar 3.
  DateTime addMonths(int months) => this.copyWith(month: this.month + months);
}
