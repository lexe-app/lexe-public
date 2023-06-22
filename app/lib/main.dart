import 'package:flutter/material.dart';

import 'bindings.dart' show api;
import 'bindings_generated_api.dart' show AppHandle, Config;
import 'cfg.dart' as cfg;
import 'logger.dart' as logger;
import 'logger.dart' show info;
import 'route/landing.dart' show LandingPage;
import 'route/wallet.dart' show WalletPage;
import 'style.dart' show LxColors, LxTheme;

Future<void> main() async {
  // runZonedGuarded(
  //   () async => await mainInner(),
  //   (error, stackTrace) => /* do something w/ error */,
  // );

  // TODO(phlip9): initialize dart internationalization for preferred system
  //               locale. <https://pub.dev/packages/intl#initialization>

  WidgetsFlutterBinding.ensureInitialized();

  logger.init();

  final Config config = await cfg.build();
  info("Build config: $config");

  final maybeApp = await AppHandle.load(bridge: api, config: config);

  final Widget child;
  if (maybeApp != null) {
    // wallet already exists => show wallet page
    child = WalletPage(app: maybeApp);
  } else {
    // no wallet persisted => first run -> show landing
    child = LandingPage(config: config);
  }

  runApp(LexeApp(
    child: child,
  ));
}

class LexeApp extends StatelessWidget {
  const LexeApp({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: "Lexe App",
      color: LxColors.background,
      themeMode: ThemeMode.light,
      theme: LxTheme.light(),
      debugShowCheckedModeBanner: false,
      home: this.child,
    );
  }
}
