// An alternate application entrypoint specifically for designing pages
// and components in isolation, without actually touching any real backends.

// Ignore this lint as flutter_rust_bridge ffi errors don't impl Error/Exception...
// ignore_for_file: only_throw_errors

import 'dart:async';
import 'dart:typed_data' show Uint8List;

import 'package:collection/collection.dart';
import 'package:flutter/material.dart';
// import 'package:flutter_markdown/flutter_markdown.dart';
// import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';
import 'package:intl/intl.dart' show Intl;
import 'package:lexeapp/app_rs/ffi/ffi.dart'
    show
        App,
        AppHandle,
        Balance,
        ClientPaymentId,
        Config,
        CreateInvoiceRequest,
        CreateInvoiceResponse,
        FeeEstimate,
        FiatRate,
        FiatRates,
        Invoice,
        NodeInfo,
        Onchain,
        PayInvoiceRequest,
        PayInvoiceResponse,
        PayOnchainRequest,
        PayOnchainResponse,
        Payment,
        PaymentDirection,
        PaymentIndex,
        PaymentKind,
        PaymentMethod,
        PaymentStatus,
        PreflightPayInvoiceRequest,
        PreflightPayInvoiceResponse,
        PreflightPayOnchainRequest,
        PreflightPayOnchainResponse,
        ShortPaymentAndIndex,
        U8Array32,
        UpdatePaymentNote;
import 'package:lexeapp/app_rs/ffi/ffi.ext.dart' show PaymentExt;
import 'package:lexeapp/app_rs/frb_generated.dart' show AppRs;
import 'package:lexeapp/app_rs/load.dart' show appRsLib;
import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/components.dart'
    show
        HeadingText,
        LoadingSpinnerModal,
        LxBackButton,
        LxFilledButton,
        LxOutlinedButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SubheadingText,
        showModalAsyncFlow;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth, GDriveAuthInfo;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/landing.dart' show LandingPage;
import 'package:lexeapp/route/payment_detail.dart' show PaymentDetailPageInner;
import 'package:lexeapp/route/receive.dart'
    show LnInvoiceInputs, ReceivePaymentEditInvoicePage, ReceivePaymentPage;
import 'package:lexeapp/route/scan.dart' show ScanPage;
import 'package:lexeapp/route/send/page.dart' show SendPaymentPage;
import 'package:lexeapp/route/send/state.dart'
    show
        PreflightedPayment_Onchain,
        SendFlowResult,
        SendState_NeedAmount,
        SendState_NeedUri,
        SendState_Preflighted;
import 'package:lexeapp/route/show_qr.dart' show ShowQrPage;
import 'package:lexeapp/route/signup.dart'
    show SignupApi, SignupBackupPasswordPage, SignupPage;
import 'package:lexeapp/route/wallet.dart' show WalletPage;
import 'package:lexeapp/stream_ext.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxTheme, Space;
import 'package:lexeapp/uri_events.dart' show UriEvents;
import 'package:rxdart_ext/rxdart_ext.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // TODO(phlip9): need to init here?
  await AppRs.init(externalLibrary: appRsLib);

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

  final uriEvents = await UriEvents.prod();

  runApp(
    MaterialApp(
      title: "Lexe App - Design Mode",
      color: LxColors.background,
      themeMode: ThemeMode.light,
      theme: LxTheme.light(),
      debugShowCheckedModeBanner: false,
      home: LexeDesignPage(config: config, uriEvents: uriEvents),
    ),
  );
}

class LexeDesignPage extends StatefulWidget {
  const LexeDesignPage(
      {super.key, required this.config, required this.uriEvents});

  final Config config;
  final UriEvents uriEvents;

  @override
  State<LexeDesignPage> createState() => _LexeDesignPageState();
}

class _LexeDesignPageState extends State<LexeDesignPage> {
  // When this stream ticks, all the payments' createdAt label should update.
  // This stream ticks every 30 seconds.
  final StateSubject<DateTime> paymentDateUpdates =
      StateSubject(DateTime.now());
  Timer? paymentDateUpdatesTimer;

  @override
  void dispose() {
    this.paymentDateUpdatesTimer?.cancel();
    this.paymentDateUpdates.close();

    super.dispose();
  }

  @override
  void initState() {
    super.initState();

    this.paymentDateUpdatesTimer =
        Timer.periodic(const Duration(seconds: 30), (timer) {
      this.paymentDateUpdates.addIfNotClosed(DateTime.now());
    });
  }

