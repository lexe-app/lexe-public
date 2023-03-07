import 'package:flutter/material.dart';

import 'bindings.dart' show api;
import 'bindings_generated_api.dart' show AppHandle, Config;
import 'route/landing.dart' show LandingPage;

Future<void> main() async {
  // TODO(phlip9): load initial state
  // TODO(phlip9): navigate to wallet if already signed up or landing o/w

  // runZonedGuarded(
  //   () async => await mainInner(),
  //   (error, stackTrace) => /* do something w/ error */,
  // );

  final config = Config.regtest(bridge: api);
  final maybeApp = await AppHandle.load(bridge: api, config: config);

  final Widget child;
  if (maybeApp != null) {
    // final app = maybeApp!;
    // TODO(phlip9): already have wallet persisted
    child = const SizedBox();
  } else {
    // no wallet persisted => first run -> show landing
    child = const LandingPage();
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
      title: 'Lexe',
      home: this.child,
    );
  }
}
