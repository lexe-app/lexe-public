import 'package:flutter/material.dart';

import 'route/landing.dart' show LandingPage;

import 'bindings.dart' show api;
import 'bindings_generated_api.dart' show Config;

Future<void> main() async {
  // TODO: load initial state
  // TODO: navigate to wallet if already signed up or landing o/w

  // runZonedGuarded(
  //   () async => await mainInner(),
  //   (error, stackTrace) => /* do something w/ error */,
  // );

  final config = Config.regtest(bridge: api);
  final haveWallet = await api.appLoad(config: config);

  final Widget child;
  if (haveWallet) {
    // TODO
    child = const SizedBox();
  } else {
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
      home: child,
    );
  }
}
