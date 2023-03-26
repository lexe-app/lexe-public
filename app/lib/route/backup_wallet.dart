import 'dart:async' show unawaited;

import 'package:flutter/material.dart';

import '../../bindings_generated_api.dart' show AppHandle;
import '../style.dart' show Fonts, LxColors, Space;
import 'wallet.dart' show WalletPage;

class BackupWalletPage extends StatelessWidget {
  const BackupWalletPage({super.key, required this.app});

  final AppHandle app;

  void skipBackup(BuildContext context) {
    // TODO(phlip9): show warning that skipping backup is bad >:(

    unawaited(Navigator.of(context).pushReplacement(MaterialPageRoute(
      maintainState: false,
      builder: (BuildContext _) => WalletPage(app: this.app),
    )));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        automaticallyImplyLeading: false,
        leading: Builder(
          builder: (context) => IconButton(
            iconSize: Fonts.size700,
            icon: const Icon(Icons.close_rounded),
            onPressed: () => this.skipBackup(context),
          ),
        ),
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
