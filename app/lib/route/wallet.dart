// The primary wallet page.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../style.dart' show Fonts, LxColors, Space;

class WalletPage extends StatelessWidget {
  const WalletPage({super.key});

  @override
  Widget build(BuildContext context) {
    final balance_sats = 32000;

    return Scaffold(
      backgroundColor: LxColors.background,
      appBar: AppBar(
        backgroundColor: LxColors.background,
        foregroundColor: LxColors.grey50,
        elevation: 0.0,
        systemOverlayStyle: SystemUiOverlayStyle.dark.copyWith(
          statusBarColor: LxColors.background,
        ),
      ),
      drawer: Drawer(
        child: ListView(
          padding: EdgeInsets.zero,
          children: [
            const DrawerHeader(
              child: Text("DrawerHeader", style: Fonts.fontUI),
            ),
            ListTile(
              title: const Text("ListTile", style: Fonts.fontUI),
              onTap: () => debugPrint("tapped drawer item"),
            ),
          ],
        ),
      ),
      body: Center(
        child: ListView(
          children: [
            const SizedBox(height: Space.s1000),
            const BalanceWidget(balanceSats: 32000),
          ],
        ),
      ),
    );
  }
}

class BalanceWidget extends StatelessWidget {
  const BalanceWidget({
    super.key,
    this.balanceSats,
  });

  final int? balanceSats;

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Text(
          "\$123.45",
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size800,
          ),
        ),
        const SizedBox(height: Space.s500),
        Text(
          "$balanceSats SATS",
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size500,
          ),
        ),
      ],
    );
  }
}
