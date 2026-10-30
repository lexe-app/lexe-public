// ignore_for_file: avoid_print

import 'dart:async' show unawaited;

import 'package:app_rs_dart/app_rs_dart.dart' as app_rs_dart;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/design_mode/main.dart' show Component, LexeDesignPage;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/style.dart' show LxColors, LxTheme;
import 'package:lexeapp/uri_events.dart';

/// Screenshots each design-mode [Component] with a `screenshot` path.
/// Run via `just docs-update-screenshots`.
void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    await app_rs_dart.init();
    await date_format.initializeDateLocaleData();
    Logger.init();
  });

  testWidgets('Take screenshots for documentation', (tester) async {
    final userAgent = await cfg.UserAgent.fromPlatform();
    final config = await cfg.buildTest(userAgent: userAgent);
    final uriEvents = await UriEvents.prod();

    await tester.pumpWidget(
      MaterialApp(
        title: "Lexe App - Design Mode",
        color: LxColors.background,
        themeMode: ThemeMode.light,
        theme: LxTheme.light(),
        darkTheme: null,
        debugShowCheckedModeBanner: false,
        home: LexeDesignPage(config: config, uriEvents: uriEvents),
      ),
    );
    await tester.pumpAndSettle();

    // Required on Android before `takeScreenshot`.
    await binding.convertFlutterSurfaceToImage();
    await tester.pump();

    final designPage = find.byType(LexeDesignPage).evaluate().single;
    final designState = (designPage as StatefulElement).state as dynamic;
    final components =
        designState.buildComponentsList(designPage) as List<Component>;
    final navigator = Navigator.of(designPage);

    for (final component in components.expand((c) => [c, ...c.sublist])) {
      final screenshot = component.screenshot;
      if (screenshot == null) continue;
      print('${component.title} -> $screenshot');

      unawaited(navigator.push(MaterialPageRoute(builder: component.builder)));
      await pumpFor(tester, const Duration(seconds: 3));
      await binding.takeScreenshot(screenshot);

      navigator.pop();
      await pumpFor(tester, const Duration(milliseconds: 1500));
    }
  });
}

/// Pumps a frame every 100ms for [duration]. Unlike `pumpAndSettle`, this
/// can't time out on pages with continuous animations.
Future<void> pumpFor(WidgetTester tester, Duration duration) async {
  const frame = Duration(milliseconds: 100);
  for (var elapsed = Duration.zero; elapsed < duration; elapsed += frame) {
    await tester.pump(frame);
  }
}
