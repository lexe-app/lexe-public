import 'dart:async' show scheduleMicrotask;
import 'dart:math' show max, min;

import 'package:app_rs_dart/ffi/api.dart'
    show FiatRate, ListChannelsResponse, NodeInfo;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show LxChannelDetails;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show SystemUiOverlayStyle;
import 'package:lexeapp/components.dart'
    show
        ChannelBalanceBar,
        FilledCircle,
        FilledTextPlaceholder,
        ListIcon,
        LxBackButton,
        LxRefreshButton,
        ScrollableSinglePageBody,
        SliverPullToRefresh,
        SplitAmountText;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/notifier_ext.dart';
import 'package:lexeapp/route/close_channel.dart';
import 'package:lexeapp/route/open_channel.dart'
    show OpenChannelFlowResult, OpenChannelPage;
import 'package:lexeapp/service/list_channels.dart' show ListChannelsService;
import 'package:lexeapp/service/node_info.dart' show NodeInfoService;
import 'package:lexeapp/service/refresh.dart' show RefreshService;
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxRadius, LxTheme, Space;
import 'package:lexeapp/types.dart' show BalanceState, FiatAmount;

const double channelsListEntryHeight = 90;

/// The user can view and manage their Lightning channels on this page.
class ChannelsPage extends StatefulWidget {
  const ChannelsPage({
    super.key,
    required this.app,
    required this.fiatRate,
    required this.balanceState,
    required this.nodeInfoService,
  });

  final AppHandle app;

  /// Updating stream of fiat rates.
  final ValueListenable<FiatRate?> fiatRate;

  /// The current top-level balance.
  final ValueListenable<BalanceState> balanceState;

  /// The [NodeInfo] fetcher.
  final NodeInfoService nodeInfoService;

  @override
  State<ChannelsPage> createState() => _ChannelsPageState();
}

class _ChannelsPageState extends State<ChannelsPage> {
  /// Manage refreshing the [NodeInfo] and list of [LxChannelDetails].
  final RefreshService refreshService = RefreshService();

  /// Fetch [NodeInfo] on refresh.
  late final LxListener nodeInfoFetchOnRefresh;

  /// List channels on refresh.
  late final ListChannelsService listChannelsService =
      ListChannelsService(app: this.widget.app);
  late final LxListener listChannelsOnRefresh;

  /// The current sorted and projected lightning channels list.
  late final ComputedValueListenable<ChannelsList?> channels;

  /// The current total lightning channel balance.
  late final ComputedValueListenable<TotalChannelBalance?> totalChannelBalance;

  /// When the refresh button shows a loading spinner.
  late final ComputedValueListenable<bool> isRefreshing;

  @override
  void dispose() {
    // Dispose in reverse field order.
    this.isRefreshing.dispose();
    this.totalChannelBalance.dispose();
    this.channels.dispose();
    this.listChannelsOnRefresh.dispose();
    this.listChannelsService.dispose();
    this.nodeInfoFetchOnRefresh.dispose();
    this.refreshService.dispose();

    super.dispose();
  }

  @override
  void initState() {
    super.initState();

    // Fetch [NodeInfo] on refresh.
    this.nodeInfoFetchOnRefresh =
        this.refreshService.refresh.listen(this.widget.nodeInfoService.fetch);

    // List channels on refresh.
    this.listChannelsOnRefresh =
        this.refreshService.refresh.listen(this.listChannelsService.fetch);

    // Project API response into [ChannelsList] for UI display.
    this.channels = this
        .listChannelsService
        .listChannels
        .map((resp) => (resp != null) ? ChannelsList.fromApi(resp) : null);

    // Build [TotalChannelBalance].
    this.totalChannelBalance = combine2(
      this.listChannelsService.listChannels,
      this.widget.fiatRate,
      (channels, fiatRate) => (channels != null)
          ? TotalChannelBalance.fromApi(channels, fiatRate)
          : null,
    );

    // When the refresh button shows a loading spinner.
    this.isRefreshing = combine2(
      this.widget.nodeInfoService.isFetching,
      this.listChannelsService.isFetching,
      (fetchingNodeInfo, fetchingChannels) =>
          fetchingNodeInfo || fetchingChannels,
    );

    // Kick off an initial refresh.
    scheduleMicrotask(this.refreshService.triggerRefreshUnthrottled);
  }

  void triggerRefresh() => this.refreshService.triggerRefresh();

