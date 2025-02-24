// The primary wallet page.

import 'dart:async'
    show StreamSubscription, TimeoutException, scheduleMicrotask;
import 'dart:math' as math;

import 'package:app_rs_dart/ffi/api.dart' show FiatRate, NodeInfo;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/settings.dart' show Settings;
import 'package:app_rs_dart/ffi/types.dart'
    show
        ClientPaymentId,
        Config,
        PaymentDirection,
        PaymentIndex,
        PaymentKind,
        PaymentStatus,
        ShortPayment,
        ShortPaymentAndIndex;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:intl/intl.dart' show NumberFormat;
import 'package:lexeapp/cfg.dart' show UserAgent;
import 'package:lexeapp/components.dart'
    show
        FilledTextPlaceholder,
        ListIcon,
        LxRefreshButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SplitAmountText,
        SubBalanceRow,
        showModalAsyncFlow;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/notifier_ext.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/channels.dart' show ChannelsPage;
import 'package:lexeapp/route/debug.dart' show DebugPage;
import 'package:lexeapp/route/node_info.dart' show NodeInfoPage;
import 'package:lexeapp/route/payment_detail.dart'
    show PaymentDetailPage, PaymentSource;
import 'package:lexeapp/route/receive.dart' show ReceivePaymentPage;
import 'package:lexeapp/route/scan.dart' show ScanPage;
import 'package:lexeapp/route/send/page.dart' show SendPaymentPage;
import 'package:lexeapp/route/send/state.dart'
    show SendFlowResult, SendState, SendState_NeedUri;
import 'package:lexeapp/service/fiat_rates.dart' show FiatRateService;
import 'package:lexeapp/service/node_info.dart' show NodeInfoService;
import 'package:lexeapp/service/payment_sync.dart' show PaymentSyncService;
import 'package:lexeapp/service/refresh.dart' show RefreshService;
import 'package:lexeapp/settings.dart' show LxSettings;
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxRadius, Space;
import 'package:lexeapp/types.dart' show BalanceKind, BalanceState;
import 'package:lexeapp/uri_events.dart' show UriEvents;

class WalletPage extends StatefulWidget {
  const WalletPage({
    super.key,
    required this.config,
    required this.app,
    required this.settings,
    required this.uriEvents,
  });

  final Config config;
  final AppHandle app;
  final LxSettings settings;
  final UriEvents uriEvents;

  @override
  WalletPageState createState() => WalletPageState();
}

class WalletPageState extends State<WalletPage> {
  final GlobalKey<ScaffoldState> scaffoldKey = GlobalKey();

  /// Manages page refresh state.
  final RefreshService refreshService = RefreshService();

  /// Maintains the fiat exchange rate feed, combined with the user's preferred
  /// fiat as a [Stream<FiatRate>].
  late final FiatRateService fiatRateService;

  /// Sync payments on refresh.
  late final PaymentSyncService paymentSyncService =
      PaymentSyncService(app: this.widget.app);
  late final LxListener paymentSyncOnRefresh;

  /// Fetch [NodeInfo] on refresh.
  late final NodeInfoService nodeInfoService =
      NodeInfoService(app: this.widget.app);
  late final LxListener nodeInfoFetchOnRefresh;

  /// Compute [BalanceState] from [FiatRate] and [NodeInfo] signals.
  late final ComputedValueListenable<BalanceState> balanceState;

  /// When to show refresh loading indicator.
  late final ComputedValueListenable<bool> isRefreshing;

  /// The wallet page listens to URI events. We'll navigate to the right page
  /// after a user scans/taps a bitcoin/lightning URI.
  late StreamSubscription<String> uriEventsListener;

  @override
  void dispose() {
    // Dispose in reverse field order.
    this.uriEventsListener.cancel();
    this.isRefreshing.dispose();
    this.balanceState.dispose();
    this.nodeInfoFetchOnRefresh.dispose();
    this.nodeInfoService.dispose();
    this.paymentSyncOnRefresh.dispose();
    this.paymentSyncService.dispose();
    this.fiatRateService.dispose();
    this.refreshService.dispose();

    super.dispose();
  }

