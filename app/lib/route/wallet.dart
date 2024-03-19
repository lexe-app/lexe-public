// The primary wallet page.

import 'dart:async' show StreamController, Timer;

import 'package:flutter/material.dart';
import 'package:freezed_annotation/freezed_annotation.dart' show freezed;
import 'package:intl/intl.dart' show NumberFormat;
import 'package:rxdart_ext/rxdart_ext.dart';

import '../bindings_generated_api.dart'
    show
        AppHandle,
        Config,
        DeployEnv,
        FiatRate,
        FiatRates,
        NodeInfo,
        PaymentDirection,
        PaymentKind,
        PaymentStatus,
        ShortPayment;
import '../components.dart'
    show FilledPlaceholder, LxRefreshButton, StateStreamBuilder;
import '../currency_format.dart' as currency_format;
import '../date_format.dart' as date_format;
import '../logger.dart';
import '../result.dart';
import '../route/debug.dart' show DebugPage;
import '../route/payment_detail.dart' show PaymentDetailPage;
import '../route/send.dart' show SendContext, SendPaymentPage;
import '../stream_ext.dart';
import '../style.dart' show Fonts, LxColors, Space;

// Include code generated by @freezed
part 'wallet.freezed.dart';

class WalletPage extends StatefulWidget {
  const WalletPage({super.key, required this.config, required this.app});

  final Config config;
  final AppHandle app;

  @override
  WalletPageState createState() => WalletPageState();
}

class WalletPageState extends State<WalletPage> {
  final GlobalKey<ScaffoldState> scaffoldKey = GlobalKey();

  /// A stream controller to trigger refreshes of the wallet page contents.
  final StreamController<Null> refresh = StreamController.broadcast();

  /// True if there's currently an outstanding refresh.
  final ValueNotifier<bool> isRefreshing = ValueNotifier(false);

  /// A stream controller to notify when some payments are updated.
  final StreamController<Null> paymentsUpdated = StreamController.broadcast();

  // BehaviorSubject: a StreamController that captures the latest item added
  // to the controller, and emits that as the first item to any new listener.
  final BehaviorSubject<FiatRate?> fiatRate = BehaviorSubject.seeded(null);
  final BehaviorSubject<NodeInfo?> nodeInfos = BehaviorSubject.seeded(null);

  // StateSubject: like BehaviorSubject but only notifies subscribers if the
  // new item is actually different.
  final StateSubject<BalanceState> balanceStates =
      StateSubject(BalanceState.placeholder);

  // TODO(phlip9): get from user preferences
  final String fiatPreference = "USD";
  // final String fiatPreference = "EUR";

  @override
  void initState() {
    super.initState();

    // Call `this.onRefresh` when we get a new refresh (and always once at
    // page open). Refreshes are also throttled and won't trigger while a
    // a previous refresh is pending.
    this
        .refresh
        .stream
        // ignore `triggerRefresh` if we're currently refreshing.
        .where((_) => !this.isRefreshing.value)
        // but unconditionally start with an initial "refresh" to load node
        // state.
        .startWith(null)
        // ignore multiple refreshes if the user triggers again within 5 secs.
        .throttleTime(const Duration(seconds: 5))
        .log(id: "refresh start")
        // ok we're actually refreshing for real this time! do some bookkeeping
        // and send some requests.
        .listen(this.onRefresh);

    // A stream of `BalanceState`s that gets updated when `nodeInfos` or
    // `fiatRate` are updated. Since it's fed into a `StateSubject`, it also
    // avoids widget rebuilds if new state == old state.
    Rx.combineLatest2(
      this.nodeInfos.map((nodeInfo) => nodeInfo?.spendableBalanceSats),
      this.fiatRate,
      (balanceSats, fiatRate) => BalanceState(
        balanceSats: balanceSats,
        fiatName: this.fiatPreference,
        fiatRate: fiatRate,
      ),
    ).listen(this.balanceStates.addIfNotClosed);
  }