  /// Called when the big channel "Open" button is pressed. Begins the channel
  /// open UI flow.
  Future<void> onOpenPressed() async {
    // Begin open channel flow and wait for the flow result.
    final OpenChannelFlowResult? flowResult =
        await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (context) => OpenChannelPage(
          app: this.widget.app,
          balanceState: this.widget.balanceState,
        ),
      ),
    );

    info("ChannelsPage: onOpenPressed: $flowResult");

    if (!this.mounted || flowResult == null) return;

    // Successfully opened the channel, refresh channels list
    this.refreshService.triggerRefreshUnthrottled();

    // TODO(phlip9): highlight the new channel? open a detail page to track the
    // channel open?
  }

  /// Called when the big channel "Closed" button is pressed. Begins the channel
  /// close UI flow.
  Future<void> onClosePressed() async {
    final CloseChannelFlowResult? flowResult =
        await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (context) => CloseChannelPage(
          app: this.widget.app,
          fiatRate: this.widget.fiatRate,
          channels: this.channels,
        ),
      ),
    );

    info("ChannelsPage: onClosePressed: $flowResult");

    if (!this.mounted || flowResult == null) return;

    // Successfully closed the channel, refresh channels list
    this.refreshService.triggerRefreshUnthrottled();

    // TODO(phlip9): open a detail page to track status/confirmations?
  }

  @override
  Widget build(BuildContext context) {
    // Android: set the bottom nav bar to white bg so it matches the bottom sheet.
    return AnnotatedRegion<SystemUiOverlayStyle>(
      value: LxTheme.systemOverlayStyleLightWhiteBg,
      child: Scaffold(
        appBar: AppBar(
          leading: const LxBackButton(isLeading: true),
          // Refresh channels
          actions: [
            LxRefreshButton(
              isRefreshing: this.isRefreshing,
              triggerRefresh: this.triggerRefresh,
            ),
            const SizedBox(width: Space.appBarTrailingPadding),
          ],
        ),
        body: Stack(
          alignment: Alignment.bottomCenter,
          children: [
            ScrollableSinglePageBody(
              bodySlivers: [
                // Pull-to-refresh
                SliverPullToRefresh(onRefresh: this.triggerRefresh),

                // Heading + send/recv up to. Fixed extent.
                SliverToBoxAdapter(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      // Heading
                      const Padding(
                        padding: EdgeInsets.only(
                            top: Space.s300, bottom: Space.s100),
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.center,
                          children: [
                            ListIcon.lightning(),
                            SizedBox(width: Space.s200),
                            Text("Lightning channels",
                                style: Fonts.fontHeadlineSmall),
                          ],
                        ),
                      ),
                      const Text(
                        "Open channels to send payments instantly over the Lightning network",
                        style: Fonts.fontSubheading,
                      ),
                      const SizedBox(height: Space.s600),

                      // Send up to/Receive up to
                      ValueListenableBuilder(
                        valueListenable: this.totalChannelBalance,
                        builder: (context, totalChannelBalance, child) =>
                            TotalChannelBalanceWidget(
                                totalChannelBalance: totalChannelBalance),
                      ),
                      const SizedBox(height: Space.s800),

                      // You/Lexe LSP channels heading
                      const Row(
                        mainAxisAlignment: MainAxisAlignment.spaceBetween,
                        children: [
                          ChannelsPartyChip(name: "You"),
                          ChannelsPartyChip(name: "Lexe LSP"),
                        ],
                      ),
                      const SizedBox(height: Space.s200),
                    ],
                  ),
                ),

                // Channels list
                SliverPadding(
                  padding: const EdgeInsets.only(bottom: 300.0),
                  sliver: ValueListenableBuilder(
                    valueListenable: this.channels,
                    builder: (context, channelsList, child) =>
                        SliverFixedExtentList.list(
                      itemExtent: channelsListEntryHeight,
                      children: (channelsList != null)
                          ? channelsList.channels
                              .map((channel) => ChannelsListEntry(
                                    maxValueSats: channelsList.maxValueSats,
                                    channel: channel,
                                    fiatRate: this.widget.fiatRate,
                                  ))
                              .toList()
                          : [],
                    ),
                  ),
                ),
              ],
            ),

            // On-chain balance and open/close channel buttons
            Positioned(
              child: OnchainBottomSheet(
                balanceState: this.widget.balanceState,
                onOpenPressed: this.onOpenPressed,
                onClosedPressed: this.onClosePressed,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// The You/Lexe LSP header chip things
class ChannelsPartyChip extends StatelessWidget {
  const ChannelsPartyChip({super.key, required this.name});

  final String name;

  @override
  Widget build(BuildContext context) => DecoratedBox(
        decoration: const BoxDecoration(
          color: LxColors.grey850,
          borderRadius: BorderRadius.all(Radius.circular(LxRadius.r200)),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(
            vertical: Space.s200,
            horizontal: Space.s300,
          ),
          child: Text(
            this.name,
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size200,
              color: LxColors.fgSecondary,
            ),
          ),
        ),
      );
}

class TotalChannelBalance {
  const TotalChannelBalance({
    required this.outboundSendableSats,
    required this.inboundCapacitySats,
    required this.fiatRate,
  });

  /// The "true" point-in-time limit on what we can actually expect to send over
  /// our outbound channels. This value is the sum of `next_outbound_htlc_limit`
  /// over all `is_usable` channels.
  ///
  /// This value is different from a sum over the simpler `outbound_capacity`
  /// values, each of which is just:
  ///
  /// `outbound_capacity := balance - punishment_reserve - pending_outbound_htlcs`
  ///
  /// Instead, a `next_outbound_htlc_limit` represents the true limit for the
  /// next HTLC sent over that channel. It accounts for commitment tx fees, dust
  /// limits, and counterparty constraints, on top of the base `outbound_capacity`.
  final int outboundSendableSats;

  /// An approximate lower bound on the inbound capacity available to us.
  ///
  /// We don't currently have an accurate guage on "true" next HTLC receive
  /// limits the way we do for outbound channels.
  final int inboundCapacitySats;

  final FiatRate? fiatRate;

  factory TotalChannelBalance.fromApi(
      ListChannelsResponse channels, FiatRate? fiatRate) {
    final outboundSendableSats = maxInt(
          channels.channels
              .where((channel) => channel.isUsable)
              .map((channel) => channel.nextOutboundHtlcLimitSats),
        ) ??
        0;
    final inboundCapacitySats = maxInt(
          channels.channels
              .where((channel) => channel.isUsable)
              .map((channel) => channel.inboundCapacitySats),
        ) ??
        0;

    return TotalChannelBalance(
      outboundSendableSats: outboundSendableSats,
      inboundCapacitySats: inboundCapacitySats,
      fiatRate: fiatRate,
    );
  }

  @override
  int get hashCode =>
      this.outboundSendableSats.hashCode ^
      this.inboundCapacitySats.hashCode ^
      this.fiatRate.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is TotalChannelBalance &&
          runtimeType == other.runtimeType &&
          this.outboundSendableSats == other.outboundSendableSats &&
          this.inboundCapacitySats == other.inboundCapacitySats &&
          this.fiatRate == other.fiatRate;
}

class TotalChannelBalanceWidget extends StatelessWidget {
  const TotalChannelBalanceWidget(
      {super.key, required this.totalChannelBalance});

  final TotalChannelBalance? totalChannelBalance;

  @override
  Widget build(BuildContext context) {
    final fiatRate = this.totalChannelBalance?.fiatRate;
    final ourBalanceSats = this.totalChannelBalance?.outboundSendableSats;
    final theirBalanceSats = this.totalChannelBalance?.inboundCapacitySats;

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        TotalChannelBalanceRow(
          color: LxColors.moneyGoUp,
          primaryText: "Send up to",
          secondaryText: null,
          amountSats: ourBalanceSats,
          fiatRate: fiatRate,
        ),
        const SizedBox(height: Space.s300),
        TotalChannelBalanceRow(
          color: LxColors.moneyGoUpSecondary,
          primaryText: "Receive up to",
          secondaryText: "without miner fee",
          amountSats: theirBalanceSats,
          fiatRate: fiatRate,
        ),
      ],
    );
  }
}

class TotalChannelBalanceRow extends StatelessWidget {
  const TotalChannelBalanceRow({
    super.key,
    required this.color,
    required this.primaryText,
    required this.secondaryText,
    required this.amountSats,
    required this.fiatRate,
  });

  final Color? color;

  final String primaryText;
  final String? secondaryText;

  final int? amountSats;
  final FiatRate? fiatRate;

  @override
  Widget build(BuildContext context) {
    final fiatRate = this.fiatRate;

    final amountSats = this.amountSats;
    final amountFiat = FiatAmount.maybeFromSats(fiatRate, amountSats);

    final primaryStyle = Fonts.fontUI.copyWith(
      fontSize: Fonts.size400,
      fontVariations: [Fonts.weightMedium],
      // fontFeatures: [Fonts.featTabularNumbers],
      height: 1.25,
      letterSpacing: -0.5,
    );

    final Widget primaryAmount = (amountFiat != null)
        ? SplitAmountText(
            amount: amountFiat.amount,
            fiatName: amountFiat.fiat,
            style: primaryStyle,
          )
        : FilledTextPlaceholder(
            width: Space.s1000,
            style: primaryStyle,
          );

    final secondaryStyle = Fonts.fontUI.copyWith(
      fontSize: Fonts.size300,
      color: LxColors.fgTertiary,
      fontVariations: [Fonts.weightMedium],
      // fontFeatures: [Fonts.featTabularNumbers],
      height: 1.25,
      letterSpacing: -0.5,
    );

    final Widget secondaryAmount = (amountSats != null)
        ? Text(
            currency_format.formatSatsAmount(amountSats),
            style: secondaryStyle,
          )
        : FilledTextPlaceholder(
            width: Space.s900,
            style: secondaryStyle,
          );

    final Widget secondaryText = (this.secondaryText != null)
        ? Text(
            this.secondaryText!,
            style: secondaryStyle.copyWith(
              color: primaryStyle.color,
              fontVariations: [],
            ),
          )
        : const SizedBox();

    const dimCircle = Fonts.size500;
    const padCirclePrimary = Space.s200;

    final color = this.color;

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        Row(
          children: [
            if (color != null)
              Align(
                alignment: Alignment.centerLeft,
                child: Padding(
                  padding: const EdgeInsets.only(right: padCirclePrimary),
                  child: FilledCircle(size: dimCircle, color: color),
                ),
              ),
            Expanded(
              child: Text(
                this.primaryText,
                style: primaryStyle.copyWith(fontVariations: []),
              ),
            ),
            primaryAmount,
          ],
        ),
        const SizedBox(height: 1.0),
        Row(
          crossAxisAlignment: CrossAxisAlignment.baseline,
          textBaseline: TextBaseline.alphabetic,
          children: [
            const SizedBox(width: dimCircle + padCirclePrimary),
            Expanded(child: secondaryText),
            secondaryAmount,
          ],
        ),
      ],
    );
  }
}

