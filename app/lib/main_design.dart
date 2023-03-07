// An alternate application entrypoint specifically for designing pages
// and components in isolation, without actually touching any real backends.

import 'package:flutter/material.dart';

import 'route/landing.dart' show LandingPage;
// import 'route/backup_wallet.dart' show BackupWalletPage;
import 'route/wallet.dart' show WalletPage;

import 'style.dart' show Fonts, LxColors, Space;

Future<void> main() async {
  runApp(MaterialApp(
    title: 'Lexe',
    themeMode: ThemeMode.light,
    home: ComponentList(components: [
      Component("LandingPage", (_) => const LandingPage()),
      // TODO(phlip9): figure out mocking
      Component("BackupWalletPage", (_) => const SizedBox()),
      Component("WalletPage", (_) => const WalletPage()),
    ]),
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
    return Container(
      color: LxColors.background,
      padding: const EdgeInsets.only(top: Space.s700),
      child: ListView.builder(
          padding: const EdgeInsets.all(Space.s400),
          itemCount: components.length,
          itemBuilder: (BuildContext context, int index) {
            final component = components[index];

            return GestureDetector(
              onTap: () {
                Navigator.of(context).push(MaterialPageRoute(
                  maintainState: false,
                  builder: component.builder,
                ));
              },
              child: SizedBox(
                height: Space.s700,
                child: Align(
                  alignment: Alignment.centerLeft,
                  child: Text(
                    component.name,
                    style: Fonts.fontBody,
                  ),
                ),
              ),
            );
          }),
    );
  }
}
