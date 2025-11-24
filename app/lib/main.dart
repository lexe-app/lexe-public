import 'package:app_rs_dart/app_rs_dart.dart' as app_rs_dart;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show Config, DeployEnv, RootSeed;
import 'package:flutter/material.dart';
import 'package:intl/intl.dart' show Intl;
import 'package:intl/intl_standalone.dart' as intl_standalone;
import 'package:lexeapp/app_data.dart' show LxAppData;
import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/feature_flags.dart';
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart' show Err, Ok, Result;
import 'package:lexeapp/route/app_load_error.dart' show AppLoadErrorPage;
import 'package:lexeapp/route/landing.dart' show LandingPage;
import 'package:lexeapp/route/restore.dart' show RestoreApi;
import 'package:lexeapp/route/signup.dart' show SignupApi;
import 'package:lexeapp/route/wallet.dart' show WalletPage;
import 'package:lexeapp/settings.dart' show LxSettings;
import 'package:lexeapp/style.dart' show LxColors, LxTheme;
import 'package:lexeapp/uri_events.dart' show UriEvents;

Future<void> main() async {
  // runZonedGuarded(
  //   () async => await mainInner(),
  //   (error, stackTrace) => /* do something w/ error */,
  // );

  WidgetsFlutterBinding.ensureInitialized();

  // Init native Rust ffi bindings.
  await app_rs_dart.init();

  Logger.init();

  final userAgent = await cfg.UserAgent.fromPlatform();
  final Config config = await cfg.build(userAgent);
  info("Build config: $config");

  final resultApp = await Result.tryAsync<AppHandle?, Exception>(
    () => AppHandle.load(config: config),
  );
  final AppHandle? maybeApp;
  final String? loadErrorMessage;

  switch (resultApp) {
    case Ok(:final ok):
      maybeApp = ok;
      loadErrorMessage = null;
    case Err(:final err):
      error("Failed to load app: $err");
      loadErrorMessage = err.toString();
      maybeApp = null;
  }

  final uriEvents = await UriEvents.prod();

  // Determine the current system locale and set the global `Intl.systemLocale`.
  await intl_standalone.findSystemLocale();

  // Initialize date formatting locale data for ALL locales. Adds a few 100 KiB
  // to binary size, but much simpler.
  await date_format.initializeDateLocaleData();

  // Skip GDrive auth in local dev.
  final gdriveAuth = switch (config.deployEnv) {
    DeployEnv.dev => GDriveAuth.mock,
    DeployEnv.prod || DeployEnv.staging => GDriveAuth.prod,
  };

  final Widget child;
  if (loadErrorMessage != null) {
    child = AppLoadErrorPage(errorMessage: loadErrorMessage);
  } else if (maybeApp != null) {
    final app = maybeApp;
    final settings = LxSettings(app.settingsDb());
    final appData = LxAppData(app.appDb());
    final featureFlags = FeatureFlags(
      deployEnv: config.deployEnv,
      userPk: app.userInfo().userPk,
    );

    // If user has a locale preference set then use that over the system locale.
    final locale = settings.locale.value;
    if (locale != null) {
      Intl.defaultLocale = settings.locale.value;
    }

    // wallet already exists => show wallet page
    child = WalletPage(
      config: config,
      app: app,
      settings: settings,
      appData: appData,
      featureFlags: featureFlags,
      uriEvents: uriEvents,
      gdriveAuth: gdriveAuth,
    );
  } else {
    // no wallet persisted => first run -> show landing
    child = LandingPage(
      config: config,
      rootSeed: RootSeed.fromSysRng(),
      gdriveAuth: gdriveAuth,
      signupApi: SignupApi.prod,
      restoreApi: RestoreApi.prod,
      uriEvents: uriEvents,
      fixedShaderTime: null,
    );
  }

  runApp(LexeApp(child: child));
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