  ValueStream<FiatRate?> makeFiatRateStream() =>
      Stream.fromIterable(<FiatRate?>[
        const FiatRate(fiat: "USD", rate: 73111.19),
        const FiatRate(fiat: "USD", rate: 73222.29),
        const FiatRate(fiat: "USD", rate: 73333.39),
      ]).interval(const Duration(seconds: 2)).shareValueSeeded(null);

  /// Complete the payment after a few seconds
  ValueNotifier<Payment> makeCompletingPayment(final Payment payment) {
    final notifier = ValueNotifier(payment);

    unawaited(Future.delayed(const Duration(seconds: 4), () {
      final p = notifier.value;
      notifier.value = p.copyWith(
        status: PaymentStatus.completed,
        statusStr: "completed",
        finalizedAt: DateTime.now().millisecondsSinceEpoch,
      );
    }));

    return notifier;
  }

  @override
  Widget build(BuildContext context) {
    final mockApp = MockAppHandle();
    const mockGDriveAuth = GDriveAuth.mock;
    final mockSignupApi = MockSignupApi(app: mockApp);

    final cidBytes = List.generate(32, (idx) => idx);
    final cid = ClientPaymentId(id: U8Array32(Uint8List.fromList(cidBytes)));

    const balance = Balance(
      onchainSats: 111111,
      lightningSats: 222222,
      totalSats: 111111 + 222222,
    );

    const feeEstimates = PreflightPayOnchainResponse(
      high: FeeEstimate(amountSats: 849),
      normal: FeeEstimate(amountSats: 722),
      background: FeeEstimate(amountSats: 563),
    );

    return Theme(
      data: LxTheme.light(),
      child: Scaffold(
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
              (context) => LandingPage(
                config: widget.config,
                gdriveAuth: mockGDriveAuth,
                signupApi: mockSignupApi,
                uriEvents: this.widget.uriEvents,
              ),
            ),
            Component(
              "SignupPage",
              (context) => SignupPage(
                config: widget.config,
                gdriveAuth: mockGDriveAuth,
                signupApi: mockSignupApi,
              ),
            ),
            Component(
              "SignupBackupPasswordPage",
              (context) => SignupBackupPasswordPage(
                config: widget.config,
                authInfo: const GDriveAuthInfo(authCode: "fake"),
                signupApi: mockSignupApi,
              ),
            ),
            Component(
              "WalletPage",
              (_) => WalletPage(
                app: mockApp,
                config: widget.config,
                uriEvents: this.widget.uriEvents,
              ),
            ),
            Component(
              "SendPaymentNeedUriPage",
              (context) => SendPaymentPage(
                startNewFlow: true,
                sendCtx: SendState_NeedUri(
                  app: mockApp,
                  configNetwork: widget.config.network,
                  balance: balance,
                  cid: cid,
                ),
              ),
            ),
            Component(
              "SendPaymentAmountPage",
              subtitle: "onchain address-only",
              (context) => SendPaymentPage(
                startNewFlow: true,
                sendCtx: SendState_NeedAmount(
                  app: mockApp,
                  configNetwork: widget.config.network,
                  balance: balance,
                  cid: cid,
                  paymentMethod: const PaymentMethod.onchain(
                    Onchain(
                        address: "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"),
                  ),
                ),
              ),
            ),
            Component(
              "SendPaymentConfirmPage",
              subtitle: "onchain",
              (context) => SendPaymentPage(
                startNewFlow: true,
                sendCtx: SendState_Preflighted(
                  app: mockApp,
                  configNetwork: widget.config.network,
                  balance: balance,
                  cid: cid,
                  preflightedPayment: const PreflightedPayment_Onchain(
                    onchain: Onchain(
                        address: "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"),
                    preflight: feeEstimates,
                    amountSats: 2500,
                  ),
                ),
              ),
            ),
            Component(
              "ReceivePaymentPage",
              (context) => ReceivePaymentPage(
                app: mockApp,
                fiatRate: this.makeFiatRateStream(),
              ),
            ),
            Component(
              "ReceivePaymentPage",
              subtitle: "fetch invoice error",
              (context) => ReceivePaymentPage(
                app: MockAppHandleErroring(),
                fiatRate: this.makeFiatRateStream(),
              ),
            ),
            Component(
              "ReceivePaymentEditInvoicePage",
              (context) => const ReceivePaymentEditInvoicePage(
                prev: LnInvoiceInputs(amountSats: null, description: null),
              ),
            ),
            Component(
              "PaymentDetailPage",
              subtitle: "btc failed outbound",
              (context) => PaymentDetailPageInner(
                app: mockApp,
                payment: ValueNotifier(dummyOnchainOutboundFailed01),
                paymentDateUpdates: this.paymentDateUpdates,
                fiatRate: this.makeFiatRateStream(),
                isRefreshing: ValueNotifier(false),
                triggerRefresh: () {},
              ),
            ),
            Component(
              "PaymentDetailPage",
              subtitle: "btc completed inbound",
              (context) => PaymentDetailPageInner(
                app: mockApp,
                payment: ValueNotifier(dummyOnchainInboundCompleted01),
                paymentDateUpdates: this.paymentDateUpdates,
                fiatRate: this.makeFiatRateStream(),
                isRefreshing: ValueNotifier(false),
                triggerRefresh: () {},
              ),
            ),
            Component(
              "PaymentDetailPage",
              subtitle: "ln invoice pending inbound",
              (context) => PaymentDetailPageInner(
                app: mockApp,
                payment:
                    this.makeCompletingPayment(dummyInvoiceInboundPending01),
                paymentDateUpdates: this.paymentDateUpdates,
                fiatRate: this.makeFiatRateStream(),
                isRefreshing: ValueNotifier(false),
                triggerRefresh: () {},
              ),
            ),
            Component(
              "ScanPage",
              (_) => MultistepFlow<SendFlowResult>(
                builder: (_) => ScanPage(
                  sendCtx: SendState_NeedUri(
                    app: mockApp,
                    configNetwork: widget.config.network,
                    balance: balance,
                    cid: cid,
                  ),
                ),
              ),
            ),
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
              "Buttons",
              (_) => const ButtonDesignPage(),
            ),
            Component(
              "ModalAsyncFlow",
              (_) => const ModalAsyncFlowDesignPage(),
            ),
            // Component(
            //   "Markdown",
            //   (context) => const MarkdownPage(),
            // ),
            const SizedBox(height: Space.s800),
          ],
        ),
      ),
    );
  }
}

