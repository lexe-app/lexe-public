/// [int] extension methods
library;

extension IntExt on int {
  int saturatingSub(final int other) => (this >= other) ? this - other : 0;
}
