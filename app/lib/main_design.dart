// An alternate application entrypoint specifically for designing pages
// and components in isolation, without actually touching any real backends.

import 'package:flutter/material.dart';

import 'route/landing.dart' show LandingPage;
// import 'route/backup_wallet.dart' show BackupWalletPage;
import 'route/wallet.dart' show DrawerListItem, WalletPage;

import 'style.dart' show LxColors, LxTheme, Space;

Future<void> main() async {
  runApp(MaterialApp(
    title: "Lexe App - Design Mode",
    color: LxColors.background,
    themeMode: ThemeMode.light,
    theme: LxTheme.light(),
    debugShowCheckedModeBanner: false,
    home: Scaffold(
      body: ComponentList(
        components: [
          Component("LandingPage", (_) => const LandingPage()),
          // TODO(phlip9): figure out mocking
          Component("BackupWalletPage", (_) => const SizedBox()),
          Component("WalletPage", (_) => const WalletPage()),
        ],
      ),
    ),
  ));
}

class Component {
  const Component(this.name, this.builder);

  final String name;
  final WidgetBuilder builder;
}

class ComponentList extends StatelessWidget {
  const ComponentList({super.key, required this.components});

  final List<Component> components;

  @override
  Widget build(BuildContext context) {
    final systemBarHeight = MediaQuery.of(context).padding.top;

    return Container(
      color: LxColors.background,
      padding: EdgeInsets.only(top: systemBarHeight + Space.s400),
      child: ListView.builder(
        padding: const EdgeInsets.symmetric(horizontal: Space.s500),
        itemCount: this.components.length,
        itemBuilder: (BuildContext context, int index) {
          final component = this.components[index];

          return DrawerListItem(
            title: component.name,
            onTap: () {
              Navigator.of(context).push(MaterialPageRoute(
                maintainState: false,
                builder: component.builder,
              ));
            },
          );
        },
      ),
    );
  }
}
