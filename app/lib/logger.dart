import 'package:flutter/foundation.dart' show debugPrint;

import 'bindings.dart' show api;

Future<void> init() async {
  final rustLogStream = api.initRustLogStream();

  rustLogStream.listen((logEntry) {
    debugPrint(logEntry.message);
  });
}
