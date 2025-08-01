// An alternate application entrypoint specifically for designing pages
// and components in isolation, without actually touching any real backends.

import 'dart:async' show unawaited;
import 'dart:convert' show utf8;

import 'package:app_rs_dart/app_rs_dart.dart' as app_rs_dart;
import 'package:app_rs_dart/ffi/api.dart'
    show
        FeeEstimate,
        FiatRate,
        ListChannelsResponse,
        NodeInfo,
        PreflightCloseChannelResponse,
        PreflightOpenChannelResponse,
        PreflightPayInvoiceResponse,
        PreflightPayOnchainResponse;
import 'package:app_rs_dart/ffi/app.dart' show U8Array16, U8Array32;
import 'package:app_rs_dart/ffi/types.dart'
    show
        AppUserInfo,
        ClientPaymentId,
        Config,
        Invoice,
        Onchain,
        Payment,
        PaymentMethod,
        PaymentStatus,
        UserChannelId;
import 'package:app_rs_dart/ffi/types.ext.dart' show PaymentExt;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart' show MarkdownBody;
import 'package:intl/intl.dart' show Intl;
import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/components.dart'
    show
        ErrorMessage,
        ErrorMessageSection,
        FilledTextPlaceholder,
        HeadingText,
        LoadingSpinnerModal,
        LxBackButton,
        LxFilledButton,
        LxOutlinedButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SplitAmountText,
        SubheadingText,
        showModalAsyncFlow;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/design_mode/mocks.dart' as mocks;
import 'package:lexeapp/feature_flags.dart' show FeatureFlags;
import 'package:lexeapp/gdrive_auth.dart'
    show
        GDriveAuth,
        GDriveServerAuthCode,
        MockGDriveRestoreCandidate,
        MockRootSeed;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/notifier_ext.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/channels.dart'
    show ChannelBalanceBarRow, ChannelButton, ChannelsList, ChannelsPage;
import 'package:lexeapp/route/clients.dart' show ClientsPage;
import 'package:lexeapp/route/close_channel.dart'
    show CloseChannelConfirmPage, CloseChannelPage;
import 'package:lexeapp/route/landing.dart' show LandingPage;
import 'package:lexeapp/route/node_info.dart' show NodeInfoPage;
import 'package:lexeapp/route/open_channel.dart'
    show OpenChannelConfirmPage, OpenChannelPage;
import 'package:lexeapp/route/payment_detail.dart' show PaymentDetailPageInner;
import 'package:lexeapp/route/receive/page.dart'
    show ReceivePaymentEditInvoicePage, ReceivePaymentPage;
import 'package:lexeapp/route/receive/state.dart' show LnInvoiceInputs;
import 'package:lexeapp/route/restore.dart'
    show RestoreChooseWalletPage, RestorePage, RestorePasswordPage;
import 'package:lexeapp/route/scan.dart' show ScanPage;
import 'package:lexeapp/route/send/page.dart' show SendPaymentPage;
import 'package:lexeapp/route/send/state.dart'
    show
        PreflightedPayment_Invoice,
        PreflightedPayment_Onchain,
        SendFlowResult,
        SendState_NeedAmount,
        SendState_NeedUri,
        SendState_Preflighted;
import 'package:lexeapp/route/show_qr.dart' show ShowQrPage;
import 'package:lexeapp/route/signup.dart'
    show
        SignupBackupPasswordPage,
        SignupBackupSeedConfirmPage,
        SignupBackupSeedPage,
        SignupCtx,
        SignupPage;
import 'package:lexeapp/route/wallet.dart' show WalletActionButton, WalletPage;
import 'package:lexeapp/save_file.dart' as save_file;
import 'package:lexeapp/service/node_info.dart';
import 'package:lexeapp/settings.dart' show LxSettings;
import 'package:lexeapp/stream_ext.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxRadius, LxTheme, Space;
import 'package:lexeapp/types.dart' show BalanceState;
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

  final userAgent = await cfg.UserAgent.fromPlatform();
  final Config config = await cfg.buildTest(userAgent: userAgent);
  info("Test build config: $config");

  final uriEvents = await UriEvents.prod();

  runApp(
    MaterialApp(
      title: "Lexe App - Design Mode",
      color: LxColors.background,
      themeMode: ThemeMode.light,
      theme: LxTheme.light(),
      darkTheme: null,
      debugShowCheckedModeBanner: false,
      home: LexeDesignPage(config: config, uriEvents: uriEvents),
    ),
  );
}

class LexeDesignPage extends StatefulWidget {
  const LexeDesignPage({
    super.key,
    required this.config,
    required this.uriEvents,
  });

  final Config config;
  final UriEvents uriEvents;

  @override
  State<LexeDesignPage> createState() => _LexeDesignPageState();
}

class _LexeDesignPageState extends State<LexeDesignPage> {
  // When this stream ticks, all the payments' createdAt label should update.
  // This stream ticks every 30 seconds.
  final DateTimeNotifier paymentDateUpdates = DateTimeNotifier(
    period: const Duration(seconds: 30),
  );

