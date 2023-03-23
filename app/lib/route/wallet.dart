// The primary wallet page.

import 'package:flutter/material.dart';

import '../../bindings_generated_api.dart' show AppHandle;
import '../../style.dart' show Fonts, LxColors, Space;

class WalletPage extends StatelessWidget {
  const WalletPage({super.key, required this.app});

  final AppHandle app;

  @override
  Widget build(BuildContext context) {
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
      drawer: const WalletDrawer(),
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

class WalletDrawer extends StatelessWidget {
  const WalletDrawer({super.key});

  @override
  Widget build(BuildContext context) {
    final systemBarHeight = MediaQuery.of(context).padding.top;

    return Drawer(
      child: Padding(
        padding: EdgeInsets.only(top: systemBarHeight),
        child: ListView(
          padding: EdgeInsets.zero,
          children: [
            // X - close
            DrawerListItem(
              icon: Icons.close_rounded,
              onTap: () => Scaffold.of(context).closeDrawer(),
            ),
            const SizedBox(height: Space.s600),

            // * Settings
            // * Backup
            // * Security
            // * Support
            DrawerListItem(
              title: "Settings",
              icon: Icons.settings_outlined,
              onTap: () => debugPrint("settings pressed"),
            ),
            DrawerListItem(
              title: "Backup",
              icon: Icons.drive_file_move_outline,
              onTap: () => debugPrint("backup pressed"),
            ),
            DrawerListItem(
              title: "Security",
              icon: Icons.lock_outline_rounded,
              onTap: () => debugPrint("security pressed"),
            ),
            DrawerListItem(
              title: "Support",
              icon: Icons.help_outline_rounded,
              onTap: () => debugPrint("support pressed"),
            ),
            const SizedBox(height: Space.s600),

            // < Invite Friends >
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: Space.s500),
              child: OutlinedButton(
                style: OutlinedButton.styleFrom(
                  backgroundColor: LxColors.background,
                  foregroundColor: LxColors.foreground,
                  side:
                      const BorderSide(color: LxColors.foreground, width: 2.0),
                  padding: const EdgeInsets.symmetric(vertical: Space.s500),
                ),
                onPressed: () => debugPrint("invite pressed"),
                child: Text("Invite Friends",
                    style: Fonts.fontUI.copyWith(
                      fontSize: Fonts.size400,
                      fontVariations: [Fonts.weightMedium],
                    )),
              ),
            ),
            const SizedBox(height: Space.s600),

            // app version
            Text("Lexe App · v1.2.345",
                textAlign: TextAlign.center,
                style: Fonts.fontUI.copyWith(
                  color: LxColors.grey600,
                  fontSize: Fonts.size200,
                )),
          ],
        ),
      ),
    );
  }
}

class DrawerListItem extends StatelessWidget {
  const DrawerListItem({super.key, this.title, this.icon, this.onTap});

  final String? title;
  // final String? subtitle;
  final IconData? icon;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: Space.s500),
      horizontalTitleGap: Space.s200,
      visualDensity: VisualDensity.standard,
      dense: false,
      leading: (this.icon != null)
          ? Icon(this.icon!, color: LxColors.foreground, size: Fonts.size700)
          : null,
      title: (this.title != null)
          ? Text(this.title!,
              style: Fonts.fontUI.copyWith(
                fontSize: Fonts.size400,
                fontVariations: [Fonts.weightMedium],
              ))
          : null,
      // subtitle: (this.subtitle != null)
      //     ? Text(this.subtitle!,
      //         style: Fonts.fontUI
      //             .copyWith(fontSize: Fonts.size300, color: LxColors.grey600))
      //     : null,
      onTap: this.onTap,
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
        const SizedBox(height: Space.s400),
        Text(
          "73,187 SATS",
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
          "\$15",
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size800,
            fontVariations: [Fonts.weightMedium],
          ),
        ),
        Text(
          ".21",
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
        const SizedBox(width: Space.s400),
        WalletActionButton(
          onPressed: () => debugPrint("recv pressed"),
          icon: Icons.arrow_downward_rounded,
          label: "Receive",
        ),
        const SizedBox(width: Space.s400),
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
            backgroundColor: LxColors.grey975,
            disabledBackgroundColor: LxColors.grey850,
            foregroundColor: LxColors.foreground,
            disabledForegroundColor: LxColors.grey725,
          ),
          child: Padding(
            padding: const EdgeInsets.all(Space.s400),
            child: Icon(this.icon, size: Fonts.size700),
          ),
        ),
        const SizedBox(height: Space.s400),
        Text(
          label,
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size300,
            color: (!isDisabled) ? LxColors.foreground : LxColors.grey725,
            fontVariations: [Fonts.weightSemiBold],
          ),
        ),
      ],
    );
  }
}
