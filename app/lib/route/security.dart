import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/secret_store.dart' show SecretStore;
import 'package:app_rs_dart/ffi/types.dart';
import 'package:flutter/material.dart';
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        HeadingText,
        LxBackButton,
        LxFilledButton,
        ScrollableSinglePageBody,
        SeedWordsCard,
        SubheadingText;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart' show Err, Ok, Result;
import 'package:lexeapp/route/send/page.dart' show StackedButton;
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;

/// Basic security page that leads to displa SeedPhrase, connect GDrive or
/// test GDrive connection.
class SecurityPage extends StatefulWidget {
  const SecurityPage({super.key, required this.config});

  final Config config;

  @override
  State<SecurityPage> createState() => _SecurityPageState();
}

class _SecurityPageState extends State<SecurityPage> {
  Result<List<String>, String> getSeedPhrase() {
    final secretStore = SecretStore(config: this.widget.config);
    final result = Result.tryFfi(secretStore.readRootSeed);
    final RootSeed? rootSeed;
    switch (result) {
      case Ok(:final ok):
        rootSeed = ok;
      case Err(:final err):
        return Err("$err");
    }

    if (rootSeed == null) return const Err("Could not open secret store");

    return Ok(rootSeed.seedPhrase());
  }

  void onViewSeedPhraseTap() {
    final seedPhraseResult = this.getSeedPhrase();
    final List<String> seedPhrase;
    switch (seedPhraseResult) {
      case Ok(:final ok):
        seedPhrase = ok;
      case Err(:final err):
        warn(err);
        return;
    }

    Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (context) => SeedPhrasePage(seedPhrase: seedPhrase),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    const cardPad = Space.s300;
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        padding: const EdgeInsets.symmetric(horizontal: Space.s600 - cardPad),
        body: [
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: cardPad),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                HeadingText(text: "Node Security"),
                SubheadingText(
                  text: "Backup your node and test your security backups.",
                ),
                SizedBox(height: Space.s500),
              ],
            ),
          ),

          InfoCardWithSubtitle(
            title: const Text(
              "Seed phrase",
              style: TextStyle(
                color: LxColors.fgTertiary,
                fontSize: Fonts.size200,
                fontVariations: [Fonts.weightMedium],
              ),
            ),
            subtitle: const Text.rich(
              TextSpan(
                style: TextStyle(
                  fontVariations: [Fonts.weightNormal],
                  fontSize: Fonts.size200,
                ),
                children: [
                  TextSpan(
                    text: "WARNING: ",
                    style: TextStyle(color: Color(0xffeb5d47)),
                  ),
                  TextSpan(
                    text:
                        "This is the root seed for your wallet. Anyone "
                        "with this secret also controls your funds.",
                    style: TextStyle(color: LxColors.fgTertiary),
                  ),
                ],
              ),
            ),
            children: [
              NodeSecurityButton(
                label: "View seed phrase",
                onTap: this.onViewSeedPhraseTap,
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class InfoCardWithSubtitle extends StatelessWidget {
  const InfoCardWithSubtitle({
    super.key,
    required this.children,
    required this.title,
    required this.subtitle,
    this.bodyPadding = Space.s300,
  });

  final Text title;
  final Text subtitle;
  final List<Widget> children;
  final double bodyPadding;

  @override
  Widget build(BuildContext context) {
    final section = Card(
      color: LxColors.grey1000,
      elevation: 0.0,
      margin: const EdgeInsets.all(0),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: Space.s300 / 2),
        child: Column(children: this.children),
      ),
    );

    const intraCardSpace = Space.s200;

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: intraCardSpace),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: EdgeInsets.only(
              left: this.bodyPadding,
              bottom: Space.s200,
            ),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.start,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                this.title,
                const SizedBox(height: Space.s200),
                this.subtitle,
              ],
            ),
          ),
          section,
        ],
      ),
    );
  }
}

class NodeSecurityButton extends StatelessWidget {
  const NodeSecurityButton({
    super.key,
    required this.onTap,
    required this.label,
  });

  final VoidCallback? onTap;
  final String label;

