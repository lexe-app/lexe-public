// ignore_for_file: avoid_print

import 'dart:async' show unawaited;

import 'package:app_rs_dart/app_rs_dart.dart' as app_rs_dart;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/design_mode/main.dart' show LexeDesignPage;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/style.dart' show LxColors, LxTheme;
import 'package:lexeapp/uri_events.dart';

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    // Initialize Rust FFI
    await app_rs_dart.init();

    // Initialize date formatting
    await date_format.initializeDateLocaleData();

    // Initialize logger
    Logger.init();
  });

  testWidgets('Take screenshots for documentation', (
    WidgetTester tester,
  ) async {
    // Build config for design mode
    final userAgent = await cfg.UserAgent.fromPlatform();
    final config = await cfg.buildTest(userAgent: userAgent);

    final uriEvents = await UriEvents.prod();

    // Build the design mode app
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

    // Wait for all widgets to render
    await tester.pump(const Duration(seconds: 2));
    await tester.pumpAndSettle();

    // Convert the Flutter surface to an image (required on Android)
    await binding.convertFlutterSurfaceToImage();
    await tester.pump();

    // Get the design page state to access components directly
    final designPageFinder = find.byType(LexeDesignPage);
    if (designPageFinder.evaluate().isEmpty) {
      throw Exception('LexeDesignPage not found in widget tree');
    }

    final designPageElement = designPageFinder.evaluate().first;
    final designPageState = designPageElement as StatefulElement;
    final state = designPageState.state as dynamic;

    // Build the components list
    final components = state.buildComponentsList(designPageElement) as List;

    // Filter to only components with a screenshot path
    final screenshotComponents = components.where((component) {
      final comp = component as dynamic;
      return comp.screenshot != null;
    }).toList();

    print('Found ${screenshotComponents.length} components to screenshot');

    for (final component in screenshotComponents) {
      final comp = component as dynamic;
      final componentName = comp.title as String;
      final outputPath = comp.screenshot as String;
      final builder = comp.builder as Widget Function(BuildContext);

      print('Taking screenshot for $componentName -> $outputPath');

      // Navigate to the component page by pushing a route
      final navigator = Navigator.of(designPageElement);
      unawaited(navigator.push(MaterialPageRoute(builder: builder)));

      // Wait for navigation animation to start
      await tester.pump(const Duration(milliseconds: 100));

      // Verify we navigated (for debugging)
      if (!navigator.canPop()) {
        print('  ⚠ Navigation failed for $componentName');
        continue;
      }

      // Wait for all animations and async operations to complete
      // Use individual pump calls to avoid timeout on continuous animations
      for (int i = 0; i < 30; i++) {
        await tester.pump(const Duration(milliseconds: 100));
      }

      // Take the screenshot
      await binding.takeScreenshot(outputPath);

      // Navigate back
      navigator.pop();

      // Wait for back navigation to complete
      for (int i = 0; i < 15; i++) {
        await tester.pump(const Duration(milliseconds: 100));
      }

      print('  ✓ Screenshot saved');
    }

    print('All screenshots completed!');
  });
}
