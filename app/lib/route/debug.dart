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
import 'package:lexeapp/style.dart' show LxColors, Space;

class DebugPage extends StatelessWidget {
  const DebugPage({
    super.key,
    required this.config,
    required this.app,
  });

  final Config config;
  final AppHandle app;

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

  void doDeleteLatestProvisionedFile() {
    info("Deleting latest_provisioned file");
    Result.tryFfi(() => debug.deleteLatestProvisioned(config: this.config))
        .inspectErr((err) => error(err.message));
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
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text("Delete local PaymentDb"),
            subtitle: const Text(
              "The PaymentDb will be recreated after the next payment sync",
              style: TextStyle(color: LxColors.fgTertiary),
            ),
            onTap: this.doDeleteLocalPaymentDb,
          ),
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text("Delete latest_provisioned file"),
            subtitle: const Text(
              "On next restart, will ask the Lexe API for the most recent node "
              "version and unconditionally provision to it.",
              style: TextStyle(color: LxColors.fgTertiary),
            ),
            onTap: this.doDeleteLatestProvisionedFile,
          ),
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text("Delete SecretStore & RootSeed"),
            subtitle: const Text.rich(TextSpan(children: [
              TextSpan(
                  text: "WARNING: ",
                  style: TextStyle(color: Color(0xffeb5d47))),
              TextSpan(
                  text:
                      "you will need a backup recovery to use the account afterwards",
                  style: TextStyle(color: LxColors.fgTertiary)),
            ])),
            onTap: this.doDeleteSecretStore,
          ),
        ],
      ),
    );
  }
}
