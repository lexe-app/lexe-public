// An alternate application entrypoint specifically for designing pages
// and components in isolation, without actually touching any real backends.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:intl/intl.dart' show Intl;

import 'bindings.dart' show api;
import 'bindings_generated_api.dart'
    show
        App,
        AppHandle,
        AppRs,
        BasicPayment,
        Config,
        FiatRate,
        FiatRates,
        NodeInfo,
        PaymentDirection,
        PaymentKind,
        PaymentStatus;
import 'cfg.dart' as cfg;
import 'logger.dart' as logger;
import 'logger.dart' show info;
import 'route/backup_wallet.dart' show BackupWalletPage;
import 'route/landing.dart' show LandingPage;
import 'route/wallet.dart' show DrawerListItem, WalletPage;
import 'style.dart' show LxColors, LxTheme, Space;

Future<void> main() async {
  Intl.defaultLocale = "en_US";

  logger.init();

  final Config config = await cfg.buildTest();
  info("Test build config: $config");

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
          Component("LandingPage", (_) => LandingPage(config: config)),
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

  // Some sample data
  List<BasicPayment> payments = [
    const BasicPayment(
      index:
          "0000001687090000000-bc_551df4ef3b67b3f2ca53f3e668eb73c2a9b3a77dea84b340fd2407ec5542aa66",
      id: "bc_551df4ef3b67b3f2ca53f3e668eb73c2a9b3a77dea84b340fd2407ec5542aa66",
      kind: PaymentKind.Onchain,
      direction: PaymentDirection.Inbound,
      amountSat: 20000,
      status: PaymentStatus.Completed,
      statusStr: "completed",
      // note: "tb1qhlqcmf383f9zddmvc36ngwxjlffgtk5ldrrsav",
      createdAt: 1670090392000,
    ),
    const BasicPayment(
      index:
          "0000001687120000000-bc_46e52089b60b00de067c84ce58d34a75ffd71a106f720855bc099f20da11700c",
      id: "bc_46e52089b60b00de067c84ce58d34a75ffd71a106f720855bc099f20da11700c",
      kind: PaymentKind.Onchain,
      direction: PaymentDirection.Outbound,
      amountSat: 95000000,
      status: PaymentStatus.Failed,
      statusStr: "dropped",
      note: "Sweep from Muun",
      createdAt: 1671818392000,
    ),
    const BasicPayment(
      index:
          "0000001687130000000-ln_6973b3c58738403ceb3fccec470365a44361f34f4c2664ccae04f0f39fe71dc0",
      id: "ln_6973b3c58738403ceb3fccec470365a44361f34f4c2664ccae04f0f39fe71dc0",
      kind: PaymentKind.Spontaneous,
      direction: PaymentDirection.Outbound,
      amountSat: 123000,
      status: PaymentStatus.Pending,
      statusStr: "invoice generated",
      note: "🍑🍑🍑🍆🍆🍆😂😂😂",
      createdAt: 1686938392000,
    ),
    const BasicPayment(
      index:
          "0000001687150000000-ln_6f9dad93ceb2e78181ef5cb73601a28930e9774204d6fb335297b1f4add83d30",
      id: "ln_6f9dad93ceb2e78181ef5cb73601a28930e9774204d6fb335297b1f4add83d30",
      kind: PaymentKind.Invoice,
      direction: PaymentDirection.Inbound,
      amountSat: 4470000,
      status: PaymentStatus.Pending,
      statusStr: "pending",
      note:
          "My super long note that really is too long it just keeps going and going",
      createdAt: 1687150000000,
    ),
    const BasicPayment(
      index:
          "0000001687200000000-ln_6fc9375017dd3d911fe4ee52f4becd2f376384f42053381a09c99cca61dbf87a",
      id: "ln_6fc9375017dd3d911fe4ee52f4becd2f376384f42053381a09c99cca61dbf87a",
      kind: PaymentKind.Invoice,
      direction: PaymentDirection.Inbound,
      amountSat: 222000,
      status: PaymentStatus.Completed,
      statusStr: "completed",
      createdAt: 1687200000000,
    ),
    const BasicPayment(
      index:
          "0000001687309696000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68",
      id: "bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68",
      kind: PaymentKind.Onchain,
      direction: PaymentDirection.Outbound,
      amountSat: 77000,
      status: PaymentStatus.Completed,
      statusStr: "completed",
      note: "Brunch w/ friends",
      createdAt: 1687385080000,
    ),
  ];

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

  @override
  Future<bool> syncPayments({dynamic hint}) => Future.delayed(
        const Duration(milliseconds: 1500),
        () => true,
      );

  @override
  BasicPayment? getPaymentByScrollIdx({
    required int scrollIdx,
    dynamic hint,
  }) {
    if (scrollIdx < this.payments.length) {
      return this.payments[this.payments.length - scrollIdx - 1];
    } else {
      return null;
    }
  }

  @override
  int getNumPayments({dynamic hint}) => this.payments.length;
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