  @override
  Widget build(BuildContext context) {
    final bool isDisabled = (this.onTap == null);
    final Color color = (!isDisabled)
        ? LxColors.fgSecondary
        : LxColors.fgTertiary;

    return InkWell(
      onTap: this.onTap,
      child: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          const Expanded(child: SizedBox()),
          Padding(
            padding: const EdgeInsets.symmetric(
              horizontal: Space.s450,
              vertical: Space.s200,
            ),
            child: Text(
              this.label,
              style: Fonts.fontUI.copyWith(
                fontSize: Fonts.size200,
                color: color,
                fontVariations: [Fonts.weightNormal],
              ),
            ),
          ),
          Expanded(
            child: Align(
              alignment: Alignment.centerRight,
              child: Padding(
                padding: const EdgeInsets.only(right: Space.s300),
                child: Icon(LxIcons.next, size: Fonts.size100, color: color),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class SeedPhrasePage extends StatefulWidget {
  const SeedPhrasePage({super.key, required this.seedPhrase});

  final List<String> seedPhrase;

  @override
  State<SeedPhrasePage> createState() => _SeedPhrasePageState();
}

class _SeedPhrasePageState extends State<SeedPhrasePage> {
  /// Whether the user has tapped the "switch" tile to confirm they've backed
  /// up their seed phrase.
  final ValueNotifier<bool> isConfirmed = ValueNotifier(false);

  @override
  void dispose() {
    this.isConfirmed.dispose();
    super.dispose();
  }

  void onConfirm(bool value) {
    this.isConfirmed.value = value;
  }

  void onSubmit() {
    Navigator.of(this.context).pop();
  }

  void onCopy() {
    final words = this.widget.seedPhrase.indexed
        .map((x) => "${x.$1 + 1}. ${x.$2}")
        .join(" ");
    unawaited(LxClipboard.copyTextWithFeedback(this.context, words));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(leading: null, automaticallyImplyLeading: false),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Backup seed phrase"),
          const SubheadingText(
            text: "Store this in a safe place, like a password manager.",
          ),
          const SizedBox(height: Space.s600),
          Align(
            alignment: Alignment.center,
            child: SeedWordsCard(seedWords: this.widget.seedPhrase),
          ),
          const SizedBox(height: Space.s500),
          ValueListenableBuilder(
            valueListenable: this.isConfirmed,
            builder: (context, isConfirmed, child) {
              return SwitchListTile(
                value: isConfirmed,
                // Disable switch while signing up
                onChanged: this.onConfirm,
                title: const Text(
                  "I have backed up my seed phrase. I understand my funds cannot be recovered if I lose the seed phrase.",
                  style: TextStyle(fontSize: Fonts.size200, height: 1.4),
                ),
                contentPadding: EdgeInsets.zero,
                inactiveTrackColor: LxColors.grey1000,
                activeTrackColor: LxColors.moneyGoUp,
                inactiveThumbColor: LxColors.grey850,
                controlAffinity: ListTileControlAffinity.leading,
              );
            },
          ),
        ],

        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s300, bottom: Space.s200),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              Row(
                children: [
                  // Copy
                  Expanded(
                    child: GestureDetector(
                      onTap: this.onCopy,
                      child: StackedButton(
                        button: LxFilledButton(
                          onTap: this.onCopy,
                          icon: const Center(child: Icon(LxIcons.copy)),
                        ),
                        label: "Copy",
                      ),
                    ),
                  ),
                  const SizedBox(width: Space.s200),
                  // Sign up ->
                  Expanded(
                    child: ValueListenableBuilder(
                      valueListenable: this.isConfirmed,
                      builder: (_context, isConfirmed, _widget) {
                        final isEnabled = isConfirmed;

                        return GestureDetector(
                          onTap: isEnabled ? this.onSubmit : null,
                          child: StackedButton(
                            button: LxFilledButton(
                              label: const Icon(LxIcons.back),
                              icon: const Center(),
                              onTap: isEnabled ? this.onSubmit : null,
                            ),
                            label: "Go Back",
                          ),
                        );
                      },
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}