  @override
  void dispose() {
    this.refresh.close();
    this.isRefreshing.dispose();
    this.paymentsUpdated.close();
    this.nodeInfos.close();
    this.fiatRate.close();
    this.balanceStates.close();

    super.dispose();
  }

  /// User triggers a refresh (fetch balance, fiat rates, payment sync).
  /// NOTE: the refresh stream is throttled (on)
  void triggerRefresh() => this.refresh.addNull();

  /// On refresh, resync state from the node.
  Future<void> onRefresh(Null n) async {
    this.isRefreshing.value = true;

    await (fetchNodeInfo(), fetchFiatRates(), syncPayments()).wait;

    if (!this.mounted) return;
    this.isRefreshing.value = false;
  }

  Future<void> fetchNodeInfo() async {
    final res = await Result.tryFfiAsync(this.widget.app.nodeInfo);
    switch (res) {
      case Ok(:final ok):
        info("nodeInfo: $ok");
        this.nodeInfos.addIfNotClosed(ok);
        return;
      case Err(:final err):
        error("Failed to fetch nodeInfo: $err");
        return;
    }
  }

  Future<void> fetchFiatRates() async {
    final res = await Result.tryFfiAsync(this.widget.app.fiatRates);

    final FiatRates fiatRates;
    switch (res) {
      case Ok(:final ok):
        fiatRates = ok;
      case Err(:final err):
        error("Failed to fetch fiatRates: $err");
        return;
    }

    // Select just fiat rate for user's current preferred fiat currency
    final fiatRate =
        fiatRates.rates.firstWhere((rate) => rate.fiat == this.fiatPreference);
    info("fiatRate: $fiatRate, timestampMs: ${fiatRates.timestampMs}");

    this.fiatRate.addIfNotClosed(fiatRate);
  }

  Future<void> syncPayments() async {
    final res = await Result.tryFfiAsync(this.widget.app.syncPayments);

    final bool anyChangedPayments;
    switch (res) {
      case Ok(:final ok):
        anyChangedPayments = ok;
      case Err(:final err):
        error("Failed to syncPayments: $err");
        return;
    }

    // Only re-render payments if they've actually changed.
    if (anyChangedPayments) {
      this.paymentsUpdated.addIfNotClosed(null);
    }
  }

  void openScaffoldDrawer() {
    this.scaffoldKey.currentState?.openDrawer();
  }

  /// Called when the "Receive" button is pressed. Pushes the receive payment
  /// page onto the navigation stack.
  Future<void> onReceivePressed() async {
    // TODO(phlip9): remove this temporary hack once the recv UI gets build
    final result = await Result.tryFfiAsync(() => this.widget.app.getAddress());
    info("getAddress => $result");
  }

