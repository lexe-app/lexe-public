import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show HeadingText, LxBackButton, ScrollableSinglePageBody, SubheadingText;
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;

/// Basic security page that leads to displa SeedPhrase, connect GDrive or
/// test GDrive connection.
class SecurityPage extends StatefulWidget {
  const SecurityPage({super.key, required this.app});

  final AppHandle app;

  @override
  State<SecurityPage> createState() => _SecurityPageState();
}

class _SecurityPageState extends State<SecurityPage> {
  void onViewSeedPhraseTap() {}

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
