import 'dart:io';

import 'package:integration_test/integration_test_driver_extended.dart';

/// Saves each screenshot the test takes to `build/screenshots/<name>`.
Future<void> main() => integrationDriver(
  onScreenshot: (name, bytes, [_]) async {
    final file = File('build/screenshots/$name');
    await file.create(recursive: true);
    await file.writeAsBytes(bytes);
    return true;
  },
  responseDataCallback: null,
);