  /// Called when the "Send" button is pressed. Pushes the send payment page
  /// onto the navigation stack.
  Future<void> onSendPressed() async {
    final maybeNodeInfo = this.nodeInfos.value;
    if (maybeNodeInfo == null) {
      return;
    }

    final balanceSats = maybeNodeInfo.spendableBalanceSats;

    final bool? flowResult =
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (context) => SendPaymentPage(
        sendCtx: SendContext.cidFromRng(
          app: this.widget.app,
          configNetwork: this.widget.config.network,
          balanceSats: balanceSats,
        ),
      ),
    ));

    // User canceled
    if (flowResult == null || !flowResult) return;
    if (!this.mounted) return;

    // Refresh to pick up new payment
    this.triggerRefresh();
  }

  void onDebugPressed() {
    Navigator.of(this.context).push(MaterialPageRoute(
      builder: (context) => DebugPage(
        config: this.widget.config,
        app: this.widget.app,
      ),
    ));
  }

  /// Called when one of the payments in the [SliverPaymentsList] is tapped.
  void onPaymentTap(int paymentVecIdx) {
    Navigator.of(this.context).push(MaterialPageRoute(
      builder: (context) => PaymentDetailPage(
        app: this.widget.app,
        vecIdx: paymentVecIdx,
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
          icon: const Icon(Icons.menu_rounded),
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
        onDebugPressed: this.onDebugPressed,
      ),
      body: CustomScrollView(
        slivers: [
          // The primary wallet page content
          //
          // * Balance
          // * Wallet Actions (Fund, Receive, Send, ...)
          SliverToBoxAdapter(
              child: Column(children: [
            const SizedBox(height: Space.s1100),
            StateStreamBuilder(
              stream: this.balanceStates,
              builder: (context, balanceState) => BalanceWidget(balanceState),
            ),
            const SizedBox(height: Space.s700),
            WalletActions(
              // + - (doesn't exist yet) fund wallet from exchange integration
              onFundPressed: null,
              // ↓ - Open BTC/LN receive payment page
              onReceivePressed: this.onReceivePressed,
              // ↑ - Open BTC/LN send payment page
              onSendPressed: this.onSendPressed,
            ),
            const SizedBox(height: Space.s800),
          ])),

          // Pending payments + header
          StreamBuilder(
            stream: this.paymentsUpdated.stream,
            initialData: null,
            builder: (context, snapshot) => SliverPaymentsList(
              app: this.widget.app,
              filter: PaymentsListFilter.pending,
              onPaymentTap: this.onPaymentTap,
            ),
          ),

          // Completed+Failed payments + header
          StreamBuilder(
            stream: this.paymentsUpdated.stream,
            initialData: null,
            builder: (context, snapshot) => SliverPaymentsList(
              app: this.widget.app,
              filter: PaymentsListFilter.finalized,
              onPaymentTap: this.onPaymentTap,
            ),
          )
        ],
      ),
    );
  }
}

class WalletDrawer extends StatelessWidget {
  const WalletDrawer({
    super.key,
    required this.config,
    this.onSettingsPressed,
    this.onBackupPressed,
    this.onSecurityPressed,
    this.onSupportPressed,
    this.onDebugPressed,
    this.onInvitePressed,
  });

  final Config config;

  final VoidCallback? onSettingsPressed;
  final VoidCallback? onBackupPressed;
  final VoidCallback? onSecurityPressed;
  final VoidCallback? onSupportPressed;
  final VoidCallback? onDebugPressed;
  final VoidCallback? onInvitePressed;

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
              icon: Icons.close_rounded,
              onTap: () => Scaffold.of(context).closeDrawer(),
            ),
            const SizedBox(height: Space.s600),

            // * Settings
            // * Backup
            // * Security
            // * Support
            DrawerListItem(
              title: "Settings",
              icon: Icons.settings_outlined,
              onTap: this.onSettingsPressed,
            ),
            DrawerListItem(
              title: "Backup",
              icon: Icons.drive_file_move_outline,
              onTap: this.onBackupPressed,
            ),
            DrawerListItem(
              title: "Security",
              icon: Icons.lock_outline_rounded,
              onTap: this.onSecurityPressed,
            ),
            DrawerListItem(
              title: "Support",
              icon: Icons.help_outline_rounded,
              onTap: this.onSupportPressed,
            ),
            if (config.deployEnv == DeployEnv.Dev ||
                config.deployEnv == DeployEnv.Staging)
              DrawerListItem(
                title: "Debug",
                icon: Icons.bug_report_outlined,
                onTap: this.onDebugPressed,
              ),

            const SizedBox(height: Space.s600),

            // < Invite Friends >
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: Space.s500),
              child: OutlinedButton(
                style: OutlinedButton.styleFrom(
                  backgroundColor: LxColors.background,
                  foregroundColor: LxColors.foreground,
                  side:
                      const BorderSide(color: LxColors.foreground, width: 2.0),
                  padding: const EdgeInsets.symmetric(vertical: Space.s500),
                ),
                onPressed: this.onInvitePressed,
                child: Text("Invite Friends",
                    style: Fonts.fontUI.copyWith(
                      fontSize: Fonts.size400,
                      fontVariations: [Fonts.weightMedium],
                    )),
              ),
            ),
            const SizedBox(height: Space.s600),

            // app version
            Text("Lexe App · v1.2.345",
                textAlign: TextAlign.center,
                style: Fonts.fontUI.copyWith(
                  color: LxColors.grey600,
                  fontSize: Fonts.size200,
                )),
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
          ? Icon(this.icon!, color: LxColors.foreground, size: Fonts.size700)
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