  @override
  void initState() {
    super.initState();

    // Start fetching fiat rates in the background. We fetch the fiat rates on a
    // separate timer from the syncPayments and nodeInfo fetchers, since they
    // update on a different cadence (exactly 15 min vs unknowable) and from a
    // different source (lexe backend vs user node).
    this.fiatRateService = FiatRateService.start(
      app: this.widget.app,
      settings: this.widget.settings,
    );

    // Sync payments on refresh.
    this.paymentSyncOnRefresh =
        this.refreshService.refresh.listen(this.paymentSyncService.sync);

    // Fetch [NodeInfo] on refresh.
    this.nodeInfoFetchOnRefresh =
        this.refreshService.refresh.listen(this.nodeInfoService.fetch);

    // A stream of `BalanceState`s that gets updated when `nodeInfos` or
    // `fiatRate` are updated.
    this.balanceState = combine2(
      this.nodeInfoService.nodeInfo,
      this.fiatRateService.fiatRate,
      (nodeInfo, fiatRate) {
        final balance =
            BalanceState(balanceSats: nodeInfo?.balance, fiatRate: fiatRate);
        info("balance-state: $balance");
        return balance;
      },
    );

    // When the refresh button should show a loading spinner.
    this.isRefreshing = combine2(
      this.paymentSyncService.isSyncing,
      this.nodeInfoService.isFetching,
      (isSyncing, isFetching) => isSyncing || isFetching,
    );

    // Listen to platform URI events (e.g., user taps a "lightning:" URI in
    // their browser).
    this.uriEventsListener =
        this.widget.uriEvents.uriStream.listen(this.onUriEvent);

    // Start us off with an initial refresh.
    scheduleMicrotask(this.refreshService.triggerRefreshUnthrottled);
  }

  /// User triggers a refresh (fetch balance, fiat rates, payment sync).
  /// NOTE: the refresh stream is throttled to max every 3 sec.
  void triggerRefresh() => this.refreshService.triggerRefresh();

  /// Start a new burst refresh.
  void triggerBurstRefresh() => this.refreshService.triggerBurstRefresh();

  /// When a user taps a payment URI (ex: "lightning:") in another app/browser,
  /// and chooses Lexe to handle it, we'll try to open a new send flow to handle
  /// it.
  Future<void> onUriEvent(String uri) async {
    // TODO(phlip9): one issue here is: what to do if we get _another_ payment
    // URI while we're already mid send flow? Probably the right thing to do is
    // ask the user if they want to interrupt their current flow, and then
    // replace the current flow with a new flow if they agree.

    // For now, just queue up events while we're already handling one.
    this.uriEventsListener.pause();

    try {
      info("WalletPage: uriEvent: $uri");

      // Wait for NodeInfo to be available (if not already) and try to preflight
      // this send payment URI, showing a modal loading widget.
      final result = await this._onUriEventPreflight(uri);
      info("WalletPage: uriEvent: preflight result: $result");
      if (!this.mounted || result == null || result.isErr) return;

      // If the user successfully sent a payment, we'll get the new payment's
      // `PaymentIndex` from the flow. O/w canceling the flow will give us `null`.
      final SendFlowResult? flowResult =
          await Navigator.of(this.context).push(MaterialPageRoute(
        builder: (context) =>
            SendPaymentPage(sendCtx: result.unwrap(), startNewFlow: true),
      ));

      info("WalletPage: uriEvent: flowResult: $flowResult");

      // User canceled
      if (!this.mounted || flowResult == null) return;

      // Refresh and open new payment detail
      await this.onSendFlowSuccess(flowResult);
    } finally {
      this.uriEventsListener.resume();
    }
  }

  /// Try to preflight a send payment URI, showing a spinner while it's loading
  /// and an error modal if it fails.
  Future<Result<SendState, String>?> _onUriEventPreflight(String uri) async {
    // We could be cold starting (the user wants Lexe to make a payment from
    // another app, but Lexe isn't already running, so it needs to startup
    // cold).
    //
    // In such a case, we'll need to wait (with a timeout) for our connection to
    // the node to go through so we can get our balance.
    final result = await this.collectSendContext();

    // Canceled or timedout
    if (!this.mounted || result.isErr) return null;

    final sendCtx = result.unwrap();
    return showModalAsyncFlow(
      context: this.context,
      future: sendCtx.resolveAndMaybePreflight(uri),
      // TODO(phlip9): error messages need work
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
  }

  /// Open the left drawer.
  void openScaffoldDrawer() {
    this.scaffoldKey.currentState?.openDrawer();
  }

  /// Open the [ChannelsPage] for the user to manage their lightning channels.
  Future<void> onOpenChannelsPage() async {
    // We want to reuse the same cached [NodeInfo] while allowing the
    // [ChannelsPage] to fetch on its own cadence, so we'll pass down the
    // [NodeInfoService] but pause the refresher on this page.
    this.refreshService.pauseBackgroundRefresh();

    try {
      await Navigator.of(this.context).push(
        MaterialPageRoute(
          builder: (context) => ChannelsPage(
            app: this.widget.app,
            fiatRate: this.fiatRateService.fiatRate,
            balanceState: this.balanceState,
            nodeInfoService: this.nodeInfoService,
          ),
        ),
      );
    } finally {
      if (this.mounted) this.refreshService.resumeBackgroundRefresh();
    }
  }

  /// Called when the "Receive" button is pressed. Pushes the receive payment
  /// page onto the navigation stack.
  Future<void> onReceivePressed() async {
    // Navigate to receive page and wait for user to return to wallet screen.
    await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (context) => ReceivePaymentPage(
          app: this.widget.app,
          fiatRate: this.fiatRateService.fiatRate,
        ),
      ),
    );
    if (!this.mounted) return;

