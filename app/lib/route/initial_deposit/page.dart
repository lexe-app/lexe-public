/// Initial deposit onboarding flow.
library;

import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        HeadingText,
        LxCloseButton,
        LxCloseButtonKind,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/route/initial_deposit/state.dart' show DepositMethod;
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxRadius, Space;

/// Choose between Lightning or On-chain deposit method.
class InitialDepositChooseMethodPage extends StatelessWidget {
  const InitialDepositChooseMethodPage({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const SizedBox.shrink(),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Let's fund your wallet"),
          const SubheadingText(
            text: "Choose how you'd like to receive your first deposit.",
          ),

          const SizedBox(height: Space.s700),

          // Lightning option
          _MethodCard(
            icon: LxIcons.lightning,
            title: "Lightning",
            description:
                "Instant and low fees. "
                "Receiving funds will open a channel.",
            isPrimary: true,
            onTap: () =>
                this._onMethodSelected(context, DepositMethod.lightning),
          ),

          const SizedBox(height: Space.s400),

          // On-chain option
          _MethodCard(
            icon: LxIcons.bitcoin,
            title: "On-chain",
            description: "Send from any Bitcoin wallet. Slower, higher fees.",
            isPrimary: false,
            onTap: () => this._onMethodSelected(context, DepositMethod.onchain),
          ),
        ],
      ),
    );
  }

  void _onMethodSelected(BuildContext context, DepositMethod method) {
    // TODO(a-mpch): Navigate to appropriate next page based on method
    switch (method) {
      case DepositMethod.lightning:
        break;
      case DepositMethod.onchain:
        break;
    }
  }
}

/// A card widget for selecting a deposit method.
///
/// When [isPrimary] is true, uses an emphasized CTA-style appearance.
/// When false, uses a more subdued secondary style.
class _MethodCard extends StatelessWidget {
  const _MethodCard({
    required this.icon,
    required this.title,
    required this.description,
    required this.isPrimary,
    required this.onTap,
  });

  final IconData icon;
  final String title;
  final String description;
  final bool isPrimary;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    // Primary (Lightning): emphasized with border and icon highlight
    // Secondary (On-chain): subdued appearance
    final borderColor = this.isPrimary
        ? LxColors.foreground
        : Colors.transparent;
    final iconBgColor = this.isPrimary
        ? LxColors.foreground
        : LxColors.fgTertiary.withValues(alpha: 0.1);
    final iconColor = this.isPrimary
        ? LxColors.background
        : LxColors.fgSecondary;

    return Material(
      color: LxColors.grey1000,
      borderRadius: BorderRadius.circular(LxRadius.r400),
      child: InkWell(
        onTap: this.onTap,
        borderRadius: BorderRadius.circular(LxRadius.r400),
        child: Container(
          decoration: BoxDecoration(
            border: Border.all(color: borderColor, width: 1.5),
            borderRadius: BorderRadius.circular(LxRadius.r400),
          ),
          padding: const EdgeInsets.all(Space.s400),
          child: Row(
            children: [
              // Icon container
              Container(
                width: 48,
                height: 48,
                decoration: BoxDecoration(
                  color: iconBgColor,
                  borderRadius: BorderRadius.circular(LxRadius.r300),
                ),
                child: Icon(this.icon, size: 24, color: iconColor),
              ),

              const SizedBox(width: Space.s400),

              // Title and description
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      this.title,
                      style: const TextStyle(
                        fontSize: Fonts.size400,
                        fontVariations: [Fonts.weightSemiBold],
                        color: LxColors.foreground,
                      ),
                    ),
                    const SizedBox(height: Space.s100),
                    Text(
                      this.description,
                      style: const TextStyle(
                        fontSize: Fonts.size200,
                        color: LxColors.fgSecondary,
                        height: 1.3,
                      ),
                    ),
                  ],
                ),
              ),

              const SizedBox(width: Space.s200),

              // Arrow indicator
              const Icon(LxIcons.next, size: 20, color: LxColors.fgTertiary),
            ],
          ),
        ),
      ),
    );
  }
}
