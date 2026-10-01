import 'dart:io' show Platform;

import 'package:device_info_plus/device_info_plus.dart' show DeviceInfoPlugin;
import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:integration_test/integration_test.dart'
    show IntegrationTestWidgetsFlutterBinding;
import 'package:lexeapp/uri_events.dart' show UriEvents;
import 'package:url_launcher/url_launcher.dart' as url_launcher;

const Duration timeout = Duration(seconds: 15);

void main() async {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  // This test launches "bitcoin:" URIs and expects the OS to route them back
  // into this app via `app_links`. That only holds on a fresh emulator or
  // simulator, where this app is the only "bitcoin:" handler. On desktop or
  // physical devices, prod Lexe or other wallets may intercept them.
  final skip = await isMobileEmulator()
      ? null
      : "Requires a mobile emulator/simulator";

  test("UriEvents receives payment URIs routed back to the app", () async {
    final uriEvents = await UriEvents.prod();
    expect(uriEvents.initialUri, null);

    // Delivered to an active listener
    const uri1 = "bitcoin:bcrt1qxvnuxcz5j64y7sgkcdyxag8c9y4uxagj2u02fk";
    final next1 = uriEvents.uriStream.first;
    await launchExternal(uri1);
    expect(await next1.timeout(timeout), uri1);

    // Only the latest URI is replayed to a late listener, e.g., `WalletPage`
    // after signup
    const uri2 = "$uri1?amount=0.0001";
    final next2 = uriEvents.uriStream.skip(1).first;
    await launchExternal(uri2);
    expect(await next2.timeout(timeout), uri2);
    expect(await uriEvents.uriStream.first.timeout(timeout), uri2);
  }, skip: skip);
}

Future<void> launchExternal(String uri) async {
  final launched = await url_launcher.launchUrl(
    Uri.parse(uri),
    mode: url_launcher.LaunchMode.externalApplication,
  );
  expect(launched, true);
}

Future<bool> isMobileEmulator() async {
  final deviceInfo = DeviceInfoPlugin();
  if (Platform.isAndroid) {
    return !(await deviceInfo.androidInfo).isPhysicalDevice;
  } else if (Platform.isIOS) {
    return !(await deviceInfo.iosInfo).isPhysicalDevice;
  } else {
    return false;
  }
}