  @override
  void dispose() {
    this.paymentDateUpdates.dispose();
    super.dispose();
  }

  ValueListenable<FiatRate?> makeFiatRateStream() =>
      Stream.fromIterable(<FiatRate?>[
            const FiatRate(fiat: "USD", rate: 97111.19),
            const FiatRate(fiat: "USD", rate: 97222.29),
            const FiatRate(fiat: "USD", rate: 97333.39),
          ])
          .interval(const Duration(seconds: 2))
          .shareValueSeeded(null)
          .streamValueNotifier();

  /// Complete the payment after a few seconds
  ValueNotifier<Payment> makeCompletingPayment(final Payment payment) {
    final notifier = ValueNotifier(payment);

    unawaited(
      Future.delayed(const Duration(seconds: 4), () {
        final p = notifier.value;
        notifier.value = p.copyWith(
          status: PaymentStatus.completed,
          statusStr: "completed",
          finalizedAt: DateTime.now().millisecondsSinceEpoch,
        );
      }),
    );

    return notifier;
  }

  @override
  Widget build(BuildContext context) {
    final mockApp = mocks.MockAppHandle(
      balance: mocks.balanceDefault,
      payments: mocks.defaultDummyPayments,
      channels: mocks.defaultDummyChannels,
    );
    final mockAppErr = mocks.MockAppHandleErr(
      balance: mocks.balanceDefault,
      payments: mocks.defaultDummyPayments,
      channels: mocks.defaultDummyChannels,
    );
    final mockSignupApi = mocks.MockSignupApi(app: mockApp);
    const mockSignupApiErr = mocks.MockSignupApiErr();
    final mockRestoreApi = mocks.MockRestoreApi(app: mockApp);
    const mockRootSeed = MockRootSeed();
    final mockSignupCtx = SignupCtx(
      this.widget.config,
      mockRootSeed,
      GDriveAuth.mock,
      mockSignupApi,
    );
    final mockSignupCtxErr = SignupCtx(
      this.widget.config,
      mockRootSeed,
      GDriveAuth.mockError,
      mockSignupApiErr,
    );

    final cidBytes = List.generate(32, (idx) => idx);
    final cid = ClientPaymentId(id: U8Array32(Uint8List.fromList(cidBytes)));

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
                rootSeed: mockRootSeed,
                gdriveAuth: GDriveAuth.mock,
                signupApi: mockSignupApi,
                restoreApi: mockRestoreApi,
                uriEvents: this.widget.uriEvents,
                fixedShaderTime: null,
              ),
            ),
            Component(
              "SignupPage (mock gdrive)",
              (context) => SignupPage(ctx: mockSignupCtx),
            ),
            Component(
              "SignupPage (mock gdrive error)",
              (context) => SignupPage(ctx: mockSignupCtxErr),
            ),
            Component(
              "SignupPage (real gdrive)",
              (context) => SignupPage(
                ctx: SignupCtx(
                  this.widget.config,
                  mockRootSeed,
                  GDriveAuth.prod,
                  mockSignupApi,
                ),
              ),
            ),
            Component(
              "SignupBackupPasswordPage",
              (context) => SignupBackupPasswordPage(
                ctx: mockSignupCtx,
                authInfo: const GDriveServerAuthCode(serverAuthCode: "fake"),
                signupCode: null,
              ),
            ),
            Component(
              "SignupBackupPasswordPage",
              subtitle: "signup error",
              (context) => SignupBackupPasswordPage(
                ctx: mockSignupCtxErr,
                authInfo: const GDriveServerAuthCode(serverAuthCode: "fake"),
                signupCode: null,
              ),
            ),
            Component(
              "SignupBackupSeedConfirmPage",
              (context) => const SignupBackupSeedConfirmPage(),
            ),
            Component(
              "SignupBackupSeedPage",
              (context) => SignupBackupSeedPage(
                ctx: mockSignupCtx,
                seedWords: mocks.seedWords1,
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
              "RestoreChooseWalletPage",
              (context) => RestoreChooseWalletPage(
                candidates: const [
                  MockGDriveRestoreCandidate(
                    userPk:
                        "4072836db6c62f1fd07281feb1f2d6d1b8f05f8be3f0019a9205edff244017f1",
                  ),
                  MockGDriveRestoreCandidate(
                    userPk:
                        "ef64652cc9fc1d79d174bb52d0ffb7ad365db842e72e056aa5c4bfe00bcb20da",
                  ),
                ],
                serverAuthCode: const GDriveServerAuthCode(
                  serverAuthCode: "fake",
                ),
                config: widget.config,
                restoreApi: mockRestoreApi,
              ),
            ),
            Component(
              "RestorePasswordPage",
              (context) => RestorePasswordPage(
                candidate: const MockGDriveRestoreCandidate(
                  userPk:
                      "ef64652cc9fc1d79d174bb52d0ffb7ad365db842e72e056aa5c4bfe00bcb20da",
                ),
                serverAuthCode: const GDriveServerAuthCode(
                  serverAuthCode: "fake",
                ),
                config: widget.config,
                restoreApi: mockRestoreApi,
              ),
            ),
            Component(
              "WalletPage",
              (_) => WalletPage(
                config: widget.config,
                app: mockApp,
                settings: LxSettings(mockApp.settingsDb()),
                featureFlags: const FeatureFlags.all(),
                uriEvents: this.widget.uriEvents,
                gdriveAuth: GDriveAuth.mock,
              ),
            ),
            Component(
              "WalletPage",
              subtitle: "fresh wallet with no payments",
              (_) => WalletPage(
                config: widget.config,
                app: mocks.MockAppHandle(
                  payments: [],
                  channels: [],
                  balance: mocks.balanceZero,
                ),
                settings: LxSettings(mockApp.settingsDb()),
                featureFlags: const FeatureFlags.all(),
                uriEvents: this.widget.uriEvents,
                gdriveAuth: GDriveAuth.mock,
              ),
            ),
            Component(
              "WalletPage",
              subtitle: "on-chain-only wallet",
              (_) => WalletPage(
                config: widget.config,
                app: mocks.MockAppHandle(
                  payments: [mocks.dummyOnchainInboundCompleted01],
                  channels: [],
                  balance: mocks.balanceOnchainOnly,
                ),
                settings: LxSettings(mockApp.settingsDb()),
                featureFlags: const FeatureFlags.all(),
                uriEvents: this.widget.uriEvents,
                gdriveAuth: GDriveAuth.mock,
              ),
            ),
            Component(
              "SendPaymentNeedUriPage",
              (context) => SendPaymentPage(
                startNewFlow: true,
                sendCtx: SendState_NeedUri(
                  app: mockApp,
                  configNetwork: widget.config.network,
                  balance: mockApp.balance,
                  cid: cid,
                ),
              ),
            ),
            Component(
              "SendPaymentAmountPage",
              subtitle: "onchain",
              (context) => SendPaymentPage(
                startNewFlow: true,
                sendCtx: SendState_NeedAmount(
                  app: mockApp,
                  configNetwork: widget.config.network,
                  balance: mockApp.balance,
                  cid: cid,
                  paymentMethod: const PaymentMethod.onchain(
                    Onchain(
                      address: "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
                    ),
                  ),
                ),
              ),
            ),
            Component(
              "SendPaymentAmountPage",
              subtitle: "invoice",
              (context) => SendPaymentPage(
                startNewFlow: true,
                sendCtx: SendState_NeedAmount(
                  app: mockApp,
                  configNetwork: widget.config.network,
                  balance: mockApp.balance,
                  cid: cid,
                  paymentMethod: const PaymentMethod.invoice(
                    Invoice(
                      string:
                          "lnbcrt1qqp4ydsdq22dhxzcmtwvpp5kv0433rmqrm6rj9r70dv4z5w3vyfdda97lzacf2z2ue06tdrz45ssp54jrpc79t9myqyywfslvr5f94tt938xpxcvm8hzu7hc7275lq9stq9qyysgqcqpcxq9p4yd3l05qyptltyujph97g7t9yw6exnlxce76uk9qcqq7h2hdp28qagh9cc77fn6vhukccvr8hedgmq0y6r84vusrsz3z86d4ty2scldj3eqq3mm4ln",
                      createdAt: 1741232485000,
                      expiresAt: 1741233485000,
                      payeePubkey:
                          "28157d6ca3555a0a3275817d0832c535955b28b20a55f9596f6873434feebfd797d4b245397fab8f8f94dcdd32aac475d64893aa042f18b8d725e116082ae909",
                    ),
                  ),
                ),
              ),
            ),
            Component(
              "SendPaymentAmountPage",
              subtitle: "onchain (preflight error)",
              (context) => SendPaymentPage(
                startNewFlow: true,
                sendCtx: SendState_NeedAmount(
                  app: mockAppErr,
                  configNetwork: widget.config.network,
                  balance: mockApp.balance,
                  cid: cid,
                  paymentMethod: const PaymentMethod.onchain(
                    Onchain(
                      address: "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
                    ),
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
                  balance: mockApp.balance,
                  cid: cid,
                  preflightedPayment: const PreflightedPayment_Onchain(
                    onchain: Onchain(
                      address: "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
                    ),
                    preflight: feeEstimates,
                    amountSats: 2500,
                  ),
                ),
              ),
            ),
            Component(
              "SendPaymentConfirmPage",
              subtitle: "pay invoice error",
              (context) => SendPaymentPage(
                startNewFlow: true,
                sendCtx: SendState_Preflighted(
                  app: mockAppErr,
                  configNetwork: widget.config.network,
                  balance: mockApp.balance,
                  cid: cid,
                  preflightedPayment: (() {
                    final invoice =
                        mocks.dummyInvoiceOutboundPending01.invoice!;
                    final amountSats = invoice.amountSats!;
                    return PreflightedPayment_Invoice(
                      invoice: invoice,
                      preflight: PreflightPayInvoiceResponse(
                        amountSats: amountSats,
                        feesSats: (0.0095 * amountSats).truncate(),
                      ),
                      amountSats: amountSats,
                    );
                  })(),
                ),
              ),
            ),
            Component(
              "ReceivePaymentPage",
              (context) => ReceivePaymentPage(
                app: mockApp,
                featureFlags: const FeatureFlags.all(),
                fiatRate: this.makeFiatRateStream(),
              ),
            ),
            Component(
              "ReceivePaymentPage",
              subtitle: "BOLT12 offers feature disabled",
              (context) => ReceivePaymentPage(
                app: mockApp,
                featureFlags: const FeatureFlags.all(
                  showBolt12OffersRecvPage: false,
                ),
                fiatRate: this.makeFiatRateStream(),
              ),
            ),
            Component(
              "ReceivePaymentPage",
              subtitle: "fetch invoice error",
              (context) => ReceivePaymentPage(
                app: mockAppErr,
                featureFlags: const FeatureFlags.all(),
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
                isSyncing: ValueNotifier(false),
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
                isSyncing: ValueNotifier(false),
                triggerRefresh: () {},
              ),
            ),
            Component(
              "PaymentDetailPage",
              subtitle: "ln invoice pending inbound",
              (context) => PaymentDetailPageInner(
                app: mockApp,
                payment: this.makeCompletingPayment(
                  mocks.dummyInvoiceInboundPending01,
                ),
                paymentDateUpdates: this.paymentDateUpdates,
                fiatRate: this.makeFiatRateStream(),
                isSyncing: ValueNotifier(false),
                triggerRefresh: () {},
              ),
            ),
            Component(
              "PaymentDetailPage",
              subtitle: "ln offer completed outbound",
              (context) => PaymentDetailPageInner(
                app: mockApp,
                payment: ValueNotifier(mocks.dummyOfferOutboundPayment01),
                paymentDateUpdates: this.paymentDateUpdates,
                fiatRate: this.makeFiatRateStream(),
                isSyncing: ValueNotifier(false),
                triggerRefresh: () {},
              ),
            ),
            Component(
              "PaymentDetailPage",
              subtitle: "ln offer completed inbound",
              (context) => PaymentDetailPageInner(
                app: mockApp,
                payment: ValueNotifier(mocks.dummyOfferInboundPayment01),
                paymentDateUpdates: this.paymentDateUpdates,
                fiatRate: this.makeFiatRateStream(),
                isSyncing: ValueNotifier(false),
                triggerRefresh: () {},
              ),
            ),
            Component("ChannelsPage", (context) {
              // TODO(phlip9): fix issue where fiat rate unsets after hot reload
              final nodeInfoService = NodeInfoService(app: mockApp);
              final fiatRate = this.makeFiatRateStream();
              final balanceState = combine2(
                nodeInfoService.nodeInfo,
                fiatRate,
                (nodeInfo, fiatRate) => BalanceState(
                  balanceSats: nodeInfo?.balance,
                  fiatRate: fiatRate,
                ),
              );
              return ChannelsPage(
                app: mockApp,
                fiatRate: fiatRate,
                nodeInfoService: nodeInfoService,
                balanceState: balanceState,
              );
            }),
            Component(
              "OpenChannelPage",
              (context) => OpenChannelPage(
                app: mockApp,
                balanceState: ValueNotifier(
                  const BalanceState(
                    balanceSats: mocks.balanceOnchainOnly,
                    fiatRate: FiatRate(fiat: "USD", rate: 73111.19),
                  ),
                ),
              ),
            ),
            Component(
              "OpenChannelPage",
              subtitle: "preflight error",
              (context) => OpenChannelPage(
                app: mockAppErr,
                balanceState: ValueNotifier(
                  const BalanceState(
                    balanceSats: mocks.balanceOnchainOnly,
                    fiatRate: FiatRate(fiat: "USD", rate: 73111.19),
                  ),
                ),
              ),
            ),
            Component(
              "OpenChannelConfirmPage",
              (context) => OpenChannelConfirmPage(
                app: mockApp,
                balanceState: ValueNotifier(
                  const BalanceState(
                    balanceSats: mocks.balanceOnchainOnly,
                    fiatRate: FiatRate(fiat: "USD", rate: 73111.19),
                  ),
                ),
                channelValueSats: 6500,
                userChannelId: UserChannelId(id: U8Array16.init()),
                preflight: const PreflightOpenChannelResponse(
                  feeEstimateSats: 122,
                ),
              ),
            ),
            Component(
              "OpenChannelConfirmPage",
              subtitle: "error",
              (context) => OpenChannelConfirmPage(
                app: mockAppErr,
                balanceState: ValueNotifier(
                  const BalanceState(
                    balanceSats: mocks.balanceOnchainOnly,
                    fiatRate: FiatRate(fiat: "USD", rate: 73111.19),
                  ),
                ),
                channelValueSats: 6500,
                userChannelId: UserChannelId(id: U8Array16.init()),
                preflight: const PreflightOpenChannelResponse(
                  feeEstimateSats: 122,
                ),
              ),
            ),
            Component(
              "CloseChannelPage",
              (context) => CloseChannelPage(
                app: mockApp,
                fiatRate: this.makeFiatRateStream(),
                channels: ValueNotifier(
                  ChannelsList.fromApi(
                    ListChannelsResponse(channels: mockApp.channels),
                  ),
                ),
              ),
            ),
            Component(
              "CloseChannelPage",
              subtitle: "preflight error",
              (context) => CloseChannelPage(
                app: mockAppErr,
                fiatRate: this.makeFiatRateStream(),
                channels: ValueNotifier(
                  ChannelsList.fromApi(
                    ListChannelsResponse(channels: mockApp.channels),
                  ),
                ),
              ),
            ),
            Component(
              "CloseChannelConfirmPage",
              (context) => CloseChannelConfirmPage(
                app: mockApp,
                fiatRate: this.makeFiatRateStream(),
                channelId:
                    "2607641588c8a779a6f7e7e2d110b0c67bc1f01b9bb9a89bbe98c144f0f4b04c",
                channelOurBalanceSats: 300231,
                preflight: const PreflightCloseChannelResponse(
                  feeEstimateSats: 1100,
                ),
              ),
            ),
            Component(
              "CloseChannelConfirmPage",
              subtitle: "error",
              (context) => CloseChannelConfirmPage(
                app: mockAppErr,
                fiatRate: this.makeFiatRateStream(),
                channelId:
                    "2607641588c8a779a6f7e7e2d110b0c67bc1f01b9bb9a89bbe98c144f0f4b04c",
                channelOurBalanceSats: 300231,
                preflight: const PreflightCloseChannelResponse(
                  feeEstimateSats: 1100,
                ),
              ),
            ),
            Component(
              "ScanPage",
              (_) => MultistepFlow<SendFlowResult>(
                builder: (_) => ScanPage(
                  sendCtx: SendState_NeedUri(
                    app: mockApp,
                    configNetwork: widget.config.network,
                    balance: mockApp.balance,
                    cid: cid,
                  ),
                ),
              ),
            ),
            Component("NodeInfoPage", (_) {
              final nodeInfo = ValueNotifier<NodeInfo?>(null);
              const userInfo = AppUserInfo(
                userPk:
                    "52b999003525a3d905f9916eff26cee6625a3976fc25270ce5b3e79aa3c16f45",
                nodePk:
                    "024de9a91aaf32588a7b0bb97ba7fad3db22fcfe62a52bc2b2d389c5fa9d946e1b",
                nodePkProof:
                    "024de9a91aaf32588a7b0bb97ba7fad3db22fcfe62a52bc2b2d389c5fa9d946e1b46304402206f762d23d206f3af2ffa452a71a11bca3df68838408851ab77931d7eb7fa1ef6022057141408428d6885d00ca6ca50e6d702aeab227c1550135be5fce4af4e726736",
              );
              unawaited(
                Future.delayed(const Duration(seconds: 1), () {
                  nodeInfo.value = NodeInfo(
                    nodePk: userInfo.nodePk,
                    version: "1.2.3",
                    measurement:
                        "1d97c2c837b09ec7b0e0b26cb6fa9a211be84c8fdb53299cc9ee8884c7a25ac1",
                    balance: mocks.balanceZero,
                  );
                }),
              );
              return NodeInfoPage(nodeInfo: nodeInfo, userInfo: userInfo);
            }),
            Component("SdkClientsPage", (_) => ClientsPage(app: mockApp)),
            Component(
              "SdkClientsPage",
              subtitle: "error",
              (_) => ClientsPage(app: mockAppErr),
            ),
            Component(
              "Screenshot 01",
              subtitle: "LandingPage",
              (context) => LandingPage(
                config: widget.config,
                rootSeed: mockRootSeed,
                gdriveAuth: GDriveAuth.mock,
                signupApi: mockSignupApi,
                restoreApi: mockRestoreApi,
                uriEvents: this.widget.uriEvents,
                fixedShaderTime: 8.5,
              ),
            ),
            Component(
              "Screenshot 02",
              subtitle: "WalletPage",
              (_) => WalletPage(
                config: widget.config,
                app: mocks.MockAppHandleScreenshots(),
                settings: LxSettings(mockApp.settingsDb()),
                featureFlags: const FeatureFlags.all(),
                uriEvents: this.widget.uriEvents,
                gdriveAuth: GDriveAuth.mock,
              ),
            ),
            Component(
              "Screenshot 03",
              subtitle: "ReceivePage (Invoice)",
              (_) => ReceivePaymentPage(
                app: mocks.MockAppHandleScreenshots(),
                featureFlags: const FeatureFlags.all(),
                fiatRate: ValueNotifier(
                  const FiatRate(fiat: "USD", rate: 96626.76),
                ),
              ),
            ),
            Component(
              "Screenshot 04",
              subtitle: "SendPaymentConfirmPage (Invoice)",
              (_) => SendPaymentPage(
                startNewFlow: true,
                sendCtx: SendState_Preflighted(
                  app: mocks.MockAppHandleScreenshots(),
                  configNetwork: widget.config.network,
                  balance: mockApp.balance,
                  cid: cid,
                  preflightedPayment: PreflightedPayment_Invoice(
                    invoice: Invoice(
                      string:
                          mocks.dummyInvoiceOutboundPending01.invoice!.string,
                      createdAt: 1686743442000,
                      expiresAt: 1686745442000,
                      payeePubkey: mocks
                          .dummyInvoiceOutboundPending01
                          .invoice!
                          .payeePubkey,
                      amountSats: 10000,
                    ),
                    preflight: const PreflightPayInvoiceResponse(
                      amountSats: 10000,
                      feesSats: 92,
                    ),
                    amountSats: 10092,
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
                value: "bitcoin:BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4",
              ),
            ),
            Component(
              "ShowQrPage",
              subtitle: "unified bolt 12",
              (_) => const ShowQrPage(
                value:
                    "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday&lightning=LNBC10U1P3PJ257PP5YZTKWJCZ5FTL5LAXKAV23ZMZEKAW37ZK6KMV80PK4XAEV5QHTZ7QDPDWD3XGER9WD5KWM36YPRX7U3QD36KUCMGYP282ETNV3SHJCQZPGXQYZ5VQSP5USYC4LK9CHSFP53KVCNVQ456GANH60D89REYKDNGSMTJ6YW3NHVQ9QYYSSQJCEWM5CJWZ4A6RFJX77C490YCED6PEMK0UPKXHY89CMM7SCT66K8GNEANWYKZGDRWRFJE69H9U5U0W57RRCSYSAS7GADWMZXC8C6T0SPJAZUP6",
              ),
            ),
            Component("Buttons", (_) => const ButtonDesignPage()),
            Component(
              "ModalAsyncFlow",
              (_) => const ModalAsyncFlowDesignPage(),
            ),
            Component("Markdown", (context) => const MarkdownPage()),
            Component(
              "SplitAmountText",
              (context) => const SplitAmountTextPage(),
            ),
            Component(
              "FilledTextPlaceholder",
              (context) => const FilledTextPlaceholderPage(),
            ),
            Component(
              "ChannelBalanceBarRow",
              (context) => const ChannelBalanceBarRowPage(),
            ),
            Component(
              "ErrorMessageSection",
              (context) => const ErrorMessageSectionPage(),
            ),
            Component("SaveFile", (context) => const SaveFilePage()),
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
        Navigator.of(context).push(MaterialPageRoute(builder: this.builder));
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
            const LxFilledButton(onTap: onTap, label: Text("Send")),
            const SizedBox(height: Space.s400),

            // disabled
            const LxFilledButton(onTap: null, label: Text("Send")),
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
                  child: LxOutlinedButton(onTap: onTap, label: Text("Cancel")),
                ),
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
                  child: LxOutlinedButton(onTap: onTap, label: Text("Cancel")),
                ),
                SizedBox(width: Space.s200),
                Expanded(
                  child: LxOutlinedButton(onTap: onTap, label: Text("Skip")),
                ),
                SizedBox(width: Space.s200),
                Expanded(
                  child: LxFilledButton(onTap: onTap, label: Text("Next")),
                ),
              ],
            ),
            const SizedBox(height: Space.s400),

            //
            // WalletActionButton
            //
            const HeadingText(text: "Wallet action buttons"),
            const SizedBox(height: Space.s400),

            const Row(
              mainAxisAlignment: MainAxisAlignment.center,
              spacing: Space.s400,
              children: [
                WalletActionButton(
                  onPressed: onTap,
                  icon: LxIcons.scan,
                  label: "Scan",
                ),
                WalletActionButton(
                  onPressed: onTap,
                  icon: LxIcons.receive,
                  label: "Receive",
                ),
                WalletActionButton(
                  onPressed: onTap,
                  icon: LxIcons.send,
                  label: "Send",
                ),
                // Builder(builder: (context) {
                //   info("IconTheme.of(context): ${IconTheme.of(context)}");
                //   return Text("foo");
                // }),
              ],
            ),
            const SizedBox(height: Space.s600),

            //
            // Channel open/close buttons
            //
            const HeadingText(text: "Channel buttons"),
            const SizedBox(height: Space.s400),

            const Row(
              mainAxisAlignment: MainAxisAlignment.center,
              spacing: Space.s400,
              children: [
                ChannelButton(
                  onPressed: onTap,
                  label: "Open",
                  icon: LxIcons.openChannel,
                ),
                ChannelButton(
                  onPressed: onTap,
                  label: "Close",
                  icon: LxIcons.closeChannel,
                ),
              ],
            ),
            const SizedBox(height: Space.s600),

            //
            // ReceivePage buttons
            //
            const HeadingText(text: "Receive page buttons"),
            const SizedBox(height: Space.s400),

            Container(
              decoration: BoxDecoration(
                color: LxColors.grey1000,
                borderRadius: BorderRadius.circular(LxRadius.r300),
              ),
              padding: const EdgeInsets.symmetric(vertical: Space.s400),
              child: const Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  OutlinedButton(
                    onPressed: onTap,
                    style: ButtonStyle(
                      visualDensity: VisualDensity(
                        horizontal: -3.0,
                        vertical: -3.0,
                      ),
                    ),
                    child: Row(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        SizedBox(width: Space.s200),
                        Icon(LxIcons.add),
                        SizedBox(width: Space.s200),
                        Text(
                          "Amount",
                          style: TextStyle(fontSize: Fonts.size300),
                        ),
                        SizedBox(width: Space.s400),
                      ],
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(height: Space.s600),

            const Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                // Copy code
                Padding(
                  padding: EdgeInsets.symmetric(horizontal: Space.s200),
                  child: FilledButton(
                    onPressed: onTap,
                    child: Icon(LxIcons.copy),
                  ),
                ),

                // Share payment URI (w/ share code fallback)
                Padding(
                  padding: EdgeInsets.symmetric(horizontal: Space.s200),
                  child: FilledButton(
                    onPressed: onTap,
                    child: Icon(LxIcons.share),
                  ),
                ),

                // Refresh
                Padding(
                  padding: EdgeInsets.symmetric(horizontal: Space.s200),
                  child: FilledButton(
                    onPressed: onTap,
                    child: Icon(LxIcons.refresh),
                  ),
                ),
              ],
            ),
            const SizedBox(height: Space.s600),

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
          "W/WindowOnBackDispatcher(26148): Set 'android:enableOnBackInvokedCallback=\"true\"' in the application manifest.",
        ),
      ),
      errorBuilder: (context, err) => AlertDialog(
        title: const Text("Issue with payment"),
        content: Text(err),
        scrollable: true,
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

/// View [SplitAmountText] with various currencies, locales, and values.
class SplitAmountTextPage extends StatelessWidget {
  const SplitAmountTextPage({super.key});

  @override
  Widget build(BuildContext context) {
    final TextStyle style = Fonts.fontUI.copyWith(
      fontSize: Fonts.size600,
      color: LxColors.foreground,
      fontVariations: [Fonts.weightMedium],
      fontFeatures: [Fonts.featTabularNumbers],
      // fontFeatures: [Fonts.featDisambugation],
    );

    // const double amount = 0.0;
    // const double amount = 3.50;
    // const double amount = 10463;
    const double amount = 1801.96;
    // const double amount = 255.01;

    Widget forLocaleFiat(String locale, String fiatName) => Padding(
      padding: const EdgeInsets.only(bottom: Space.s100),
      child: SplitAmountText(
        amount: amount,
        fiatName: fiatName,
        style: style,
        locale: locale,
      ),
    );

    Widget forLocale(String locale) => Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.end,
      children: [
        Align(
          alignment: Alignment.topLeft,
          child: HeadingText(text: locale),
        ),
        //
        forLocaleFiat(locale, "USD"),
        forLocaleFiat(locale, "EUR"),
        forLocaleFiat(locale, "MXN"),
        forLocaleFiat(locale, "RUB"),
        const SizedBox(height: Space.s100),
        //
        forLocaleFiat(locale, "ETB"),
        forLocaleFiat(locale, "DKK"),
        const SizedBox(height: Space.s100),
        //
        forLocaleFiat(locale, "JPY"),
        forLocaleFiat(locale, "KRW"),
        const SizedBox(height: Space.s300),
      ],
    );

    return Scaffold(
      appBar: AppBar(
        leading: const LxBackButton(isLeading: true),
        leadingWidth: Space.appBarLeadingWidth,
      ),
      body: Theme(
        data: LxTheme.light(),
        child: ScrollableSinglePageBody(
          body: [
            const HeadingText(text: "SplitAmountText"),
            const SubheadingText(text: "Listed by locale"),
            const SizedBox(height: Space.s400),
            forLocale("en_US"),
            forLocale("fr_FR"),
            forLocale("nb"),
            forLocale("ja"),
            forLocale("ru"),
            forLocale("am"),
            forLocale("th"),
            forLocale("hi"),
            forLocale("es_MX"),
          ],
        ),
      ),
    );
  }
}

