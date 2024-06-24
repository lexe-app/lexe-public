import 'package:flutter/material.dart';
import 'package:intl/intl_standalone.dart' as intl_standalone;
import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/bindings_generated_api.dart'
    show AppHandle, Config, DeployEnv;
import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/route/landing.dart' show LandingPage;
import 'package:lexeapp/route/signup.dart' show SignupApi;
import 'package:lexeapp/route/wallet.dart' show WalletPage;
import 'package:lexeapp/style.dart' show LxColors, LxTheme;
import 'package:lexeapp/uri_events.dart' show UriEvents;

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

  Logger.init();

  final Config config = await cfg.build();
  info("Build config: $config");

  final uriEvents = await UriEvents.prod();
  info("UriEvents: initialUri: ${uriEvents.initialUri}");

  final maybeApp = await AppHandle.load(bridge: api, config: config);

  final Widget child;
  if (maybeApp != null) {
    // wallet already exists => show wallet page
    child = WalletPage(
      config: config,
      app: maybeApp,
      uriEvents: uriEvents,
    );
  } else {
    // Skip GDrive auth in local dev.
    final gdriveAuth = switch (config.deployEnv) {
      DeployEnv.Dev => GDriveAuth.mock,
      DeployEnv.Prod || DeployEnv.Staging => GDriveAuth.prod,
    };

    // no wallet persisted => first run -> show landing
    child = LandingPage(
      config: config,
      gdriveAuth: gdriveAuth,
      signupApi: SignupApi.prod,
      uriEvents: uriEvents,
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
