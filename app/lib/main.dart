import 'package:flutter/material.dart';

import 'route/landing.dart' show LandingPage;

void main() {
  // TODO: load initial state
  // TODO: navigate to wallet if already signed up or landing o/w

  runApp(const LexeApp());
}

class LexeApp extends StatelessWidget {
  const LexeApp({super.key});

  @override
  Widget build(BuildContext context) {
    return const MaterialApp(
      title: 'Lexe',
      home: LandingPage(),
    );
  }
}