    // Maybe user received a payment, burst refresh to pick it up if we're lucky.
    // TODO(phlip9): real event stream from node should make this unnecessary.
    this.triggerBurstRefresh();
  }

  /// Called when the "Send" button is pressed. Pushes the send payment page
  /// onto the navigation stack.
  Future<void> onSendPressed() async {
    final sendCtx = this.tryCollectSendContext();
    if (sendCtx == null) return;

    // If the user successfully sent a payment, we'll get the new payment's
    // `PaymentIndex` from the flow. O/w canceling the flow will give us `null`.
    final SendFlowResult? flowResult =
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (context) =>
          SendPaymentPage(sendCtx: sendCtx, startNewFlow: true),
    ));

    info("WalletPage: onSendPressed: flowResult: $flowResult");

    // User canceled
    if (!this.mounted || flowResult == null) return;

    // Refresh and open new payment detail
    await this.onSendFlowSuccess(flowResult);
  }

  /// Called when the "Scan" button is pressed. Pushes the QR scan page onto the
  /// navigation stack.
  Future<void> onScanPressed() async {
    final sendCtx = this.tryCollectSendContext();
    if (sendCtx == null) return;

    // If the user successfully sent a payment, we'll get the new payment's
    // `PaymentIndex` from the flow. O/w canceling the flow will give us `null`.
    //
    // Note: this is inside a MultistepFlow so "back" goes back a step while
    // "close" exits the flow to this page again.
    final SendFlowResult? flowResult =
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_context) => MultistepFlow<SendFlowResult>(
        builder: (_context) => ScanPage(sendCtx: sendCtx),
      ),
    ));
    info("WalletPage: onScanPressed: flowResult: $flowResult");

    // User canceled
    if (!this.mounted || flowResult == null) return;

    // Refresh and open new payment detail
    await this.onSendFlowSuccess(flowResult);
  }

  /// Collect up all the relevant context needed to support a new send payment
  /// flow, and wait until it's available if it's not already immediately
  /// available.
  Future<Result<SendState_NeedUri, TimeoutException>>
      collectSendContext() async {
    final nodeInfo = this.nodeInfoService.nodeInfo.value;
    if (nodeInfo != null) return Ok(this.nodeInfoIntoSendContext(nodeInfo));

    final res = await Result.tryAsync<NodeInfo, TimeoutException>(
      () => this
          .nodeInfoService
          .nodeInfo
          .nextValue()
          .then((nodeInfo) => nodeInfo!)
          .timeout(const Duration(seconds: 15)),
    );
    return res.map(this.nodeInfoIntoSendContext);
  }

  /// Collect up all the relevant context needed to support a new send payment
  /// flow.
  SendState_NeedUri? tryCollectSendContext() {
    final nodeInfo = this.nodeInfoService.nodeInfo.value;
    // Ignore Send/Scan button press, we haven't fetched the node info yet.
    if (nodeInfo == null) return null;
    return this.nodeInfoIntoSendContext(nodeInfo);
  }

  SendState_NeedUri nodeInfoIntoSendContext(NodeInfo nodeInfo) =>
      SendState_NeedUri(
        app: this.widget.app,
        configNetwork: this.widget.config.network,
        balance: nodeInfo.balance,
        cid: ClientPaymentId.genNew(),
      );

  /// Called after the user has successfully sent a new payment and the send
  /// flow has popped back to the wallet page. We'll trigger a refresh, wait
  /// for the next payments sync, then open the payment detail page for the
  /// new page.
  ///
  /// For lightning payments, we'll also start burst refreshing, so we can
  /// quickly pick up any status changes.
  Future<void> onSendFlowSuccess(SendFlowResult flowResult) async {
    final payment = flowResult.payment;

    // Lightning payments actually have a chance to finalize in the next few
    // seconds, so start a burst refresh.
    switch (payment.kind) {
      case PaymentKind.invoice || PaymentKind.spontaneous:
        this.triggerBurstRefresh();
      case PaymentKind.onchain:
        this.triggerRefresh();
    }

    // Open the payment detail page to this unsynced payment.
    this.onPaymentTap(payment.index, PaymentSource.unsynced(payment));
  }

  /// Called when one of the payments in the [SliverPaymentsList] is tapped.
  void onPaymentTap(
    PaymentIndex paymentIndex,
    PaymentSource paymentSource,
  ) {
    Navigator.of(this.context).push(MaterialPageRoute(
      builder: (context) => PaymentDetailPage(
        app: this.widget.app,
        paymentIndex: paymentIndex,
        paymentSource: paymentSource,
        paymentsUpdated: this.paymentSyncService.updated,
        fiatRate: this.fiatRateService.fiatRate,
        isSyncing: this.paymentSyncService.isSyncing,
        triggerRefresh: this.triggerRefresh,
      ),
    ));
  }

  void onNodeInfoMenuPressed() {
    Navigator.of(this.context).push(MaterialPageRoute(
      builder: (context) => NodeInfoPage(
        nodeInfo: this.nodeInfoService.nodeInfo,
        userInfo: this.widget.app.userInfo(),
      ),
    ));
  }

  void onDebugPressed() {
    Navigator.of(this.context).push(MaterialPageRoute(
      builder: (context) => DebugPage(
        config: this.widget.config,
        app: this.widget.app,
        settings: this.widget.settings,
      ),
    ));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      key: this.scaffoldKey,
      extendBodyBehindAppBar: true,
      appBar: AppBar(
        // ≡ - Open navigation drawer on the left
        leading: IconButton(
          icon: const Icon(LxIcons.menu),
          onPressed: this.openScaffoldDrawer,
        ),

        // ⟳ - Trigger refresh of current balance, payments, etc...
        actions: [
          LxRefreshButton(
            isRefreshing: this.isRefreshing,
            triggerRefresh: this.triggerRefresh,
          ),
          const SizedBox(width: Space.s100),
        ],
      ),
      drawer: WalletDrawer(
        config: this.widget.config,
        onOpenChannelsPage: this.onOpenChannelsPage,
        onNodeInfoMenuPressed: this.onNodeInfoMenuPressed,
        onDebugPressed: this.onDebugPressed,
      ),
      body: ScrollableSinglePageBody(
        padding: EdgeInsets.zero,
        bodySlivers: [
          // The primary wallet page content
          //
          // * Balance
          // * Wallet Actions (Fund, Receive, Send, ...)
          SliverToBoxAdapter(
            child: Column(children: [
              const SizedBox(height: Space.s1000),
              ValueListenableBuilder(
                valueListenable: this.balanceState,
                builder: (context, balanceState, child) => BalanceWidget(
                  state: balanceState,
                  settings: this.widget.settings,
                  onOpenChannelsPage: this.onOpenChannelsPage,
                ),
              ),
              const SizedBox(height: Space.s700),
              WalletActions(
                // ☐ - Quickly scan a QR code
                onScanPressed: this.onScanPressed,
                // ↓ - Open BTC/LN receive payment page
                onReceivePressed: this.onReceivePressed,
                // ↑ - Open BTC/LN send payment page
                onSendPressed: this.onSendPressed,
              ),
              const SizedBox(height: Space.s600),
            ]),
          ),

          // Pending payments && not junk + header
          ListenableBuilder(
            listenable: this.paymentSyncService.updated,
            builder: (context, child) => SliverPaymentsList(
              app: this.widget.app,
              filter: PaymentsListFilter.pendingNotJunk,
              onPaymentTap: this.onPaymentTap,
            ),
          ),

          // Completed+Failed && not junk payments + header
          ListenableBuilder(
            listenable: this.paymentSyncService.updated,
            builder: (context, child) => SliverPaymentsList(
              app: this.widget.app,
              filter: PaymentsListFilter.finalizedNotJunk,
              onPaymentTap: this.onPaymentTap,
            ),
          ),
        ],
      ),
    );
  }
}

