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
}
