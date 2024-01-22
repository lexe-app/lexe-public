import 'package:flutter/material.dart';
import 'package:intl/intl_standalone.dart' as intl_standalone;

import 'bindings.dart' show api;
import 'bindings_generated_api.dart' show AppHandle, Config;
import 'cfg.dart' as cfg;
import 'date_format.dart' as date_format;
import 'gdrive_auth.dart' show GDriveAuth;
import 'logger.dart' as logger;
import 'logger.dart' show info;
import 'route/landing.dart' show LandingPage;
import 'route/signup.dart' show SignupApi;
import 'route/wallet.dart' show WalletPage;
import 'style.dart' show LxColors, LxTheme;

Future<void> main() async {
  // runZonedGuarded(
  //   () async => await mainInner(),
  //   (error, stackTrace) => /* do something w/ error */,
  // );

  WidgetsFlutterBinding.ensureInitialized();

  // TODO(phlip9): allow overriding default locale in preferences.
  // Intl.defaultLocale = settings.getUserPreferredLocale();

  // This fn determines the current system locale and sets `Intl.systemLocale`
  // to it.
  await intl_standalone.findSystemLocale();

  // Initialize date formatting locale data for ALL locales.
  await date_format.initializeDateLocaleData();

  logger.init();

  final Config config = await cfg.build();
  info("Build config: $config");

  final maybeApp = await AppHandle.load(bridge: api, config: config);

  final Widget child;
  if (maybeApp != null) {
    // wallet already exists => show wallet page
    child = WalletPage(config: config, app: maybeApp);
  } else {
    // no wallet persisted => first run -> show landing
    child = LandingPage(
      config: config,
      gdriveAuth: GDriveAuth.prod,
      signupApi: SignupApi.prod,
    );
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
