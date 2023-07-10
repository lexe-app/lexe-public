import 'dart:async' show unawaited;

import 'package:flutter/material.dart';

import '../../bindings_generated_api.dart' show AppHandle, Config;
import '../../components.dart' show LxCloseButton;
import '../../route/wallet.dart' show WalletPage;
import '../../style.dart' show Fonts, LxColors, Space;

class BackupWalletPage extends StatelessWidget {
  const BackupWalletPage({super.key, required this.config, required this.app});

  final Config config;
  final AppHandle app;

  void skipBackup(BuildContext context) {
    // TODO(phlip9): show warning that skipping backup is bad >:(

    unawaited(Navigator.of(context).pushReplacement(MaterialPageRoute(
      maintainState: false,
      builder: (BuildContext _) =>
          WalletPage(config: this.config, app: this.app),
    )));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        automaticallyImplyLeading: false,
        leading: const LxCloseButton(),
      ),
      body: ListView(
        padding: const EdgeInsets.symmetric(horizontal: Space.s500),
        children: [
          const SizedBox(height: Space.s900),
          const Center(child: Text("Backup Wallet Page")),
          const SizedBox(height: Space.s900),
          Builder(
            builder: (context) => OutlinedButton(
              onPressed: () => this.skipBackup(context),
              style: OutlinedButton.styleFrom(
                backgroundColor: LxColors.background,
                foregroundColor: LxColors.grey600,
                side: const BorderSide(color: LxColors.grey600, width: 2.0),
                padding: const EdgeInsets.symmetric(vertical: Space.s500),
              ),
              child: Text(
                "Skip Backup",
                style: Fonts.fontUI.copyWith(
                  fontSize: Fonts.size400,
                  fontVariations: [Fonts.weightMedium],
                  color: LxColors.grey500,
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
