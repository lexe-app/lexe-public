// An alternate application entrypoint specifically for designing pages
// and components in isolation, without actually touching any real backends.

import 'dart:async';
import 'dart:typed_data' show Uint8List;

import 'package:flutter/material.dart';
import 'package:intl/intl.dart' show Intl;

import 'bindings.dart' show api;
import 'bindings_generated_api.dart'
    show
        App,
        AppHandle,
        AppRs,
        ClientPaymentId,
        Config,
        EstimateFeeSendOnchainRequest,
        EstimateFeeSendOnchainResponse,
        FeeEstimate,
        FiatRate,
        FiatRates,
        Invoice,
        NodeInfo,
        Payment,
        PaymentDirection,
        PaymentKind,
        PaymentStatus,
        SendOnchainRequest,
        ShortPayment,
        U8Array32;
import 'cfg.dart' as cfg;
import 'components.dart' show HeadingText, ScrollableSinglePageBody;
import 'date_format.dart' as date_format;
import 'gdrive_auth.dart' show GDriveAuth, GDriveAuthInfo;
import 'logger.dart';
import 'result.dart';
import 'route/backup_wallet.dart' show BackupWalletPage;
import 'route/landing.dart' show LandingPage;
import 'route/payment_detail.dart' show PaymentDetailPageInner;
import 'route/scan.dart' show ScanPage;
import 'route/send.dart'
    show
        SendAmountAll,
        SendAmountExact,
        SendContext,
        SendPaymentAmountPage,
        SendPaymentConfirmPage,
        SendPaymentPage;
import 'route/show_qr.dart' show ShowQrPage;
import 'route/signup.dart' show SignupApi, SignupBackupPasswordPage, SignupPage;
import 'route/wallet.dart' show WalletPage;
import 'style.dart' show Fonts, LxColors, LxTheme, Space;

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // Initialize date formatting locale data for ALL locales.
  await date_format.initializeDateLocaleData();

  // Uncomment one to try designs with a different locale:
  Intl.defaultLocale = "en_US"; // English - USA
  // Intl.defaultLocale = "ar_EG"; // Arabic - Egypt
  // Intl.defaultLocale = "fr_FR"; // French - France
  // Intl.defaultLocale = "nb"; // Norwegian Bokmål

  Logger.init();

  final Config config = await cfg.buildTest();
  info("Test build config: $config");

  runApp(
    MaterialApp(
      title: "Lexe App - Design Mode",
      color: LxColors.background,
      themeMode: ThemeMode.light,
      theme: LxTheme.light(),
      debugShowCheckedModeBanner: false,
      home: LexeDesignHome(config: config),
    ),
  );
}

class LexeDesignHome extends StatelessWidget {
  const LexeDesignHome({super.key, required this.config});

  final Config config;

