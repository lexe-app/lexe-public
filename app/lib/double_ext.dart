/// [double] extension methods
library;

extension DoubleExt on double {
  /// Returns the fractional part of this [double].
  double fract() => this - this.floor();
}