// TODO(phlip9): unhack
// TODO(phlip9): add a `App::mock` constructor?
class MockApp extends App {
  // This makes a fake `RustOpaque<App>` w/ a null pointer. Super hacky, but frb
  // will at least panic if we accidentally call a native method.
  MockApp();

  @override
  void dispose() {}

  @override
  bool get isDisposed => false;
}

// TODO(phlip9): unhack
class MockAppHandle extends AppHandle {
  MockAppHandle() : super(inner: MockApp());

  // New user has no payments
  // List<Payment> payments = [];

  // Some sample data
  List<Payment> payments = [
    dummyOnchainInboundCompleted01,
    dummyOnchainOutboundFailed01,
    dummySpontaneousOutboundPending01,
    dummyInvoiceOutboundPending01,
    dummyInvoiceInboundPending01,
    dummyInvoiceInboundPending02,
    dummyInvoiceInboundCompleted01,
    dummyInvoiceInboundFailed01,
    dummyOnchainOutboundCompleted01,
  ].sortedBy((payment) => payment.index.field0);

  @override
  Future<NodeInfo> nodeInfo({dynamic hint}) =>
      Future.delayed(const Duration(milliseconds: 1000), () {
        const lightningSats = 409801;
        const onchainSats = 366430;
        const totalSats = lightningSats + onchainSats;
        return const NodeInfo(
          nodePk:
              "03fedbc6adf1a7175389d26b2896d10ef00fa71c81ba085a7c8cd34b6a4e0f7556",
          version: "1.2.3",
          measurement:
              "1d97c2c837b09ec7b0e0b26cb6fa9a211be84c8fdb53299cc9ee8884c7a25ac1",
          balance: Balance(
            totalSats: totalSats,
            lightningSats: lightningSats,
            onchainSats: onchainSats,
          ),
        );
      });

  @override
  Future<FiatRates> fiatRates({dynamic hint}) => Future.delayed(
        const Duration(milliseconds: 1300),
        () => const FiatRates(
          timestampMs: 1679863795,
          rates: [
            FiatRate(fiat: "USD", rate: 73111.19 /* USD/BTC */),
            FiatRate(
              fiat: "EUR",
              rate: 73111.19 /* USD/BTC */ * 1.10 /* EUR/USD */,
            ),
          ],
        ),
      );

