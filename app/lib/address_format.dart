/// Bitcoin and lightning address formatting
library;

import 'package:lexeapp/string_ext.dart';

/// Shorten a long bitcoin address by inserting an ellipsis (...) into the
/// _middle_ while leaving the start and end intact. Users often compare
/// addresses by both the first and last characters. In contrast, the default
/// flutter ellipsis truncation only shows the initial characters.
///
/// ### Examples
///
/// ```dart
/// assert(ellipsizeBtcAddress("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4") ==
///     "bc1qw508…v8f3t4");
/// );
/// ```
String ellipsizeBtcAddress(String address) => address.ellipsizeMid();
