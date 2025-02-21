/// # Lexe Debug Page
///
/// A page for manipulating app internals during development.
library;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/debug.dart' as debug;
import 'package:app_rs_dart/ffi/types.dart' show Config;
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show HeadingText, LxCloseButton, ScrollableSinglePageBody, SubheadingText;
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/result.dart';
import 'package:lexeapp/settings.dart' show LxSettings;
import 'package:lexeapp/style.dart' show LxColors, Space;

class DebugPage extends StatelessWidget {
  const DebugPage({
    super.key,
    required this.config,
    required this.app,
    required this.settings,
  });

  final Config config;
  final AppHandle app;
  final LxSettings settings;

  Future<void> doDeleteLocalPaymentDb() async {
    info("Deleting local PaymentDb");

    (await Result.tryFfiAsync(this.app.deletePaymentDb))
        .inspectErr((err) => error(err.message));
  }

  void doDeleteSecretStore() {
    info("Deleting SecretStore");

    Result.tryFfi(() => debug.deleteSecretStore(config: this.config))
        .inspectErr((err) => error(err.message));
  }

  // void doDeleteLatestProvisionedFile() {
  //   info("Deleting latest_provisioned file");
  //   Result.tryFfi(() => debug.deleteLatestProvisioned(config: this.config))
  //       .inspectErr((err) => error(err.message));
  // }

  void doResetSettingsDb() {
    info("Resetting SettingsDb");
    this.settings.reset();
  }

  @override
  Widget build(BuildContext context) {
    const bodyPadding = EdgeInsets.symmetric(horizontal: Space.s600);

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        padding: bodyPadding,
        body: [
          const HeadingText(text: "Lexe Debug"),
          const SubheadingText(text: "Page for manipulating app internals."),
          const SizedBox(height: Space.s600),

          // Reset SettingsDb
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text("Reset settings"),
            subtitle: const Text.rich(TextSpan(children: [
              TextSpan(
                  text: "Resets all settings to their default values.",
                  style: TextStyle(color: LxColors.fgTertiary)),
            ])),
            onTap: this.doResetSettingsDb,
          ),

          // Delete PaymentDb
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text("Delete local payments"),
            subtitle: const Text(
              "Your app will clear all local payment info and resync from the node",
              style: TextStyle(color: LxColors.fgTertiary),
            ),
            onTap: this.doDeleteLocalPaymentDb,
          ),

          // // TODO(phlip9): actually delete latest_provisioned
          // ListTile(
          //   contentPadding: EdgeInsets.zero,
          //   title: const Text("Delete latest_provisioned file (TODO)"),
          //   subtitle: const Text(
          //     "On next restart, will ask the Lexe API for the most recent node "
          //     "version and unconditionally provision to it.",
          //     style: TextStyle(color: LxColors.fgTertiary),
          //   ),
          //   onTap: this.doDeleteLatestProvisionedFile,
          // ),

          // Delete SecretStore
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text("Delete local secrets"),
            subtitle: const Text.rich(TextSpan(children: [
              TextSpan(
                  text: "WARNING: ",
                  style: TextStyle(color: Color(0xffeb5d47))),
              TextSpan(
                  text:
                      "you will need to recover from backup to use this wallet again",
                  style: TextStyle(color: LxColors.fgTertiary)),
            ])),
            onTap: this.doDeleteSecretStore,
          ),
        ],
      ),
    );
  }
}