  @override
  Future<PayOnchainResponse> payOnchain({
    required PayOnchainRequest req,
    dynamic hint,
  }) =>
      Future.delayed(
        const Duration(milliseconds: 1200),
        () => const PayOnchainResponse(
          index: PaymentIndex(
              field0:
                  "0000001687385080000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68"),
          txid:
              "f5f119aca79fa3ff1c95793c87ecf7bcd84fa326dfedde3d3c2181a6c733e689",
        ),
      );

  @override
  Future<PreflightPayOnchainResponse> preflightPayOnchain(
          {required PreflightPayOnchainRequest req, dynamic hint}) =>
      Future.delayed(
        const Duration(seconds: 1),
        () => const PreflightPayOnchainResponse(
          high: FeeEstimate(amountSats: 849),
          normal: FeeEstimate(amountSats: 722),
          background: FeeEstimate(amountSats: 563),
        ),
        // () => throw FfiError("Request timed out").toFfi(),
      );

  @override
  Future<String> getAddress({dynamic hint}) => Future.delayed(
        const Duration(milliseconds: 1200),
        () => "bcrt1q2nfxmhd4n3c8834pj72xagvyr9gl57n5r94fsl",
      );

  @override
  Future<CreateInvoiceResponse> createInvoice(
      {required CreateInvoiceRequest req, dynamic hint}) {
    final now = DateTime.now();
    final createdAt = now.millisecondsSinceEpoch;
    final expiresAt =
        now.add(Duration(seconds: req.expirySecs)).millisecondsSinceEpoch;

    final dummy = dummyInvoiceInboundPending01.invoice!;

    return Future.delayed(
      const Duration(milliseconds: 1000),
      () => CreateInvoiceResponse(
        invoice: Invoice(
          string: dummy.string,
          createdAt: createdAt,
          expiresAt: expiresAt,
          amountSats: req.amountSats,
          description: req.description,
          payeePubkey: dummy.payeePubkey,
        ),
      ),
    );
  }

  @override
  Future<PayInvoiceResponse> payInvoice({
    required PayInvoiceRequest req,
    dynamic hint,
  }) =>
      Future.delayed(
        const Duration(milliseconds: 1200),
        () => const PayInvoiceResponse(
          index: PaymentIndex(
              field0:
                  "0000001686744442000-ln_6973b3c58738403ceb3fccec470365a44361f34f4c2664ccae04f0f39fe71dc0"),
        ),
      );

  @override
  Future<PreflightPayInvoiceResponse> preflightPayInvoice(
          {required PreflightPayInvoiceRequest req, dynamic hint}) =>
      Future.delayed(
        const Duration(seconds: 1),
        // () => throw FfiError("Request timed out").toFfi(),
        () => const PreflightPayInvoiceResponse(
          amountSats: 9999,
          feesSats: 123,
        ),
      );

  @override
  Future<bool> syncPayments({dynamic hint}) =>
      Future.delayed(const Duration(milliseconds: 1500), () => true);

  @override
  Future<int?> getVecIdxByPaymentIndex(
      {required PaymentIndex paymentIndex, dynamic hint}) async {
    final vecIdx =
        this.payments.indexWhere((payment) => payment.index == paymentIndex);
    if (vecIdx >= 0) {
      return vecIdx;
    } else {
      return null;
    }
  }

  @override
  Payment? getPaymentByVecIdx({required int vecIdx, dynamic hint}) =>
      this.payments[vecIdx];

  ShortPaymentAndIndex? _getByScrollIdx({
    required bool Function(Payment) filter,
    required int scrollIdx,
  }) {
    final result = this
        .payments
        .reversed // can't `reversed` after .indexed...
        .indexed
        .where((x) => filter(x.$2))
        .elementAtOrNull(scrollIdx);
    if (result == null) return null;
    return ShortPaymentAndIndex(
      vecIdx: this.payments.length - result.$1 - 1,
      payment: result.$2.intoShort(),
    );
  }

  @override
  ShortPaymentAndIndex? getShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      this._getByScrollIdx(filter: (_) => true, scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getPendingShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isPending(), scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getPendingNotJunkShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isPendingNotJunk(),
          scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getFinalizedShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isFinalized(), scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getFinalizedNotJunkShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isFinalizedNotJunk(),
          scrollIdx: scrollIdx);

  @override
  int getNumPayments({dynamic hint}) => this.payments.length;

  @override
  int getNumPendingPayments({dynamic hint}) =>
      this.payments.where((payment) => payment.isPending()).length;

  @override
  int getNumPendingNotJunkPayments({dynamic hint}) =>
      this.payments.where((payment) => payment.isPendingNotJunk()).length;

