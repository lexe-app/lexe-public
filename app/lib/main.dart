import 'package:flutter/material.dart';

import 'route/landing.dart' show LandingPage;

void main() {
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
