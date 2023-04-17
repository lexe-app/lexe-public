// An alternate application entrypoint specifically for designing pages
// and components in isolation, without actually touching any real backends.

import 'package:flutter/material.dart';
import 'package:intl/intl.dart' show Intl;

import 'bindings.dart' show api;
import 'bindings_generated_api.dart'
    show App, AppHandle, AppRs, FiatRate, FiatRates, NodeInfo;
import 'route/backup_wallet.dart' show BackupWalletPage;
import 'route/landing.dart' show LandingPage;
import 'route/wallet.dart' show DrawerListItem, WalletPage;
import 'style.dart' show LxColors, LxTheme, Space;

Future<void> main() async {
  Intl.defaultLocale = "en_US";

  final mockApp = MockAppHandle(bridge: api);

  runApp(MaterialApp(
    title: "Lexe App - Design Mode",
    color: LxColors.background,
    themeMode: ThemeMode.light,
    theme: LxTheme.light(),
    debugShowCheckedModeBanner: false,
    home: Scaffold(
      appBar: AppBar(automaticallyImplyLeading: false),
      body: ComponentList(
        components: [
          Component("LandingPage", (_) => const LandingPage()),
          Component("BackupWalletPage", (_) => BackupWalletPage(app: mockApp)),
          Component("WalletPage", (_) => WalletPage(app: mockApp)),
        ],
      ),
    ),
  ));
}

// TODO(phlip9): add a `App::mock` constructor?
class MockApp extends App {
  // This makes a fake `RustOpaque<App>` w/ a null pointer. Super hacky, but frb
  // will at least panic if we accidentally call a native method.
  MockApp(AppRs bridge) : super.fromRaw(0x0, 0, bridge);
}

class MockAppHandle extends AppHandle {
  MockAppHandle({required AppRs bridge})
      : super(bridge: bridge, inner: MockApp(bridge));

  @override
  Future<NodeInfo> nodeInfo({dynamic hint}) => Future.delayed(
        const Duration(milliseconds: 1000),
        () => const NodeInfo(nodePk: "asdf", localBalanceMsat: 727505000),
      );

  @override
  Future<FiatRates> fiatRates({dynamic hint}) => Future.delayed(
        const Duration(milliseconds: 1300),
        () => const FiatRates(
          timestampMs: 1679863795,
          rates: [
            FiatRate(fiat: "USD", rate: 30296.1951578664 /* USD/BTC */),
          ],
        ),
      );
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

    return Padding(
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