  @override
  int getNumFinalizedPayments({dynamic hint}) =>
      this.payments.where((payment) => payment.isFinalized()).length;

  @override
  int getNumFinalizedNotJunkPayments({dynamic hint}) =>
      this.payments.where((payment) => payment.isFinalizedNotJunk()).length;

  @override
  Future<void> updatePaymentNote(
          {required UpdatePaymentNote req, dynamic hint}) =>
      Future.delayed(const Duration(milliseconds: 1000), () => ());
}

/// An [AppHandle] that usually errors first.
class MockAppHandleErroring extends MockAppHandle {
  MockAppHandleErroring();

  @override
  Future<CreateInvoiceResponse> createInvoice(
      {required CreateInvoiceRequest req, dynamic hint}) {
    return Future.delayed(
      const Duration(milliseconds: 1000),
      () => throw const FfiError(
              "[106=Command] Error while executing command: Failed to register new payment")
          .toFfi(),
    );
  }
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

// Dummy payments data

const Payment dummyOnchainInboundPending01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001687309696000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68"),
  kind: PaymentKind.onchain,
  direction: PaymentDirection.inbound,
  amountSat: 1469,
  feesSat: 0,
  status: PaymentStatus.pending,
  statusStr: "partially confirmed (1-5 confirmations)",
  note: null,
  createdAt: 1687309696000,
  finalizedAt: null,
  replacement: null,
);

const Payment dummyOnchainInboundCompleted01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001670090492000-bc_551df4ef3b67b3f2ca53f3e668eb73c2a9b3a77dea84b340fd2407ec5542aa66"),
  kind: PaymentKind.onchain,
  direction: PaymentDirection.inbound,
  amountSat: 20000,
  feesSat: 0,
  status: PaymentStatus.completed,
  statusStr: "fully confirmed (6+ confirmations)",
  note: "Brunch w/ friends",
  createdAt: 1670090492000,
  finalizedAt: 1670090502000,
  replacement: null,
);

const Payment dummyOnchainOutboundCompleted01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001687385080000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68"),
  kind: PaymentKind.onchain,
  direction: PaymentDirection.outbound,
  amountSat: 77000,
  feesSat: 2881,
  status: PaymentStatus.completed,
  statusStr: "fully confirmed (6+ confirmations)",
  note: "Funding exchange",
  createdAt: 1687385080000,
  finalizedAt: 1687385380000,
);

const Payment dummyOnchainOutboundFailed01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001671818392000-bc_46e52089b60b00de067c84ce58d34a75ffd71a106f720855bc099f20da11700c"),
  kind: PaymentKind.onchain,
  direction: PaymentDirection.outbound,
  amountSat: 95000000,
  feesSat: 5433,
  status: PaymentStatus.failed,
  statusStr: "dropped from mempool",
  note: "Sweep from Muun",
  createdAt: 1671818392000,
  finalizedAt: 1671918392000,
  replacement: null,
);

const Payment dummySpontaneousOutboundPending01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001686938392000-ln_6973b3c58738403ceb3fccec470365a44361f34f4c2664ccae04f0f39fe71dc0"),
  kind: PaymentKind.spontaneous,
  direction: PaymentDirection.outbound,
  amountSat: 123000,
  feesSat: 615,
  status: PaymentStatus.pending,
  statusStr: "pending",
  note: "🍑🍑🍑🍆🍆🍆😂😂😂",
  createdAt: 1686938392000,
);

const Payment dummyInvoiceOutboundPending01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001686744442000-ln_6973b3c58738403ceb3fccec470365a44361f34f4c2664ccae04f0f39fe71dc0"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.outbound,
  invoice: Invoice(
    string:
        "lnbcrt4693500n1pjgld4pxq8pjglhd3pp5h038tqal0m3xjwrmht2gcj8u4cgwg9fh6d0ynv2ds8x8xph5sm9ssp5d4jx76ttd4ek76tnv3hkv6tpdfekgenvdfkx76t2wdskg6nxda5s9qrsgqdp4wdhk6efqdehhgefqw35x2grfdemx76trv5sxxun9v96x7u3qwdjhgcqpcnp4qgywe59xssrqj004k24477svqtgynw4am39hz06hk4dlu4l0ssk8w2rpkgvpsusjrwde5qym0t9g42px0dahyh7jz9lvn5umk9gzqxtc8r0rdplu9psdewwqnw6t7uvdqtvn6heqfgxvn9a76kkl760cy4rqpewlfe6",
    description: "wuhhh",
    createdAt: 1686743442000,
    expiresAt: 1686745442000,
    amountSats: 55000,
    payeePubkey:
        "03fedbc6adf1a7175389d26b2896d10ef00fa71c81ba085a7c8cd34b6a4e0f7556",
  ),
  amountSat: 55000,
  feesSat: 150,
  status: PaymentStatus.pending,
  statusStr: "pending",
  note: null,
  createdAt: 1686744442000,
);