class ChannelsList {
  const ChannelsList._({required this.maxValueSats, required this.channels});

  factory ChannelsList.fromApi(final ListChannelsResponse response) {
    // Project and sort the channels response
    final channels =
        response.channels.mapFrom(Channel.fromApi, growable: false);
    channels.sort();

    // Get the max channel value
    final maxValueSats =
        maxInt(channels.map((chan) => chan.channelValueSats)) ?? 0;

    return ChannelsList._(maxValueSats: maxValueSats, channels: channels);
  }

  final int maxValueSats;
  final List<Channel> channels;

  @override
  int get hashCode => this.maxValueSats.hashCode ^ this.channels.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ChannelsList &&
          this.runtimeType == other.runtimeType &&
          this.maxValueSats == other.maxValueSats &&
          this.channels == other.channels;
}

extension ListExt<T> on List<T> {
  List<U> mapFrom<U>(U Function(T t) mapper, {bool growable = false}) =>
      List.generate(this.length, (idx) => mapper(this[idx]),
          growable: growable);
}

/// Return the max int in an iterator.
// Getting weird type error when I try to do an extension method...
int? maxInt(Iterable<int> iter) {
  final first = iter.firstOrNull;
  if (first == null) return null;
  int value = first;
  for (final next in iter) {
    if (next > value) value = next;
  }
  return value;
}