  @override
  Widget build(BuildContext context) {
    final mockApp = MockAppHandle(bridge: api);
    const mockGDriveAuth = MockGDriveAuth();
    final mockSignupApi = MockSignupApi(app: mockApp);

    final cidBytes = List.generate(32, (idx) => idx);
    final cid = ClientPaymentId(id: U8Array32(Uint8List.fromList(cidBytes)));

    const feeEstimates = EstimateFeeSendOnchainResponse(
      high: FeeEstimate(amountSats: 849),
      normal: FeeEstimate(amountSats: 722),
      background: FeeEstimate(amountSats: 563),
    );

    return Scaffold(
      body: ScrollableSinglePageBody(
        padding: EdgeInsets.zero,
        body: [
          const SizedBox(height: Space.s800),
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: Space.s600),
            child: HeadingText(text: "Lexe Design Home"),
          ),
          const SizedBox(height: Space.s500),
          Component(
              "LandingPage",
              (_) => LandingPage(
                    config: config,
                    gdriveAuth: mockGDriveAuth,
                    signupApi: mockSignupApi,
                  )),
          Component("BackupWalletPage",
              (_) => BackupWalletPage(config: config, app: mockApp)),
          Component(
              "WalletPage",
              (_) => WalletPage(
                    app: mockApp,
                    config: config,
                  )),
          Component("ScanPage", (_) => const ScanPage()),
          Component(
            "ShowQrPage",
            subtitle: "standard bip21",
            (_) => const ShowQrPage(
              value:
                  "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday",
            ),
          ),
          Component(
            "ShowQrPage",
            subtitle: "bitcoin address only",
            (_) => const ShowQrPage(
                value: "bitcoin:BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4"),
          ),
          Component(
            "ShowQrPage",
            subtitle: "unified bolt 12",
            (_) => const ShowQrPage(
              value:
                  "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday&lightning=LNBC10U1P3PJ257PP5YZTKWJCZ5FTL5LAXKAV23ZMZEKAW37ZK6KMV80PK4XAEV5QHTZ7QDPDWD3XGER9WD5KWM36YPRX7U3QD36KUCMGYP282ETNV3SHJCQZPGXQYZ5VQSP5USYC4LK9CHSFP53KVCNVQ456GANH60D89REYKDNGSMTJ6YW3NHVQ9QYYSSQJCEWM5CJWZ4A6RFJX77C490YCED6PEMK0UPKXHY89CMM7SCT66K8GNEANWYKZGDRWRFJE69H9U5U0W57RRCSYSAS7GADWMZXC8C6T0SPJAZUP6",
            ),
          ),
          Component(
            "SendPaymentPage",
            (context) => SendPaymentPage(
              sendCtx: SendContext(
                app: mockApp,
                configNetwork: config.network,
                balanceSats: 123456,
                cid: cid,
              ),
            ),
          ),
          Component(
            "SendPaymentAmountPage",
            (context) => SendPaymentAmountPage(
              sendCtx: SendContext(
                app: mockApp,
                configNetwork: config.network,
                balanceSats: 73450,
                cid: cid,
              ),
              address: "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
            ),
          ),
          Component(
            "SendPaymentConfirmPage",
            subtitle: "sending exact amount",
            (context) => SendPaymentConfirmPage(
              sendCtx: SendContext(
                app: mockApp,
                configNetwork: config.network,
                balanceSats: 73450,
                cid: cid,
              ),
              address: "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
              sendAmount: const SendAmountExact(2500),
              feeEstimates: feeEstimates,
            ),
          ),
          Component(
            "SendPaymentConfirmPage",
            subtitle: "sending full balance",
            (context) => SendPaymentConfirmPage(
              sendCtx: SendContext(
                app: mockApp,
                configNetwork: config.network,
                balanceSats: 73450,
                cid: cid,
              ),
              address: "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
              sendAmount: const SendAmountAll(),
              feeEstimates: feeEstimates,
            ),
          ),
          Component(
            "SignupPage",
            (context) => SignupPage(
              config: config,
              gdriveAuth: mockGDriveAuth,
              signupApi: mockSignupApi,
            ),
          ),
          Component(
            "SignupBackupPasswordPage",
            (context) => SignupBackupPasswordPage(
              config: config,
              authInfo: const GDriveAuthInfo(authCode: "fake"),
              signupApi: mockSignupApi,
            ),
          ),
          Component(
            "PaymentDetailPage",
            subtitle: "btc pending outbound",
            (context) => const PaymentDetailPageInner(
                payment: Payment(
              index:
                  "0000001687309696000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68",
              kind: PaymentKind.Onchain,
              direction: PaymentDirection.Outbound,
              invoice: null,
              amountSat: 77000,
              feesSat: 3349,
              status: PaymentStatus.Pending,
              statusStr: "broadcasted",
              note: "Brunch w/ friends",
              createdAt: 1687385080000,
            )),
          ),
          const SizedBox(height: Space.s800),
        ],
      ),
    );
  }
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

  // New user has no payments
  // List<Payment> payments = [];

  // Some sample data
  List<Payment> payments = [
    const Payment(
      index:
          "0000001687090000000-bc_551df4ef3b67b3f2ca53f3e668eb73c2a9b3a77dea84b340fd2407ec5542aa66",
      kind: PaymentKind.Onchain,
      direction: PaymentDirection.Inbound,
      amountSat: 20000,
      feesSat: 2233,
      status: PaymentStatus.Completed,
      statusStr: "fully confirmed (6+ confirmations)",
      createdAt: 1670090392000,
    ),
    const Payment(
      index:
          "0000001687120000000-bc_46e52089b60b00de067c84ce58d34a75ffd71a106f720855bc099f20da11700c",
      kind: PaymentKind.Onchain,
      direction: PaymentDirection.Outbound,
      amountSat: 95000000,
      feesSat: 433,
      status: PaymentStatus.Failed,
      statusStr: "dropped from mempool",
      note: "Sweep from Muun",
      createdAt: 1671818392000,
    ),
    const Payment(
      index:
          "0000001687130000000-ln_6973b3c58738403ceb3fccec470365a44361f34f4c2664ccae04f0f39fe71dc0",
      kind: PaymentKind.Spontaneous,
      direction: PaymentDirection.Outbound,
      amountSat: 123000,
      feesSat: 615,
      status: PaymentStatus.Pending,
      statusStr: "pending",
      note: "🍑🍑🍑🍆🍆🍆😂😂😂",
      createdAt: 1686938392000,
    ),
    const Payment(
      index:
          "0000001687150000000-ln_6f9dad93ceb2e78181ef5cb73601a28930e9774204d6fb335297b1f4add83d30",
      kind: PaymentKind.Invoice,
      direction: PaymentDirection.Inbound,
      invoice: Invoice(
        string:
            "lnbcrt1pvjluezpp5qqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqypqdq5vdhkven9v5sxyetpdeessp5zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zygs9q5sqqqqqqqqqqqqqqqqsgq2a25dxl5hrntdtn6zvydt7d66hyzsyhqs4wdynavys42xgl6sgx9c4g7me86a27t07mdtfry458rtjr0v92cnmswpsjscgt2vcse3sgpz3uapa",
        description: "some note the invoice creator set",
        createdAt: 1687140000000,
        expiresAt: 1687160000000,
      ),
      amountSat: 4470000,
      feesSat: 22350,
      status: PaymentStatus.Pending,
      statusStr: "claiming",
      note:
          "My super long note that really is too long it just keeps going and going",
      createdAt: 1687150000000,
    ),
    const Payment(
      index:
          "0000001687200000000-ln_6fc9375017dd3d911fe4ee52f4becd2f376384f42053381a09c99cca61dbf87a",
      kind: PaymentKind.Invoice,
      direction: PaymentDirection.Inbound,
      invoice: Invoice(
        string:
            "lnbcrt1metc0ejddlfthml0fz7z0e3zjk7wu2ltewa7lmms49a7lm6gh0h77l89yus9par2qqcw60rr4058g6slcty49q4uv5420qhu9tk7d080dys8e2lwy5u2q2uvstpynnc76u7x9f0y03sx06zlqfwcyjtuuu42j4eu9z478jtuu24wawlwalycdlp847j9087ftu90cfl9d57wnt3vqtu2q2uthj3j6tcz6y7z0etdqm7z5erx32wme699587wj2ndwghteta8d4gasv9tw34q49cmqf7xttrx8jh66m4saw9mcelt5epyjj5eqfezaz4uykh65hlmamhusq5ffuc25lryvmks7spuyljk60rpah4ek0r95nhng6s4rlyu4zucrfyu3r577z5598yg32whcfl9d5wd2l92535hcmrpardvhz05dg23fv3wnj7xqtpqpn72httdspp5dcs72ef655jh50zvzl9q08m8c9vx2pyqpn50kaf459fktsslvwtqsp5zmlelhnpqnxf5vfs8g3dpjjpwnuw5p50z3xzulya5wl8lp5xn7uq9qrsgqcqdtnm4um9mexh07x098dsnfrj0g27dux806kdtn5sumqu2v8dwm6xmc6u4khmmfv29n9m3mnhp7ta59gv49z6tflmmv89wmpa3jehplhqefudz2dc9huagppr2yu5",
        createdAt: 1687100000000,
        expiresAt: 1687300000000,
      ),
      amountSat: 222000,
      feesSat: 1466,
      status: PaymentStatus.Completed,
      statusStr: "completed",
      createdAt: 1687200000000,
    ),
    const Payment(
      index:
          "0000001687309696000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68",
      kind: PaymentKind.Onchain,
      direction: PaymentDirection.Outbound,
      amountSat: 77000,
      feesSat: 2881,
      status: PaymentStatus.Completed,
      statusStr: "fully confirmed (6+ confirmations)",
      note: "Brunch w/ friends",
      createdAt: 1687385080000,
    ),
  ];

  @override
  Future<NodeInfo> nodeInfo({dynamic hint}) => Future.delayed(
        const Duration(milliseconds: 1000),
        () => const NodeInfo(
          nodePk:
              "03fedbc6adf1a7175389d26b2896d10ef00fa71c81ba085a7c8cd34b6a4e0f7556",
          version: "1.2.3",
          measurement:
              "1d97c2c837b09ec7b0e0b26cb6fa9a211be84c8fdb53299cc9ee8884c7a25ac1",
          spendableBalanceSats: 727505,
        ),
      );

  @override
  Future<FiatRates> fiatRates({dynamic hint}) => Future.delayed(
        const Duration(milliseconds: 1300),
        () => const FiatRates(
          timestampMs: 1679863795,
          rates: [
            FiatRate(fiat: "USD", rate: 30296.1951578664 /* USD/BTC */),
            FiatRate(
              fiat: "EUR",
              rate: 30296.1951578664 /* USD/BTC */ * 1.10 /* EUR/USD */,
            ),
          ],
        ),
      );

  @override
  Future<void> sendOnchain({
    required SendOnchainRequest req,
    dynamic hint,
  }) =>
      Future.delayed(const Duration(milliseconds: 1200), () {});

  @override
  Future<EstimateFeeSendOnchainResponse> estimateFeeSendOnchain(
          {required EstimateFeeSendOnchainRequest req, dynamic hint}) =>
      Future.delayed(
        const Duration(seconds: 1),
        () => const EstimateFeeSendOnchainResponse(
          high: FeeEstimate(amountSats: 849),
          normal: FeeEstimate(amountSats: 722),
          background: FeeEstimate(amountSats: 563),
        ),
      );

  @override
  Future<String> getAddress({dynamic hint}) =>
      Future.value("bcrt1q2nfxmhd4n3c8834pj72xagvyr9gl57n5r94fsl");

  @override
  Future<bool> syncPayments({dynamic hint}) => Future.delayed(
        const Duration(milliseconds: 1500),
        () => true,
      );

  @override
  Payment? getPaymentByVecIdx({required int vecIdx, dynamic hint}) =>
      this.payments[vecIdx];

  @override
  (int, ShortPayment)? getShortPaymentByScrollIdx(
      {required int scrollIdx, dynamic hint}) {
    if (scrollIdx >= this.payments.length) {
      return null;
    }
    final vecIdx = this.payments.length - scrollIdx - 1;
    return (vecIdx, this.payments[vecIdx].intoShort());
  }

  @override
  (int, ShortPayment)? getPendingShortPaymentByScrollIdx(
      {required int scrollIdx, dynamic hint}) {
    if (scrollIdx >= this.getNumPendingPayments()) {
      return null;
    }
    final payment = this
        .payments
        .reversed
        .where((payment) => payment.status == PaymentStatus.Pending)
        .elementAt(scrollIdx);
    final vecIdx = this.payments.indexOf(payment);
    return (vecIdx, payment.intoShort());
  }

  @override
  (int, ShortPayment)? getFinalizedShortPaymentByScrollIdx(
      {required int scrollIdx, dynamic hint}) {
    if (scrollIdx >= this.getNumFinalizedPayments()) {
      return null;
    }
    final payment = this
        .payments
        .reversed
        .where((payment) => payment.status != PaymentStatus.Pending)
        .elementAt(scrollIdx);
    final vecIdx = this.payments.indexOf(payment);
    return (vecIdx, payment.intoShort());
  }

  @override
  int getNumPayments({dynamic hint}) => this.payments.length;

  @override
  int getNumPendingPayments({dynamic hint}) => this
      .payments
      .where((payment) => payment.status == PaymentStatus.Pending)
      .length;

  @override
  int getNumFinalizedPayments({dynamic hint}) => this
      .payments
      .where((payment) => payment.status != PaymentStatus.Pending)
      .length;
}

