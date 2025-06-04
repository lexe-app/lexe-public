/// [String] extension trait
library;

import 'package:lexeapp/address_format.dart' as address_format;

extension StringExt on String {
  /// Efficiently count the number of lines in a string.
  int countLines() {
    int count = 1;
    for (int idx = 0; idx < this.length; idx++) {
      if (this.codeUnitAt(idx) == 0x0a) {
        count += 1;
      }
    }
    return count;
  }

  /// See: [address_format.ellipsizeBtcAddress].
  String ellipsizeMid() {
    if (this.length <= 15) {
      return this;
    }

    final prefix = this.substring(0, 8);
    final suffix = this.substring(this.length - 6);

    // \u2026 == "…" (horizontal ellipsis)
    return "$prefix\u2026$suffix";
  }
}