@freezed
class BalanceState with _$BalanceState {
  const factory BalanceState({
    required int? balanceSats,
    required String fiatName,
    required FiatRate? fiatRate,
  }) = _BalanceState;

  const BalanceState._();

  static BalanceState placeholder =
      const BalanceState(balanceSats: null, fiatName: "USD", fiatRate: null);

  double? fiatBalance() => (this.balanceSats != null && this.fiatRate != null)
      ? currency_format.satsToBtc(this.balanceSats!) * this.fiatRate!.rate
      : null;
}

class BalanceWidget extends StatelessWidget {
  const BalanceWidget(this.state, {super.key});

  final BalanceState state;

  @override
  Widget build(BuildContext context) {
    const satsBalanceSize = Fonts.size300;
    final satsBalanceOrPlaceholder = (this.state.balanceSats != null)
        ? Text(
            currency_format.formatSatsAmount(this.state.balanceSats!),
            style: Fonts.fontUI.copyWith(
              fontSize: satsBalanceSize,
              color: LxColors.grey700,
              fontVariations: [Fonts.weightMedium],
            ),
          )
        : const FilledPlaceholder(
            width: Space.s1000,
            height: satsBalanceSize,
            forText: true,
          );

    final fiatBalance = this.state.fiatBalance();
    final fiatBalanceOrPlaceholder = (fiatBalance != null)
        ? PrimaryBalanceText(
            fiatBalance: fiatBalance,
            fiatName: this.state.fiatRate!.fiat,
          )
        : const FilledPlaceholder(
            width: Space.s1100,
            height: Fonts.size800,
            forText: true,
          );

    return Column(
      children: [
        fiatBalanceOrPlaceholder,
        const SizedBox(height: Space.s400),
        satsBalanceOrPlaceholder,
      ],
    );
  }
}

class PrimaryBalanceText extends StatelessWidget {
  const PrimaryBalanceText({
    super.key,
    required this.fiatBalance,
    required this.fiatName,
  });

  final double fiatBalance;
  final String fiatName;

  @override
  Widget build(BuildContext context) {
    final (fiatBalanceWhole, fiatBalanceFract) =
        currency_format.formatFiatParts(this.fiatBalance, this.fiatName);

    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        Text(
          fiatBalanceWhole,
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size800,
            fontVariations: [Fonts.weightMedium],
          ),
        ),
        if (fiatBalanceFract.isNotEmpty)
          Text(
            fiatBalanceFract,
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size800,
              color: LxColors.fgTertiary,
              fontVariations: [Fonts.weightMedium],
            ),
          ),
      ],
    );
  }
}

class WalletActions extends StatelessWidget {
  const WalletActions({
    super.key,
    this.onFundPressed,
    this.onSendPressed,
    this.onReceivePressed,
  });

  final VoidCallback? onFundPressed;
  final VoidCallback? onSendPressed;
  final VoidCallback? onReceivePressed;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        WalletActionButton(
          onPressed: this.onFundPressed,
          icon: Icons.add_rounded,
          label: "Fund",
        ),
        const SizedBox(width: Space.s400),
        WalletActionButton(
          onPressed: this.onReceivePressed,
          icon: Icons.arrow_downward_rounded,
          label: "Receive",
        ),
        const SizedBox(width: Space.s400),
        WalletActionButton(
          onPressed: this.onSendPressed,
          icon: Icons.arrow_upward_rounded,
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
          style: FilledButton.styleFrom(
            backgroundColor: LxColors.grey975,
            disabledBackgroundColor: LxColors.grey850,
            foregroundColor: LxColors.foreground,
            disabledForegroundColor: LxColors.grey725,
          ),
          child: Padding(
            padding: const EdgeInsets.all(Space.s400),
            child: Icon(this.icon, size: Fonts.size700),
          ),
        ),
        const SizedBox(height: Space.s400),
        Text(
          label,
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
  finalized;

  String asTitle() => switch (this) {
        all => "Payments",
        pending => "Pending",
        finalized => "Completed",
      };
}

typedef PaymentTapCallback = void Function(int paymentVecIdx);

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
  // When this stream ticks, all the payments' createdAt label should update.
  // This stream ticks every 30 seconds. All the payment times should also
  // update at the same time, which is why they all share the same ticker
  // stream, hoisted up here to the parent list widget.
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