class MockGDriveAuth implements GDriveAuth {
  const MockGDriveAuth();

  @override
  Future<Result<GDriveAuthInfo?, Exception>> tryAuth() => Future.delayed(
        const Duration(milliseconds: 1200),
        () => const Ok(GDriveAuthInfo(authCode: "fake")),
      );
}

class Component extends StatelessWidget {
  const Component(this.title, this.builder, {super.key, this.subtitle});

  final String title;
  final WidgetBuilder builder;
  final String? subtitle;

  @override
  Widget build(BuildContext context) {
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: Space.s600),
      visualDensity: VisualDensity.comfortable,
      dense: true,
      title: Text(this.title, style: Fonts.fontUI),
      subtitle: (this.subtitle != null)
          ? Text(
              this.subtitle!,
              style: Fonts.fontUI.copyWith(
                fontSize: Fonts.size200,
                color: LxColors.fgTertiary,
              ),
            )
          : null,
      onTap: () {
        Navigator.of(context).push(MaterialPageRoute(
          builder: this.builder,
        ));
      },
    );
  }
}

class MockSignupApi implements SignupApi {
  const MockSignupApi({required this.app});

  final AppHandle app;

  @override
  Future<FfiResult<AppHandle>> signup({
    required Config config,
    required String googleAuthCode,
    required String password,
  }) =>
      Future.delayed(
        const Duration(milliseconds: 2000),
        () => Ok(this.app),
      );
}

extension PaymentExt on Payment {
  ShortPayment intoShort() => ShortPayment(
        index: this.index,
        kind: this.kind,
        direction: this.direction,
        amountSat: this.amountSat,
        status: this.status,
        note: this.note,
        createdAt: this.createdAt,
      );
}