/// The channel state we care about for this page's UI.
class Channel implements Comparable<Channel> {
  const Channel({
    required this.channelId,
    required this.isUsable,
    required this.channelValueSats,
    required this.ourBalanceSats,
    required this.theirBalanceSats,
  });

  Channel.fromApi(LxChannelDetails c)
      : channelId = c.channelId,
        isUsable = c.isUsable,
        channelValueSats = c.channelValueSats,
        ourBalanceSats = c.ourBalanceSats,
        theirBalanceSats = c.theirBalanceSats;

  final String channelId;
  final bool isUsable;
  final int channelValueSats;
  final int ourBalanceSats;
  final int theirBalanceSats;

  // How we sort the channels on the [ChannelsPage].
  @override
  int compareTo(Channel other) {
    final thisIsUsable = (this.isUsable) ? 1 : 0;
    final otherIsUsable = (other.isUsable) ? 1 : 0;

    // Sort usable channels before pending/closing channels
    final c0 = -thisIsUsable.compareTo(otherIsUsable);
    if (c0 != 0) return c0;

    // Sort larger channels first
    final c1 = -this.channelValueSats.compareTo(other.channelValueSats);
    if (c1 != 0) return c1;

    // Finally sort by channel id if all else equal
    return this.channelId.compareTo(other.channelId);
  }

