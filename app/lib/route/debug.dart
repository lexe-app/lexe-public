/// # Lexe Debug Page
///
/// A page for manipulating app internals during development.

import 'package:flutter/material.dart';

import '../../bindings.dart' show api;
import '../../bindings_generated_api.dart' show AppHandle, Config;
import '../../components.dart'
    show HeadingText, LxCloseButton, ScrollableSinglePageBody, SubheadingText;
import '../../logger.dart' show error, info;
import '../../result.dart';
import '../../style.dart' show LxColors, Space;

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

    Result.tryFfi(() => api.debugDeleteSecretStore(config: this.config))
        .inspectErr((err) => error(err.message));
  }

  @override
  Widget build(BuildContext context) {
    const bodyPadding = EdgeInsets.symmetric(horizontal: Space.s600);

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(),
      ),
      body: ScrollableSinglePageBody(
        padding: EdgeInsets.zero,
        body: [
          const Padding(
            padding: bodyPadding,
            child: HeadingText(text: "Lexe Debug"),
          ),
          const Padding(
            padding: bodyPadding,
            child: SubheadingText(text: "Page for manipulating app internals."),
          ),
          const SizedBox(height: Space.s600),
          ListTile(
            contentPadding: bodyPadding,
            title: const Text("Delete local PaymentDb"),
            subtitle: const Text(
              "The PaymentDb will be recreated after the next payment sync",
              style: TextStyle(color: LxColors.fgTertiary),
            ),
            onTap: this.doDeleteLocalPaymentDb,
          ),
          ListTile(
            contentPadding: bodyPadding,
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