  @override
  Widget build(BuildContext context) {
    final int paymentKindCount = switch (this.widget.filter) {
      PaymentsListFilter.all => this.widget.app.getNumPayments(),
      PaymentsListFilter.pending => this.widget.app.getNumPendingPayments(),
      PaymentsListFilter.finalized => this.widget.app.getNumFinalizedPayments(),
    };
    info("build SliverPaymentsList: filter: ${this.widget.filter}, "
        "paymentKindCount: $paymentKindCount");

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

          final (int, ShortPayment)? result = switch (this.widget.filter) {
            PaymentsListFilter.all =>
              this.widget.app.getShortPaymentByScrollIdx(scrollIdx: scrollIdx),
            PaymentsListFilter.pending => this
                .widget
                .app
                .getPendingShortPaymentByScrollIdx(scrollIdx: scrollIdx),
            PaymentsListFilter.finalized => this
                .widget
                .app
                .getFinalizedShortPaymentByScrollIdx(scrollIdx: scrollIdx),
          };
          if (result == null) return null;

          final (vecIdx, payment) = result;
          return PaymentsListEntry(
            vecIdx: vecIdx,
            payment: payment,
            paymentDateUpdates: this.paymentDateUpdates,
            onTap: () => this.widget.onPaymentTap(vecIdx),
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
  final StateStream<DateTime> paymentDateUpdates;
  final ShortPayment payment;

  @override
  Widget build(BuildContext context) {
    final status = this.payment.status;
    final direction = this.payment.direction;
    final kind = this.payment.kind;
    final amountSats = this.payment.amountSat;
    final note = this.payment.note;

    final leadingIcon = PaymentListIcon(kind: kind);

    // TODO(phlip9): figure out a heuristic to get the counterparty name.
    final String primaryStr;
    if (status == PaymentStatus.Pending) {
      if (direction == PaymentDirection.Inbound) {
        primaryStr = "Receiving payment";
      } else {
        primaryStr = "Sending payment";
      }
    } else {
      if (direction == PaymentDirection.Inbound) {
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
    if (direction == PaymentDirection.Inbound &&
        status != PaymentStatus.Failed) {
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
    final secondaryText = RichText(
      text: TextSpan(
        text: null,
        children: <TextSpan>[
          // prefix with "Failed" to indicate problem w/ payment.
          if (status == PaymentStatus.Failed)
            const TextSpan(
              text: "Failed",
              style: TextStyle(
                color: LxColors.errorText,
                // fontVariations: [Fonts.weightMedium],
              ),
            ),
          // separator should only show if both sides are present
          if (status == PaymentStatus.Failed && note != null)
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
    final secondaryDateText = StateStreamBuilder(
        stream: this.paymentDateUpdates,
        builder: (_, now) {
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

  final PaymentKind kind;

  @override
  Widget build(BuildContext context) {
    final icon = switch (this.kind) {
      PaymentKind.Onchain => Icons.currency_bitcoin_rounded,
      PaymentKind.Invoice || PaymentKind.Spontaneous => Icons.bolt_rounded,
    };

    return DecoratedBox(
      decoration: const BoxDecoration(
        color: LxColors.grey850,
        borderRadius: BorderRadius.all(Radius.circular(20.0)),
      ),
      child: SizedBox.square(
        // pixel perfect alignment
        dimension: 39.0,
        child: Icon(
          icon,
          size: Space.s500,
          color: LxColors.fgSecondary,
        ),
      ),
    );
  }
}