  @override
  int get hashCode =>
      this.channelId.hashCode ^
      this.isUsable.hashCode ^
      this.channelValueSats.hashCode ^
      this.ourBalanceSats.hashCode ^
      this.theirBalanceSats.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is Channel &&
          this.runtimeType == other.runtimeType &&
          this.channelId == other.channelId &&
          this.isUsable == other.isUsable &&
          this.channelValueSats == other.channelValueSats &&
          this.ourBalanceSats == other.ourBalanceSats &&
          this.theirBalanceSats == other.theirBalanceSats;
}

/// High-level visualization of a single channel that the user has open with the
/// LSP. Displays the relative channel value and each sides' channel balance.
class ChannelsListEntry extends StatelessWidget {
  const ChannelsListEntry({
    super.key,
    required this.maxValueSats,
    required this.channel,
    required this.fiatRate,
  });

  final int maxValueSats;
  final Channel channel;

  /// Updating stream of fiat rates.
  final ValueListenable<FiatRate?> fiatRate;

  @override
  Widget build(BuildContext context) {
    final primaryStyle = Fonts.fontUI.copyWith(
      color: LxColors.foreground,
      fontSize: Fonts.size300,
      fontVariations: [Fonts.weightMedium],
      fontFeatures: [Fonts.featTabularNumbers],
      height: 1.25,
      letterSpacing: -0.5,
    );

    final secondaryStyle = primaryStyle.copyWith(color: LxColors.fgTertiary);

    final ourBalanceFiat = ValueListenableBuilder(
      valueListenable: this.fiatRate,
      builder: (context, fiatRate, child) => (fiatRate != null)
          ? SplitAmountText(
              amount: FiatAmount.fromSats(fiatRate, this.channel.ourBalanceSats)
                  .amount,
              fiatName: fiatRate.fiat,
              style: primaryStyle,
            )
          : FilledTextPlaceholder(
              width: Space.s1000,
              style: primaryStyle,
            ),
    );

    final ourBalanceSats = Text(
      currency_format.formatSatsAmount(this.channel.ourBalanceSats,
          satsSuffix: true),
      style: secondaryStyle,
    );

    final theirBalanceFiat = ValueListenableBuilder(
      valueListenable: this.fiatRate,
      builder: (context, fiatRate, child) => (fiatRate != null)
          ? Text(
              currency_format.formatFiat(
                  FiatAmount.fromSats(fiatRate, this.channel.theirBalanceSats)
                      .amount,
                  fiatRate.fiat),
              style: secondaryStyle,
            )
          : FilledTextPlaceholder(
              width: Space.s900,
              style: secondaryStyle,
            ),
    );

    final theirBalanceSats = Text(
      currency_format.formatSatsAmount(this.channel.theirBalanceSats,
          satsSuffix: true),
      style: secondaryStyle,
    );

    // Don't divide by zero :)
    final channelValueSats = this.channel.channelValueSats;
    final maxValueSats = this.maxValueSats;
    final value = (channelValueSats > 0)
        ? this.channel.ourBalanceSats / channelValueSats
        : 0.0;
    final width = (maxValueSats > 0) ? channelValueSats / maxValueSats : 0.0;

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 10.0),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          ChannelBalanceBarRow(
            value: value,
            width: width,
            isUsable: this.channel.isUsable,
          ),
          const SizedBox(height: Space.s200),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            crossAxisAlignment: CrossAxisAlignment.baseline,
            textBaseline: TextBaseline.alphabetic,
            children: [
              ourBalanceFiat,
              theirBalanceFiat,
            ],
          ),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            crossAxisAlignment: CrossAxisAlignment.baseline,
            textBaseline: TextBaseline.alphabetic,
            children: [
              ourBalanceSats,
              theirBalanceSats,
            ],
          ),
        ],
      ),
    );
  }
}

class ChannelBalanceBarRow extends StatelessWidget {
  const ChannelBalanceBarRow({
    super.key,
    required this.value,
    required this.width,
    required this.isUsable,
  });

  final double value;
  final double width;
  final bool isUsable;