const Payment dummyInvoiceInboundPending01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001687140003000-ln_bbe27583bf7ee269387bbad48c48fcae10e41537d35e49b14d81cc7306f486cb"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.inbound,
  invoice: Invoice(
    string:
        "lnbcrt4693500n1pjgld4pxq8pjglhd3pp5h038tqal0m3xjwrmht2gcj8u4cgwg9fh6d0ynv2ds8x8xph5sm9ssp5d4jx76ttd4ek76tnv3hkv6tpdfekgenvdfkx76t2wdskg6nxda5s9qrsgqdp4wdhk6efqdehhgefqw35x2grfdemx76trv5sxxun9v96x7u3qwdjhgcqpcnp4qgywe59xssrqj004k24477svqtgynw4am39hz06hk4dlu4l0ssk8w2rpkgvpsusjrwde5qym0t9g42px0dahyh7jz9lvn5umk9gzqxtc8r0rdplu9psdewwqnw6t7uvdqtvn6heqfgxvn9a76kkl760cy4rqpewlfe6",
    description: "some note the invoice creator set",
    createdAt: 1687140001000,
    expiresAt: 1687150001000,
    amountSats: 469350,
    payeePubkey:
        "772c84ef57fe5bb5573f714bdcbdba49d0020c7a5fabb2f53d090684a6d0ec082ee2f633d8398b2dd0bade4b2fd2fc78ec881b1296e4834b48c0e73c9edbc774",
  ),
  amountSat: 469350,
  feesSat: 2350,
  status: PaymentStatus.pending,
  statusStr: "claiming",
  note:
      "My super long note that really is too long it just keeps going and going",
  createdAt: 1687140003000,
);

// Junk payment
const Payment dummyInvoiceInboundPending02 = Payment(
  index: PaymentIndex(
      field0:
          "0000001714432815000-ln_c6e5e46c59267114f91d64df0e069b0dae176f9a134656820bba1e6164318980"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.inbound,
  invoice: Invoice(
    string:
        "lnbcrt1pnrq2e0xq8pnrqvaepp5cmj7gmzeyec3f7gavn0sup5mpkhpwmu6zdr9dqsthg0xzep33xqqsp5dfhkjumxv3hkj6npwdhkgenfdfshxmmfv3nx5mmfwdskg6nxda5s9qrsgqdqqcqpcnp4qwla7nx7p5e5nau5k2hh2gxf736rhw0naslthr3jmyu5jqk8gjx7v62qr2p6rh6v38kclflj2yk5x90jsshpe77tjzngc4enn2muxwhu54haacvyef60y5xz2xslezykrvfqlj9yfe4d0tdjrdtx44jusr8sqtehvp3",
    description: null,
    createdAt: 1714432815000,
    expiresAt: 1714435001000,
    amountSats: null,
    payeePubkey:
        "e68d44c7024939d9328ebb3eecf3b93b74f4c92075afb294f749330dde4cdfbfe5a75ff4cbb752a40e1c4947255d2a9c0ae88c826b5f47d6d660ce9b7c6ebca1",
  ),
  amountSat: null,
  feesSat: 0,
  status: PaymentStatus.pending,
  statusStr: "claiming",
  note: null,
  createdAt: 1714432815000,
);

const Payment dummyInvoiceInboundCompleted01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001687100002000-ln_801ffce9fbe74fecc7ec6fa72716d7de6167cc5607635062b24797b54f9ba4be"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.inbound,
  invoice: Invoice(
    string:
        "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
    createdAt: 1687100000000,
    expiresAt: 1687110000000,
    amountSats: 223466,
    description: "Direct deposit",
    payeePubkey:
        "28157d6ca3555a0a3275817d0832c535955b28b20a55f9596f6873434feebfd797d4b245397fab8f8f94dcdd32aac475d64893aa042f18b8d725e116082ae909",
  ),
  amountSat: 223466,
  feesSat: 0,
  status: PaymentStatus.completed,
  statusStr: "completed",
  note: null,
  createdAt: 1687100002000,
  finalizedAt: 1687100005000,
);

