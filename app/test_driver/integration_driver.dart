// ignore_for_file: avoid_print

import 'dart:io';

import 'package:integration_test/integration_test_driver.dart';

Future<void> main() => integrationDriver(
  responseDataCallback: (Map<String, dynamic>? data) async {
    if (data != null) {
      // Save screenshots to individual files
      final List<dynamic>? screenshots = data['screenshots'] as List<dynamic>?;
      if (screenshots != null) {
        for (final dynamic screenshot in screenshots) {
          final Map<String, dynamic> screenshotData =
              screenshot as Map<String, dynamic>;
          final String name = screenshotData['screenshotName'] as String;
          final List<dynamic> bytesData =
              screenshotData['bytes'] as List<dynamic>;
          final List<int> bytes = List<int>.from(bytesData);

          final File file = File('build/screenshots/$name');
          await file.create(recursive: true);
          await file.writeAsBytes(bytes);
          print('Screenshot saved: build/screenshots/$name');
        }
      }

      // Also write the JSON response data
      await writeResponseData(data);
    }
  },
);