class FilledTextPlaceholderPage extends StatelessWidget {
  const FilledTextPlaceholderPage({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leading: const LxBackButton(isLeading: true),
        leadingWidth: Space.appBarLeadingWidth,
      ),
      body: Theme(
        data: LxTheme.light(),
        child: const ScrollableSinglePageBody(
          body: [
            HeadingText(text: "FilledTextPlaceholder"),
            SubheadingText(text: "Align by baseline should work"),
            SizedBox(height: Space.s700),
            Row(
              mainAxisSize: MainAxisSize.min,
              mainAxisAlignment: MainAxisAlignment.start,
              textBaseline: TextBaseline.alphabetic,
              crossAxisAlignment: CrossAxisAlignment.baseline,
              children: [
                Text("Kjg", style: Fonts.fontUI),
                SizedBox(width: Space.s200),
                FilledTextPlaceholder(width: Space.s700, style: Fonts.fontUI),
                SizedBox(width: Space.s200),
                FilledTextPlaceholder(width: Space.s900, style: Fonts.fontLogo),
                SizedBox(width: Space.s200),
                Text("Ref", style: Fonts.fontLogo),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

/// Debug the [ChannelBalanceBarRow] widget for various values and widths
class ChannelBalanceBarRowPage extends StatelessWidget {
  const ChannelBalanceBarRowPage({super.key});

  @override
  Widget build(BuildContext context) {
    const values = [0.00, 0.01, 0.05, 0.25, 0.50, 0.75, 0.95, 0.99, 1.00];
    const widths = [0.00, 0.01, 0.05, 0.25, 0.50, 0.75, 0.95, 0.99, 1.00];

    List<Widget> channelBars = [];
    for (final isUsable in [true, false]) {
      for (final width in widths) {
        channelBars.add(
          Padding(
            padding: const EdgeInsets.symmetric(vertical: Space.s100),
            child: Text("width = $width"),
          ),
        );

        for (final value in values) {
          channelBars.add(
            Padding(
              padding: const EdgeInsets.symmetric(vertical: Space.s100),
              child: ChannelBalanceBarRow(
                value: value,
                width: width,
                isUsable: isUsable,
              ),
            ),
          );
        }
        channelBars.add(const SizedBox(height: Space.s400));
      }
    }

    return Scaffold(
      appBar: AppBar(
        leading: const LxBackButton(isLeading: true),
        leadingWidth: Space.appBarLeadingWidth,
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "ChannelBalanceBarPage"),
          const SubheadingText(
            text:
                "Ensure channel balance bars look good at all values and sizes",
          ),
          const SizedBox(height: Space.s700),
          const Padding(
            padding: EdgeInsets.symmetric(vertical: Space.s200),
            child: ChannelBalanceBarRow(value: 0.5, width: 1.0, isUsable: true),
          ),
          const Padding(
            padding: EdgeInsets.symmetric(vertical: Space.s200),
            child: ChannelBalanceBarRow(value: 0.5, width: 0.5, isUsable: true),
          ),
          const SizedBox(height: Space.s400),
          ...channelBars,
        ],
      ),
    );
  }
}

class ErrorMessageSectionPage extends StatefulWidget {
  const ErrorMessageSectionPage({super.key});

  @override
  State<ErrorMessageSectionPage> createState() =>
      _ErrorMessageSectionPageState();
}

class _ErrorMessageSectionPageState extends State<ErrorMessageSectionPage> {
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.errorMessage.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    const short1 = ErrorMessage(
      title: "We couldn't find any Lexe Wallet backups for this account",
    );
    const short2 = ErrorMessage(message: "Unrecognized payment code");
    const long1 = ErrorMessage(
      title: "Failed to send payment",
      message:
          "Could not find route to recipient\n\nCaused by:\n  1. Failed to find a path to the given destination",
    );
    const long2 = ErrorMessage(
      title: "There was an error connecting your Google Drive",
      message:
          "Auth code exchange failed\n\nCaused by:\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off\n  1. stacktrace error gets cut off",
    );
    const long3 = ErrorMessage(
      title: "Failed to send payment",
      message:
          "[106=Command] Already tried to pay this invoice: Error handling new payment: Payment already exists: finalized",
    );
    // Test overflow checking at boundaries (this is device specific atm)
    const title1 = ErrorMessage(
      title:
          "123456789012345678901234567890123456789012345678901234567890123456789012345",
    );
    const message1 = ErrorMessage(
      message: "12345678901234567890123456789012345678901234",
    );

    Widget testButton(String name, ErrorMessage? error) => Expanded(
      child: LxFilledButton(
        onTap: () => this.errorMessage.value = error,
        label: Text(name),
      ),
    );

    return Scaffold(
      appBar: AppBar(
        leading: const LxBackButton(isLeading: true),
        leadingWidth: Space.appBarLeadingWidth,
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "ErrorMessageSectionPage"),
          const SubheadingText(text: "Design error message display"),
          const SizedBox(height: Space.s600),
          ValueListenableBuilder(
            valueListenable: this.errorMessage,
            builder: (context, errorMessage, _child) =>
                ErrorMessageSection(errorMessage),
          ),
        ],
        bottom: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Row(
              children: [
                testButton("None", null),
                const SizedBox(width: Space.s200),
                testButton("Short1", short1),
                const SizedBox(width: Space.s200),
                testButton("Short2", short2),
              ],
            ),
            const SizedBox(height: Space.s200),
            Row(
              children: [
                testButton("Long1", long1),
                const SizedBox(width: Space.s200),
                testButton("Long2", long2),
                const SizedBox(width: Space.s200),
                testButton("Long3", long3),
              ],
            ),
            const SizedBox(height: Space.s200),
            Row(
              children: [
                testButton("Message1", message1),
                const SizedBox(width: Space.s200),
                testButton("Title1", title1),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

/// Test the [save_file.openDialog] function
class SaveFilePage extends StatelessWidget {
  const SaveFilePage({super.key});

  Future<void> saveFile(BuildContext context) async {
    info("Saving file...");

    const dataStr = '{"foo": "bar"}';
    final res = await save_file.openDialog(
      filename: "foo.json",
      data: utf8.encode(dataStr),
    );

    info("File save result: $res");
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

              // Save file button
              LxOutlinedButton(
                onTap: () => this.saveFile(context),
                label: const Text("Save file"),
              ),
              const SizedBox(height: Space.s400),
            ],
          ),
        ),
      ),
    );
  }
}