// Junk payment (failed)
const Payment dummyInvoiceInboundFailed01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001700222815000-ln_034a21eee2bea4288ec9582b10a4abd6bfdca83855b25257279e67dd02f77d43"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.inbound,
  invoice: Invoice(
    string:
        "lnbcrt1pj4w46lxq8pj4whlfpp5qd9zrmhzh6jz3rkftq43pf9t66lae2pc2ke9y4e8nena6qhh04pssp5v9k8xerxdfhkj6n0d9ekg6nxda5hxer2vekxk6npd3skk6nnve5s9qrsgqdqqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jsc5p3dhdl25x88ndth9qzc4ms2wm5xwa9xfw56dapyaj5n84vv7djsgul2gyjdvk9xzu2pjqv59lfssmft95x43gqqqq5g05r93epkpqpq8a02n",
    description: null,
    createdAt: 1700222815000,
    expiresAt: 1700225001000,
    amountSats: null,
    payeePubkey:
        "28157d6ca3555a0a3275817d0832c535955b28b20a55f9596f6873434feebfd797d4b245397fab8f8f94dcdd32aac475d64893aa042f18b8d725e116082ae909",
  ),
  amountSat: null,
  feesSat: 0,
  status: PaymentStatus.failed,
  statusStr: "expired",
  note: null,
  createdAt: 1700222815000,
);

// Some design-specific pages

class ButtonDesignPage extends StatelessWidget {
  const ButtonDesignPage({super.key});

  static void onTap() {
    info("tapped");
  }

  @override
  Widget build(BuildContext context) {
    return Theme(
      // // Uncomment to view default material theme:
      // data: ThemeData.light(useMaterial3: true),
      data: LxTheme.light(),
      child: Scaffold(
        appBar: AppBar(
          leadingWidth: Space.appBarLeadingWidth,
          leading: const LxBackButton(isLeading: true),
        ),
        body: ScrollableSinglePageBody(
          body: [
            const HeadingText(text: "Button design page"),
            const SubheadingText(text: "Check button styling here"),
            const SizedBox(height: Space.s600),

            //
            // Outlined Buttons
            //
            const HeadingText(text: "Outlined buttons"),
            const SizedBox(height: Space.s400),

            // normal
            const LxOutlinedButton(onTap: onTap, label: Text("Send")),
            const SizedBox(height: Space.s400),

            // disabled
            const LxOutlinedButton(onTap: null, label: Text("Send")),
            const SizedBox(height: Space.s400),

            // normal + icon
            const LxOutlinedButton(
              onTap: onTap,
              label: Text("Next"),
              icon: Icon(LxIcons.next),
            ),
            const SizedBox(height: Space.s400),

            // disabled + icon
            const LxOutlinedButton(
              onTap: null,
              label: Text("Next"),
              icon: Icon(LxIcons.next),
            ),
            const SizedBox(height: Space.s400),

            //
            // Filled Buttons
            //
            const SizedBox(height: Space.s400),
            const HeadingText(text: "Filled buttons"),
            const SizedBox(height: Space.s400),

            // normal
            const LxFilledButton(
              onTap: onTap,
              label: Text("Send"),
            ),
            const SizedBox(height: Space.s400),

            // disabled
            const LxFilledButton(
              onTap: null,
              label: Text("Send"),
            ),
            const SizedBox(height: Space.s400),

            // moneyGoUp + icon
            LxFilledButton.tonal(
              onTap: onTap,
              label: const Text("Send"),
              icon: const Icon(LxIcons.next),
            ),
            const SizedBox(height: Space.s400),

            // dark + icon
            LxFilledButton.strong(
              onTap: onTap,
              label: const Text("Send"),
              icon: const Icon(LxIcons.next),
            ),
            const SizedBox(height: Space.s400),

            // disabled + icon
            const LxFilledButton(
              onTap: null,
              label: Text("Send"),
              icon: Icon(LxIcons.next),
            ),
            const SizedBox(height: Space.s600),

            //
            // Buttons in a row
            //
            const HeadingText(text: "Buttons in a row"),
            const SizedBox(height: Space.s400),

            const Row(
              children: [
                Expanded(
                    child:
                        LxOutlinedButton(onTap: onTap, label: Text("Cancel"))),
                SizedBox(width: Space.s400),
                Expanded(
                  child: LxFilledButton(
                    onTap: onTap,
                    label: Text("Next"),
                    icon: Icon(LxIcons.next),
                  ),
                ),
              ],
            ),
            const SizedBox(height: Space.s400),

            const Row(
              children: [
                Expanded(
                    child:
                        LxOutlinedButton(onTap: onTap, label: Text("Cancel"))),
                SizedBox(width: Space.s200),
                Expanded(
                    child: LxOutlinedButton(onTap: onTap, label: Text("Skip"))),
                SizedBox(width: Space.s200),
                Expanded(
                  child: LxFilledButton(onTap: onTap, label: Text("Next")),
                ),
              ],
            ),
            const SizedBox(height: Space.s400),

            const SizedBox(height: Space.s1200),
          ],
        ),
      ),
    );
  }
}