  @override
  Widget build(BuildContext context) {
    final flex = max(5, min(100, (this.width * 100.0).truncate()));

    final bar = (this.isUsable)
        ? ChannelBalanceBar.usable(value: this.value)
        : ChannelBalanceBar.pending(value: this.value);

    return Row(
      children: [
        // Display small channels as proportionally smaller bars.
        Expanded(flex: flex, child: bar),
        // Show a spinner for opening/closing channels.
        // TODO(phlip9): add subdued "pending"/"closing" text here?
        if (!this.isUsable)
          const Padding(
            padding: EdgeInsets.only(left: Space.s300),
            child: SizedBox.square(
              dimension: 10.0,
              child: CircularProgressIndicator(
                strokeWidth: 2.0,
                color: LxColors.grey775,
                strokeCap: StrokeCap.round,
              ),
            ),
          ),
        Expanded(flex: 100 - flex, child: const SizedBox()),
      ],
    );
  }
}

/// The floating bottom sheet that contains the user's on-chain balance and
/// the open/close channel buttons.
class OnchainBottomSheet extends StatelessWidget {
  const OnchainBottomSheet({
    super.key,
    required this.balanceState,
    required this.onOpenPressed,
    required this.onClosedPressed,
  });

  final ValueListenable<BalanceState> balanceState;

  final VoidCallback onOpenPressed;
  final VoidCallback onClosedPressed;

  @override
  Widget build(BuildContext context) {
    return Stack(
      alignment: Alignment.topCenter,
      children: [
        // On-chain balance box
        Padding(
          padding: const EdgeInsets.only(top: Space.s600),
          child: DecoratedBox(
            decoration: const BoxDecoration(
              color: LxColors.grey1000,
              borderRadius: BorderRadius.only(
                topLeft: Radius.circular(LxRadius.r400),
                topRight: Radius.circular(LxRadius.r400),
              ),
            ),
            child: Padding(
              padding: const EdgeInsets.symmetric(
                horizontal: Space.s600,
                vertical: Space.s700,
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  // Heading
                  const Padding(
                    padding:
                        EdgeInsets.only(top: Space.s600, bottom: Space.s100),
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.center,
                      children: [
                        ListIcon.bitcoin(),
                        SizedBox(width: Space.s200),
                        Text(
                          "On-chain balance",
                          style: Fonts.fontHeadlineSmall,
                        ),
                      ],
                    ),
                  ),
                  const Text(
                    "Open Lightning channels using on-chain BTC",
                    style: Fonts.fontSubheading,
                  ),
                  const SizedBox(height: Space.s400),

                  ValueListenableBuilder(
                    valueListenable: this.balanceState,
                    builder: (context, balanceState, child) =>
                        TotalChannelBalanceRow(
                      color: null,
                      primaryText: "Send up to",
                      secondaryText: null,
                      amountSats: balanceState.onchainSats(),
                      fiatRate: balanceState.fiatRate,
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),

        // Open/Close channel buttons
        Positioned(
          top: 0.0,
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            mainAxisSize: MainAxisSize.max,
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              ChannelButton(
                label: "Open",
                icon: LxIcons.openChannel,
                onPressed: this.onOpenPressed,
              ),
              const SizedBox(width: Space.s700),
              ChannelButton(
                label: "Close",
                icon: LxIcons.closeChannel,
                onPressed: this.onClosedPressed,
              ),
            ],
          ),
        ),
      ],
    );
  }
}

/// One of the big open or close buttons
class ChannelButton extends StatelessWidget {
  const ChannelButton({
    super.key,
    required this.label,
    required this.onPressed,
    required this.icon,
  });

  final String label;
  final IconData icon;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        FilledButton(
          onPressed: onPressed,
          style: const ButtonStyle(
            side: WidgetStatePropertyAll(
              BorderSide(
                color: LxColors.background,
                width: 6.0,
                style: BorderStyle.solid,
                strokeAlign: BorderSide.strokeAlignOutside,
              ),
            ),
          ),
          child: Padding(
            padding: const EdgeInsets.symmetric(
              horizontal: Space.s450,
            ),
            child: Icon(
              this.icon,
              size: Fonts.size700,
              weight: 700,
            ),
          ),
        ),
        const SizedBox(height: Space.s300),
        Text(
          this.label,
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size500,
            fontVariations: [Fonts.weightSemiBold],
            letterSpacing: -0.5,
          ),
        ),
      ],
    );
  }
}