class WalletDrawer extends StatelessWidget {
  const WalletDrawer({
    super.key,
    required this.config,
    // this.onSettingsPressed,
    // this.onBackupPressed,
    // this.onSecurityPressed,
    // this.onSupportPressed,
    this.onOpenChannelsPage,
    this.onNodeInfoMenuPressed,
    this.onDebugPressed,
    // this.onInvitePressed,
  });

  final Config config;

  // final VoidCallback? onSettingsPressed;
  // final VoidCallback? onBackupPressed;
  // final VoidCallback? onSecurityPressed;
  // final VoidCallback? onSupportPressed;
  final VoidCallback? onOpenChannelsPage;
  final VoidCallback? onNodeInfoMenuPressed;
  final VoidCallback? onDebugPressed;
  // final VoidCallback? onInvitePressed;

  @override
  Widget build(BuildContext context) {
    final systemBarHeight = MediaQuery.of(context).padding.top;

    return Drawer(
      child: Padding(
        padding: EdgeInsets.only(top: systemBarHeight),
        child: ListView(
          padding: EdgeInsets.zero,
          children: [
            // X - close
            DrawerListItem(
              icon: LxIcons.close,
              onTap: () => Scaffold.of(context).closeDrawer(),
            ),
            const SizedBox(height: Space.s600),

            DrawerListItem(
              title: "Channels",
              icon: LxIcons.openCloseChannel,
              onTap: this.onOpenChannelsPage,
            ),
            DrawerListItem(
              title: "Node info",
              icon: LxIcons.nodeInfo,
              onTap: this.onNodeInfoMenuPressed,
            ),

            // TODO(phlip9): impl
            // // * Settings
            // // * Backup
            // // * Security
            // // * Support
            // DrawerListItem(
            //   title: "Settings",
            //   icon: LxIcons.settings,
            //   onTap: this.onSettingsPressed,
            // ),
            // DrawerListItem(
            //   title: "Backup",
            //   icon: LxIcons.backup,
            //   onTap: this.onBackupPressed,
            // ),
            // DrawerListItem(
            //   title: "Security",
            //   icon: LxIcons.security,
            //   onTap: this.onSecurityPressed,
            // ),
            // DrawerListItem(
            //   title: "Support",
            //   icon: LxIcons.support,
            //   onTap: this.onSupportPressed,
            // ),

            // Debugging
            DrawerListItem(
              title: "Debug",
              icon: LxIcons.debug,
              onTap: this.onDebugPressed,
            ),

            const SizedBox(height: Space.s600),

            // TODO(phlip9): impl
            // // < Invite Friends >
            // Padding(
            //   padding: const EdgeInsets.symmetric(horizontal: Space.s500),
            //   child: LxOutlinedButton(
            //     // TODO(phlip9): we use a closure to see button w/o disabled
            //     // styling. remove extra closure when real functionality exists.
            //     onTap: () => this.onInvitePressed?.call(),
            //     label: const Text("Invite Friends"),
            //   ),
            // ),
            // const SizedBox(height: Space.s600),

            // Show currently installed app version.
            // ex: "Lexe · v0.6.2+5"
            FutureBuilder(
              future: UserAgent.fromPlatform(),
              builder: (context, out) {
                final userAgent = out.data ?? UserAgent.dummy();
                return Text(
                  "${userAgent.appName} · v${userAgent.version}",
                  textAlign: TextAlign.center,
                  style: Fonts.fontUI.copyWith(
                    color: LxColors.grey600,
                    fontSize: Fonts.size200,
                  ),
                );
              },
            )
          ],
        ),
      ),
    );
  }
}