class ModalAsyncFlowDesignPage extends StatelessWidget {
  const ModalAsyncFlowDesignPage({super.key});

  Future<void> openLoadingModal(BuildContext context) async {
    await showDialog(
      context: context,
      builder: (_) => const LoadingSpinnerModal(),
    );
  }

  Future<void> showModalAsyncFlowOk(BuildContext context) async {
    final result = await showModalAsyncFlow(
      context: context,
      future: Future.delayed(
        const Duration(milliseconds: 1500),
        () => const Ok("success"),
      ),
    );
    info("startModalAsyncFlowOk: result: $result");
  }

  Future<void> showModalAsyncFlowErr(BuildContext context) async {
    final result = await showModalAsyncFlow(
      context: context,
      future: Future.delayed(
        const Duration(milliseconds: 1500),
        () => const Err(
            "W/WindowOnBackDispatcher(26148): Set 'android:enableOnBackInvokedCallback=\"true\"' in the application manifest."),
      ),
      errorBuilder: (context, err) => AlertDialog(
        title: const Text("Issue with payment"),
        content: Text(err),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text("Close"),
          ),
        ],
      ),
    );
    info("startModalAsyncFlowOk: result: $result");
  }

  @override
  Widget build(BuildContext context) {
    return Theme(
      data: LxTheme.light(),
      child: Scaffold(
        appBar: AppBar(
          leadingWidth: Space.appBarLeadingWidth,
          leading: const LxBackButton(isLeading: true),
        ),
        body: Builder(
          builder: (context) => ScrollableSinglePageBody(
            body: [
              const SizedBox(height: Space.s800),

              //
              LxOutlinedButton(
                onTap: () => this.openLoadingModal(context),
                label: const Text("Open loading spinner modal"),
              ),
              const SizedBox(height: Space.s400),

              //
              LxOutlinedButton(
                onTap: () => this.showModalAsyncFlowOk(context),
                label: const Text("Modal async flow (ok)"),
              ),
              const SizedBox(height: Space.s400),

              //
              LxOutlinedButton(
                onTap: () => this.showModalAsyncFlowErr(context),
                label: const Text("Modal async flow (err)"),
              ),
              const SizedBox(height: Space.s400),
            ],
          ),
        ),
      ),
    );
  }
}

// class MarkdownPage extends StatelessWidget {
//   const MarkdownPage({super.key});
//
//   @override
//   Widget build(BuildContext context) {
//     return Scaffold(
//       appBar: AppBar(
//         leading: const LxBackButton(isLeading: true),
//         leadingWidth: Space.appBarLeadingWidth,
//       ),
//       body: Theme(
//         data: LxTheme.light(),
//         child: ScrollableSinglePageBody(
//           body: [
//             MarkdownBody(
//               data: '''
// # ACME protocol
//
// ACME is a protocol for automated provisioning of web PKI certificates, which are
// just certs bound to public domains and endorsed by web root CAs like Let's Encrypt.
//
// ## Why
//
// Before ACME, web service operators had to manually update their web certs.
// This error prone and time-consuming process meant that many sites either:
//
// 1. Didn't use HTTPS at all, reducing security.
// 2. Used multi-year long cert expirations, increasing the danger when a cert was compromised.
// 3. Forgot to renew their certs when they expired, leading to outages.
//
// At worst, they still serve as reasonable
// _approximations_ of the actual values.
//
// [Source](source)
// ''',
//               styleSheet: MarkdownStyleSheet(
//                 pPadding: const EdgeInsets.only(top: 0.0, bottom: Space.s200),
//                 h1Padding:
//                     const EdgeInsets.only(top: Space.s400, bottom: Space.s200),
//                 h2Padding:
//                     const EdgeInsets.only(top: Space.s400, bottom: Space.s200),
//               ),
//             ),
//           ],
//         ),
//       ),
//     );
//   }
// }
