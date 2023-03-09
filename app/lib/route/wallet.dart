// The primary wallet page.

import 'package:flutter/material.dart';

import '../../style.dart' show Fonts, LxColors, Space;

class WalletPage extends StatelessWidget {
  const WalletPage({super.key});

  @override
  Widget build(BuildContext context) {
    final systemBarHeight = MediaQuery.of(context).padding.top;

    return Scaffold(
      appBar: AppBar(
        automaticallyImplyLeading: false,
        leading: Builder(
          builder: (context) => IconButton(
            iconSize: Fonts.size700,
            icon: const Icon(Icons.menu_rounded),
            onPressed: () => Scaffold.of(context).openDrawer(),
          ),
        ),
      ),
      drawer: Drawer(
        child: Padding(
          padding: EdgeInsets.only(top: systemBarHeight),
          child: ListView(
            padding: EdgeInsets.zero,
            children: [
              // X drawer close
              Builder(
                builder: (context) => Align(
                  alignment: Alignment.centerLeft,
                  child: IconButton(
                    iconSize: Fonts.size700,
                    icon: const Icon(Icons.close_rounded),
                    onPressed: () => Scaffold.of(context).closeDrawer(),
                  ),
                ),
              ),
              // ListTile(
              //   title: const Text("ListTile", style: Fonts.fontUI),
              //   onTap: () => debugPrint("tapped drawer item"),
              // ),
              // ListTile(
              //   title: const Text("ListTile", style: Fonts.fontUI),
              //   onTap: () => debugPrint("tapped drawer item"),
              // ),
            ],
          ),
        ),
      ),
      body: ListView(
        children: const [
          SizedBox(height: Space.s1000),
          BalanceWidget(),
          SizedBox(height: Space.s700),
          WalletActions(),
        ],
      ),
    );
  }
}

class BalanceWidget extends StatelessWidget {
  const BalanceWidget({super.key});

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        const PrimaryBalanceText(),
        const SizedBox(height: Space.s500),
        Text(
          "32,000 SATS",
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size300,
            color: LxColors.grey700,
            fontVariations: [Fonts.weightMedium],
          ),
        ),
      ],
    );
  }
}

class PrimaryBalanceText extends StatelessWidget {
  const PrimaryBalanceText({super.key});

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        Text(
          "\$123",
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size800,
            fontVariations: [Fonts.weightMedium],
          ),
        ),
        Text(
          ".45",
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size800,
            color: LxColors.grey650,
            fontVariations: [Fonts.weightMedium],
          ),
        ),
      ],
    );
  }
}

class WalletActions extends StatelessWidget {
  const WalletActions({super.key});

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        const WalletActionButton(
          onPressed: null,
          icon: Icons.add_rounded,
          label: "Fund",
        ),
        const SizedBox(width: Space.s500),
        WalletActionButton(
          onPressed: () => debugPrint("recv pressed"),
          icon: Icons.arrow_downward_rounded,
          label: "Receive",
        ),
        const SizedBox(width: Space.s500),
        WalletActionButton(
          onPressed: () => debugPrint("send pressed"),
          icon: Icons.arrow_upward_rounded,
          label: "Send",
        ),
      ],
    );
  }
}

class WalletActionButton extends StatelessWidget {
  const WalletActionButton({
    super.key,
    required this.onPressed,
    required this.icon,
    required this.label,
  });

  final VoidCallback? onPressed;
  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    final bool isDisabled = (this.onPressed == null);

    return Column(
      children: [
        FilledButton(
          onPressed: this.onPressed,
          style: FilledButton.styleFrom(
            backgroundColor: LxColors.grey1000,
            disabledBackgroundColor: LxColors.grey875,
            foregroundColor: LxColors.grey150,
            disabledForegroundColor: LxColors.grey725,
          ),
          child: Padding(
            padding: const EdgeInsets.all(Space.s500),
            child: Icon(this.icon, size: Fonts.size700),
          ),
        ),
        const SizedBox(height: Space.s500),
        Text(
          label,
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size300,
            color: (!isDisabled) ? LxColors.grey150 : LxColors.grey725,
            fontVariations: [Fonts.weightSemiBold],
          ),
        ),
      ],
    );
  }
}