class DrawerListItem extends StatelessWidget {
  const DrawerListItem({super.key, this.title, this.icon, this.onTap});

  final String? title;
  final IconData? icon;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: Space.s500),
      horizontalTitleGap: Space.s200,
      visualDensity: VisualDensity.standard,
      dense: false,
      leading: (this.icon != null)
          ? Icon(
              this.icon!,
              color: LxColors.foreground,
              size: Fonts.size700,
            )
          : null,
      title: (this.title != null)
          ? Text(this.title!,
              style: Fonts.fontUI.copyWith(
                fontSize: Fonts.size400,
                fontVariations: [Fonts.weightMedium],
              ))
          : null,
      onTap: this.onTap,
    );
  }
}

class BalanceWidget extends StatelessWidget {
  const BalanceWidget({
    super.key,
    required this.settings,
    required this.state,
    required this.onOpenChannelsPage,
  });

  final LxSettings settings;
  final BalanceState state;
  final VoidCallback onOpenChannelsPage;

  /// Toggle expanding the sub-balances drop down
  void toggleSplitBalancesExpanded() {
    final value = this.settings.showSplitBalances.value ?? true;
    this.settings.update(Settings(showSplitBalances: !value)).unwrap();
  }

  @override
  Widget build(BuildContext context) {
    final totalSats = this.state.totalSats();
    final totalSatsStyle = Fonts.fontUI.copyWith(
      fontSize: Fonts.size300,
      color: LxColors.grey700,
      fontVariations: [Fonts.weightMedium],
    );
    final totalSatsOrPlaceholder = (totalSats != null)
        ? Text(
            currency_format.formatSatsAmount(totalSats),
            style: totalSatsStyle,
          )
        : FilledTextPlaceholder(
            width: Space.s900,
            color: LxColors.background,
            style: totalSatsStyle,
          );

    final totalFiat = this.state.totalFiat();
    final totalFiatStyle = Fonts.fontUI.copyWith(
      color: LxColors.foreground,
      fontSize: Fonts.size800,
      fontVariations: [Fonts.weightMedium],
      letterSpacing: -0.5,
    );
    final totalFiatOrPlaceholder = (totalFiat != null)
        ? SplitAmountText(
            amount: totalFiat.amount,
            fiatName: totalFiat.fiat,
            style: totalFiatStyle,
            textAlign: TextAlign.end,
          )
        : FilledTextPlaceholder(
            width: Space.s1000,
            color: LxColors.background,
            style: totalFiatStyle,
          );

    const iconSize = Space.s500;
    const iconColor = LxColors.fgSecondary;
    const iconBg = LxColors.background;
    final icon = ValueListenableBuilder(
        valueListenable: this.settings.showSplitBalances,
        builder: (context, showSplitBalances, child) =>
            (showSplitBalances ?? true)
                ? const ListIcon(
                    Icon(
                      LxIcons.expandUpSmall,
                      size: iconSize,
                      color: iconColor,
                    ),
                    background: iconBg,
                  )
                : ListIcon(
                    Transform.translate(
                      offset: const Offset(0.0, 2.0),
                      child: const Icon(
                        LxIcons.expandDownSmall,
                        size: iconSize,
                        color: iconColor,
                      ),
                    ),
                    background: iconBg,
                  ));

    final totalBalance = Padding(
      padding: const EdgeInsets.symmetric(horizontal: Space.s400),
      child: Material(
        borderRadius: BorderRadius.circular(LxRadius.r400),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: this.toggleSplitBalancesExpanded,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(
                Space.s500, Space.s500, Space.s600, Space.s500),
            // Use a stack here so the amount text can span the full box and
            // occlude the icon. For large denomination currencies, this should
            // leave us enough space.
            child: Stack(
              children: [
                // v / ^ - expand/collapse icon
                Positioned(
                  bottom: 0.0,
                  left: 0.0,
                  child: Transform.translate(
                    offset: const Offset(0.0, 2.0),
                    child: icon,
                  ),
                ),
                // total balance
                Row(
                  mainAxisSize: MainAxisSize.max,
                  mainAxisAlignment: MainAxisAlignment.end,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Expanded(
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        mainAxisAlignment: MainAxisAlignment.start,
                        crossAxisAlignment: CrossAxisAlignment.end,
                        children: [
                          totalFiatOrPlaceholder,
                          const SizedBox(height: Space.s100),
                          totalSatsOrPlaceholder,
                        ],
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
    );

    final subBalances = ValueListenableBuilder(
      valueListenable: this.settings.showSplitBalances,
      builder: (context, showSplitBalances, child) =>
          (showSplitBalances ?? true) ? child! : const SizedBox(),
      child: Stack(
        alignment: Alignment.center,
        children: <Widget>[
          // LN/BTC sub balances
          GestureDetector(
            onTap: this.onOpenChannelsPage,
            child: Padding(
              padding: const EdgeInsets.only(
                left: Space.s400 + Space.s500,
                right: Space.s400 + Space.s600 + 1.0,
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  SubBalanceRow(
                    kind: BalanceKind.lightning,
                    balance: this.state,
                  ),
                  const SizedBox(height: Space.s200),
                  SubBalanceRow(
                    kind: BalanceKind.onchain,
                    balance: this.state,
                  ),
                ],
              ),
            ),
          ),
          // ↑↓ - Open/close channel button on the right
          Positioned(
            child: Align(
              alignment: Alignment.centerRight,
              child: Padding(
                padding: const EdgeInsets.only(right: 2.0),
                child: IconButton(
                  onPressed: onOpenChannelsPage,
                  // Rotate the icon so it's up/down and not left/right.
                  // Doesn't seem to be a vertical variant of this icon...
                  icon: Transform.rotate(
                    angle: 0.5 * math.pi,
                    child: const Icon(
                      LxIcons.openCloseChannel,
                      color: LxColors.fgSecondary,
                    ),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        totalBalance,
        const SizedBox(height: Space.s300),
        subBalances,
      ],
    );
  }
}

class WalletActions extends StatelessWidget {
  const WalletActions({
    super.key,
    this.onScanPressed,
    this.onSendPressed,
    this.onReceivePressed,
  });

  final VoidCallback? onScanPressed;
  final VoidCallback? onSendPressed;
  final VoidCallback? onReceivePressed;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        WalletActionButton(
          onPressed: this.onScanPressed,
          icon: LxIcons.scan,
          label: "Scan",
        ),
        const SizedBox(width: Space.s400),
        WalletActionButton(
          onPressed: this.onReceivePressed,
          icon: LxIcons.receive,
          label: "Receive",
        ),
        const SizedBox(width: Space.s400),
        WalletActionButton(
          onPressed: this.onSendPressed,
          icon: LxIcons.send,
          label: "Send",
        ),
      ],
    );
  }
}

class WalletActionButton extends StatelessWidget {
  const WalletActionButton({
    super.key,
    required this.onPressed,
    required this.icon,
    required this.label,
  });

  final VoidCallback? onPressed;
  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    final bool isDisabled = (this.onPressed == null);

    return Column(
      children: [
        FilledButton(
          onPressed: this.onPressed,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: Space.s450),
            child: Icon(this.icon, size: Fonts.size700),
          ),
        ),
        const SizedBox(height: Space.s400),
        Text(
          this.label,
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size300,
            color: (!isDisabled) ? LxColors.foreground : LxColors.grey725,
            fontVariations: [Fonts.weightSemiBold],
          ),
        ),
      ],
    );
  }
}

enum PaymentsListFilter {
  all,
  pending,
  pendingNotJunk,
  finalized,
  finalizedNotJunk,
  ;

  String asTitle() => switch (this) {
        all => "Payments",
        pending => "Pending",
        pendingNotJunk => "Pending",
        finalized => "Completed",
        finalizedNotJunk => "Completed",
      };
}

typedef PaymentTapCallback = void Function(
  PaymentIndex paymentIndex,
  PaymentSource paymentSource,
);

class SliverPaymentsList extends StatefulWidget {
  const SliverPaymentsList({
    super.key,
    required this.app,
    required this.filter,
    required this.onPaymentTap,
  });

  final AppHandle app;
  final PaymentsListFilter filter;
  final PaymentTapCallback onPaymentTap;

  @override
  State<SliverPaymentsList> createState() => _SliverPaymentsListState();
}

class _SliverPaymentsListState extends State<SliverPaymentsList> {
  // When this ticks every 30 sec, all the payments' createdAt label should
  // update. All the payment times should also update at the same time, which is
  // why they all share the same ticker, hoisted up here to the parent list
  // widget.
  final DateTimeNotifier paymentDateUpdates =
      DateTimeNotifier(period: const Duration(seconds: 30));

  @override
  void dispose() {
    this.paymentDateUpdates.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final int paymentKindCount = switch (this.widget.filter) {
      PaymentsListFilter.all => this.widget.app.getNumPayments(),
      PaymentsListFilter.pending => this.widget.app.getNumPendingPayments(),
      PaymentsListFilter.pendingNotJunk =>
        this.widget.app.getNumPendingNotJunkPayments(),
      PaymentsListFilter.finalized => this.widget.app.getNumFinalizedPayments(),
      PaymentsListFilter.finalizedNotJunk =>
        this.widget.app.getNumFinalizedNotJunkPayments(),
    };
    // info("build SliverPaymentsList: filter: ${this.widget.filter}, "
    //     "paymentKindCount: $paymentKindCount");

    final numHeaders = switch (paymentKindCount) {
      > 0 => 1,
      _ => 0,
    };
    final childCount = paymentKindCount + numHeaders;

    return SliverFixedExtentList(
      itemExtent: Space.s800,
      delegate: SliverChildBuilderDelegate(
        childCount: childCount,
        (context, paymentPlusHeaderIdx) {
          if (paymentPlusHeaderIdx < numHeaders) {
            return Align(
              alignment: Alignment.bottomLeft,
              child: Padding(
                padding: const EdgeInsets.symmetric(
                    horizontal: Space.s400, vertical: Space.s200),
                child: Text(
                  this.widget.filter.asTitle(),
                  style: Fonts.fontUI.copyWith(
                    fontSize: Fonts.size200,
                    color: LxColors.fgTertiary,
                    fontVariations: [Fonts.weightMedium],
                  ),
                ),
              ),
            );
          }

          final scrollIdx = paymentPlusHeaderIdx - numHeaders;

          final ShortPaymentAndIndex? result = switch (this.widget.filter) {
            PaymentsListFilter.all =>
              this.widget.app.getShortPaymentByScrollIdx(scrollIdx: scrollIdx),
            PaymentsListFilter.pending => this
                .widget
                .app
                .getPendingShortPaymentByScrollIdx(scrollIdx: scrollIdx),
            PaymentsListFilter.pendingNotJunk => this
                .widget
                .app
                .getPendingNotJunkShortPaymentByScrollIdx(scrollIdx: scrollIdx),
            PaymentsListFilter.finalized => this
                .widget
                .app
                .getFinalizedShortPaymentByScrollIdx(scrollIdx: scrollIdx),
            PaymentsListFilter.finalizedNotJunk => this
                .widget
                .app
                .getFinalizedNotJunkShortPaymentByScrollIdx(
                    scrollIdx: scrollIdx),
          };
          if (result == null) return null;

          return PaymentsListEntry(
            vecIdx: result.vecIdx,
            payment: result.payment,
            paymentDateUpdates: this.paymentDateUpdates,
            onTap: () => this.widget.onPaymentTap(
                  result.payment.index,
                  PaymentSource.localDb(result.vecIdx),
                ),
          );
        },
        // findChildIndexCallback: (Key childKey) => this.app.getPaymentScrollIdxByPaymentId(childKey),
      ),
    );
  }
}

String formatFiatValue({
  required FiatRate? rate,
  required int? amountSats,
  required PaymentDirection direction,
}) {
  if (rate == null || amountSats == null) {
    return "";
  }

  final fiatValue = currency_format.satsToBtc(amountSats) * rate.rate;
  final sign = currency_format.directionToSign(direction);

  final NumberFormat currencyFormatter =
      NumberFormat.simpleCurrency(name: rate.fiat);
  return "$sign${currencyFormatter.format(fiatValue)}";
}

class PaymentsListEntry extends StatelessWidget {
  PaymentsListEntry({
    required int vecIdx,
    required this.payment,
    required this.paymentDateUpdates,
    required this.onTap,
  }) : super(key: ValueKey<int>(vecIdx));

  final VoidCallback onTap;
  final ValueListenable<DateTime> paymentDateUpdates;
  final ShortPayment payment;

  @override
  Widget build(BuildContext context) {
    final status = this.payment.status;
    final direction = this.payment.direction;
    final kind = this.payment.kind;
    final amountSats = this.payment.amountSat;
    final note = this.payment.note;

    final leadingIcon =
        PaymentListIcon(kind: BalanceKind.fromPaymentKind(kind));

    // TODO(phlip9): figure out a heuristic to get the counterparty name.
    final String primaryStr;
    if (status == PaymentStatus.pending) {
      if (direction == PaymentDirection.inbound) {
        primaryStr = "Receiving payment";
      } else {
        primaryStr = "Sending payment";
      }
    } else {
      if (direction == PaymentDirection.inbound) {
        primaryStr = "You received";
      } else {
        primaryStr = "You sent";
      }
    }

    // ex: "Receiving payment" (pending, inbound)
    // ex: "Sending payment" (pending, outbound)
    // ex: "You received" (finalized, inbound)
    // ex: "You sent" (finalized, outbound)
    final primaryText = Text(
      primaryStr,
      maxLines: 1,
      style: Fonts.fontUI.copyWith(
        fontSize: Fonts.size300,
        color: LxColors.fgSecondary,
        fontVariations: [Fonts.weightMedium],
      ),
    );

    // TODO(phlip9): display as BTC rather than sats depending on user
    //               preferences.
    // the weird unicode thing that isn't rendering is the BTC B currency symbol
    // "+₿0.00001230",

    final Color primaryValueColor;
    if (direction == PaymentDirection.inbound &&
        status != PaymentStatus.failed) {
      primaryValueColor = LxColors.moneyGoUp;
    } else {
      primaryValueColor = LxColors.fgSecondary;
    }

    final String amountSatsStr = (amountSats != null)
        ? currency_format.formatSatsAmount(amountSats,
            direction: direction, satsSuffix: true)
        : "";

    // ex: "" (certain niche cases w/ failed or pending LN invoice payments)
    // ex: "+45,000 sats"
    // ex: "-128 sats"
    final primaryValueText = Text(
      amountSatsStr,
      maxLines: 1,
      textAlign: TextAlign.end,
      style: Fonts.fontUI.copyWith(
        fontSize: Fonts.size200,
        color: primaryValueColor,
      ),
    );

    // ex: "Failed" (payment failed, no note)
    // ex: "Brunch with friends" (note only)
    // ex: "Failed · Funds from Boincase" (failed + note)
    final secondaryText = Text.rich(
      TextSpan(
        text: null,
        children: <TextSpan>[
          // prefix with "Failed" to indicate problem w/ payment.
          if (status == PaymentStatus.failed)
            const TextSpan(
              text: "Failed",
              style: TextStyle(
                color: LxColors.errorText,
                // fontVariations: [Fonts.weightMedium],
              ),
            ),
          // separator should only show if both sides are present
          if (status == PaymentStatus.failed && note != null)
            const TextSpan(text: " · "),
          if (note != null) TextSpan(text: note)
        ],
        style: Fonts.fontUI.copyWith(
          fontSize: Fonts.size200,
          color: LxColors.fgTertiary,
        ),
      ),
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
    );

    // Wrap the "createdAt" text so that it updates every ~30 sec, not just
    // when we refresh.
    final createdAt = DateTime.fromMillisecondsSinceEpoch(payment.createdAt);
    final secondaryDateText = ValueListenableBuilder(
        valueListenable: this.paymentDateUpdates,
        builder: (_, now, child) {
          final createdAtStr =
              date_format.formatDateCompact(then: createdAt, now: now);

          // ex: "just now" (less than a min old)
          // ex: "10min"
          // ex: "Jun 16"
          // ex: "14h"
          return Text(
            createdAtStr ?? "",
            maxLines: 1,
            textAlign: TextAlign.end,
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size200,
              color: LxColors.fgTertiary,
            ),
          );
        });

    return ListTile(
      onTap: this.onTap,

      // list tile styling

      contentPadding: const EdgeInsets.symmetric(
        horizontal: Space.s400,
        vertical: Space.s0,
      ),
      horizontalTitleGap: Space.s200,
      visualDensity: VisualDensity.standard,
      dense: true,

      // actual content

      leading: leadingIcon,

      // NOTE: we use a Row() in `title` and `subtitle` instead of `trailing` so
      // that the text baselines align properly.

      title: Row(
        mainAxisAlignment: MainAxisAlignment.start,
        crossAxisAlignment: CrossAxisAlignment.baseline,
        textBaseline: TextBaseline.alphabetic,
        children: [
          Expanded(
            child: primaryText,
          ),
          Padding(
            padding: const EdgeInsets.only(left: Space.s200),
            child: primaryValueText,
          )
        ],
      ),

      subtitle: Row(
        mainAxisAlignment: MainAxisAlignment.start,
        crossAxisAlignment: CrossAxisAlignment.baseline,
        textBaseline: TextBaseline.alphabetic,
        children: [
          Expanded(
            child: secondaryText,
          ),
          Padding(
            padding: const EdgeInsets.only(left: Space.s200),
            child: secondaryDateText,
          )
        ],
      ),
    );
  }
}

class PaymentListIcon extends StatelessWidget {
  const PaymentListIcon({
    super.key,
    required this.kind,
  });

  final BalanceKind kind;

  @override
  Widget build(BuildContext context) => switch (this.kind) {
        BalanceKind.lightning => const ListIcon.lightning(),
        BalanceKind.onchain => const ListIcon.bitcoin(),
      };
}
