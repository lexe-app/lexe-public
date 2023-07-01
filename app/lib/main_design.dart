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
        Config,
        FiatRate,
        FiatRates,
        NodeInfo,
        PaymentDirection,
        PaymentKind,
        PaymentStatus,
        ShortPayment;
import 'cfg.dart' as cfg;
import 'date_format.dart' as date_format;
import 'logger.dart' as logger;
import 'logger.dart' show info;
import 'route/backup_wallet.dart' show BackupWalletPage;
import 'route/landing.dart' show LandingPage;
import 'route/scan.dart' show ScanPage;
import 'route/show_qr.dart' show ShowQrPage;
import 'route/wallet.dart' show DrawerListItem, WalletPage;
import 'style.dart' show LxColors, LxTheme, Space;

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // Initialize date formatting locale data for ALL locales.
  await date_format.initializeDateLocaleData();

  // Uncomment one to try designs with a different locale:
  Intl.defaultLocale = "en_US"; // English - USA
  // Intl.defaultLocale = "ar_EG"; // Arabic - Egypt
  // Intl.defaultLocale = "fr_FR"; // French - France
  // Intl.defaultLocale = "nb"; // Norwegian Bokmål

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
          Component("ScanPage", (_) => const ScanPage()),
          Component(
            "ShowQrPage (standard bip21)",
            (_) => const ShowQrPage(
              value:
                  "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday",
            ),
          ),
          Component(
            "ShowQrPage (unified bolt 12)",
            (_) => const ShowQrPage(
              value:
                  "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday&lightning=LNBC10U1P3PJ257PP5YZTKWJCZ5FTL5LAXKAV23ZMZEKAW37ZK6KMV80PK4XAEV5QHTZ7QDPDWD3XGER9WD5KWM36YPRX7U3QD36KUCMGYP282ETNV3SHJCQZPGXQYZ5VQSP5USYC4LK9CHSFP53KVCNVQ456GANH60D89REYKDNGSMTJ6YW3NHVQ9QYYSSQJCEWM5CJWZ4A6RFJX77C490YCED6PEMK0UPKXHY89CMM7SCT66K8GNEANWYKZGDRWRFJE69H9U5U0W57RRCSYSAS7GADWMZXC8C6T0SPJAZUP6",
            ),
          ),
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
  List<ShortPayment> shortPayments = [
    const ShortPayment(
      index:
          "0000001687090000000-bc_551df4ef3b67b3f2ca53f3e668eb73c2a9b3a77dea84b340fd2407ec5542aa66",
      kind: PaymentKind.Onchain,
      direction: PaymentDirection.Inbound,
      amountSat: 20000,
      status: PaymentStatus.Completed,
      // note: "tb1qhlqcmf383f9zddmvc36ngwxjlffgtk5ldrrsav",
      createdAt: 1670090392000,
    ),
    const ShortPayment(
      index:
          "0000001687120000000-bc_46e52089b60b00de067c84ce58d34a75ffd71a106f720855bc099f20da11700c",
      kind: PaymentKind.Onchain,
      direction: PaymentDirection.Outbound,
      amountSat: 95000000,
      status: PaymentStatus.Failed,
      note: "Sweep from Muun",
      createdAt: 1671818392000,
    ),
    const ShortPayment(
      index:
          "0000001687130000000-ln_6973b3c58738403ceb3fccec470365a44361f34f4c2664ccae04f0f39fe71dc0",
      kind: PaymentKind.Spontaneous,
      direction: PaymentDirection.Outbound,
      amountSat: 123000,
      status: PaymentStatus.Pending,
      note: "🍑🍑🍑🍆🍆🍆😂😂😂",
      createdAt: 1686938392000,
    ),
    const ShortPayment(
      index:
          "0000001687150000000-ln_6f9dad93ceb2e78181ef5cb73601a28930e9774204d6fb335297b1f4add83d30",
      kind: PaymentKind.Invoice,
      direction: PaymentDirection.Inbound,
      amountSat: 4470000,
      status: PaymentStatus.Pending,
      note:
          "My super long note that really is too long it just keeps going and going",
      createdAt: 1687150000000,
    ),
    const ShortPayment(
      index:
          "0000001687200000000-ln_6fc9375017dd3d911fe4ee52f4becd2f376384f42053381a09c99cca61dbf87a",
      kind: PaymentKind.Invoice,
      direction: PaymentDirection.Inbound,
      amountSat: 222000,
      status: PaymentStatus.Completed,
      createdAt: 1687200000000,
    ),
    const ShortPayment(
      index:
          "0000001687309696000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68",
      kind: PaymentKind.Onchain,
      direction: PaymentDirection.Outbound,
      amountSat: 77000,
      status: PaymentStatus.Completed,
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
  ShortPayment? getPaymentByScrollIdx({required int scrollIdx, dynamic hint}) {
    if (scrollIdx >= this.shortPayments.length) {
      return null;
    }
    return this.shortPayments[this.shortPayments.length - scrollIdx - 1];
  }

  @override
  ShortPayment? getPendingPaymentByScrollIdx(
      {required int scrollIdx, dynamic hint}) {
    if (scrollIdx >= this.getNumPendingPayments()) {
      return null;
    }
    return this
        .shortPayments
        .reversed
        .where((payment) => payment.status == PaymentStatus.Pending)
        .elementAt(scrollIdx);
  }

  @override
  ShortPayment? getFinalizedPaymentByScrollIdx(
      {required int scrollIdx, dynamic hint}) {
    if (scrollIdx >= this.getNumFinalizedPayments()) {
      return null;
    }
    return this
        .shortPayments
        .reversed
        .where((payment) => payment.status != PaymentStatus.Pending)
        .elementAt(scrollIdx);
  }

  @override
  int getNumPayments({dynamic hint}) => this.shortPayments.length;

  @override
  int getNumPendingPayments({dynamic hint}) => this
      .shortPayments
      .where((payment) => payment.status == PaymentStatus.Pending)
      .length;

  @override
  int getNumFinalizedPayments({dynamic hint}) => this
      .shortPayments
      .where((payment) => payment.status != PaymentStatus.Pending)
      .length;
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
