import 'package:flutter/material.dart';

import '../../bindings_generated_api.dart' show AppHandle;

class BackupWalletPage extends StatelessWidget {
  const BackupWalletPage({super.key, required this.app});

  final AppHandle app;

  @override
  Widget build(BuildContext context) {
    return const Scaffold(
        body: Center(
      child: Text("backup page"),
    ));
  }
}
