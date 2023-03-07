import 'package:flutter/material.dart';

import '../../bindings_generated_api.dart' show AppHandle;
import '../style.dart' show Fonts, LxColors;

class BackupWalletPage extends StatelessWidget {
  const BackupWalletPage({super.key, required this.app});

  final AppHandle app;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Center(
        child: Text("wallet page",
            style: Fonts.fontHero.copyWith(color: LxColors.grey150)),
      ),
    );
  }
}
