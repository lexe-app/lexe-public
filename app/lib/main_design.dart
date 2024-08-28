// An alternate application entrypoint specifically for designing pages
// and components in isolation, without actually touching any real backends.

import 'dart:async';
import 'dart:typed_data' show Uint8List;

import 'package:app_rs_dart/app_rs_dart.dart' as app_rs_dart;
import 'package:app_rs_dart/ffi/api.dart'
    show Balance, FeeEstimate, FiatRate, PreflightPayOnchainResponse;
import 'package:app_rs_dart/ffi/app.dart' show U8Array32;
import 'package:app_rs_dart/ffi/types.dart'
    show
        ClientPaymentId,
        Config,
        Onchain,
        Payment,
        PaymentMethod,
        PaymentStatus;
import 'package:app_rs_dart/ffi/types.ext.dart' show PaymentExt;
import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart' show MarkdownBody;
import 'package:intl/intl.dart' show Intl;
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
import 'package:lexeapp/design_mode/mocks.dart' as mocks;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth, GDriveServerAuthCode;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/landing.dart' show LandingPage;
import 'package:lexeapp/route/payment_detail.dart' show PaymentDetailPageInner;
import 'package:lexeapp/route/receive.dart'
    show LnInvoiceInputs, ReceivePaymentEditInvoicePage, ReceivePaymentPage;
import 'package:lexeapp/route/restore.dart';
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
    show SignupBackupPasswordPage, SignupPage;
import 'package:lexeapp/route/wallet.dart' show WalletPage;
import 'package:lexeapp/settings.dart' show LxSettings;
import 'package:lexeapp/stream_ext.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxTheme, Space;
import 'package:lexeapp/uri_events.dart' show UriEvents;
import 'package:rxdart_ext/rxdart_ext.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // Init native Rust ffi bindings.
  await app_rs_dart.init();

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
    final mockApp = mocks.MockAppHandle();
    final mockSignupApi = mocks.MockSignupApi(app: mockApp);
    final mockRestoreApi = mocks.MockRestoreApi(app: mockApp);

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
                gdriveAuth: GDriveAuth.mock,
                signupApi: mockSignupApi,
                restoreApi: mockRestoreApi,
                uriEvents: this.widget.uriEvents,
              ),
            ),
            Component(
              "SignupPage (mock gdrive)",
              (context) => SignupPage(
                config: widget.config,
                gdriveAuth: GDriveAuth.mock,
                signupApi: mockSignupApi,
              ),
            ),
            Component(
              "SignupPage (real gdrive)",
              (context) => SignupPage(
                config: widget.config,
                gdriveAuth: GDriveAuth.prod,
                signupApi: mockSignupApi,
              ),
            ),
            Component(
              "SignupBackupPasswordPage",
              (context) => SignupBackupPasswordPage(
                config: widget.config,
                authInfo: const GDriveServerAuthCode(serverAuthCode: "fake"),
                signupApi: mockSignupApi,
              ),
            ),
            Component(
              "RestorePage (mock gdrive)",
              (context) => RestorePage(
                config: widget.config,
                gdriveAuth: GDriveAuth.mock,
                restoreApi: mockRestoreApi,
              ),
            ),
            Component(
              "RestorePage (real gdrive)",
              (context) => RestorePage(
                config: widget.config,
                gdriveAuth: GDriveAuth.prod,
                restoreApi: mockRestoreApi,
              ),
            ),
            Component(
              "WalletPage",
              (_) => WalletPage(
                app: mockApp,
                settings: LxSettings(mockApp.settingsDb()),
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
                app: mocks.MockAppHandleErroring(),
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
                payment: ValueNotifier(mocks.dummyOnchainOutboundFailed01),
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
                payment: ValueNotifier(mocks.dummyOnchainInboundCompleted01),
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
                payment: this
                    .makeCompletingPayment(mocks.dummyInvoiceInboundPending01),
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
            Component(
              "Markdown",
              (context) => const MarkdownPage(),
            ),
            const SizedBox(height: Space.s800),
          ],
        ),
      ),
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

class MarkdownPage extends StatelessWidget {
  const MarkdownPage({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leading: const LxBackButton(isLeading: true),
        leadingWidth: Space.appBarLeadingWidth,
      ),
      body: Theme(
        data: LxTheme.light(),
        child: ScrollableSinglePageBody(
          body: [
            MarkdownBody(
              data: '''
# ACME protocol

ACME is a protocol for automated provisioning of **web PKI certificates**, which are
just certs bound to public domains and endorsed by web root CAs like Let's Encrypt.

## Why

Before ACME, web service operators had to *manually update their web certs*.
This error prone and time-consuming process meant that many sites either:

1. Didn't use HTTPS at all, reducing security.
2. Used multi-year long cert expirations, increasing the danger when a cert was compromised.
3. Forgot to renew their certs when they expired, leading to outages.

At worst, they still serve as reasonable
_approximations_ of the actual values.

[Source](source)
''',
              styleSheet: LxTheme.buildMarkdownStyle(),
            ),
          ],
        ),
      ),
    );
  }
}
