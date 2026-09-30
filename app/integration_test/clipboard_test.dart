import 'dart:io' show Platform;

import 'package:device_info_plus/device_info_plus.dart' show DeviceInfoPlugin;
import 'package:flutter_test/flutter_test.dart' show test;
import 'package:integration_test/integration_test.dart'
    show IntegrationTestWidgetsFlutterBinding;

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  test("DeviceInfoPlugin().androidInfo", () async {
    if (!Platform.isAndroid) {
      return;
    }
    // Should not throw an exception
    await DeviceInfoPlugin().androidInfo;
  });
}
