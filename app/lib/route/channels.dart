import 'dart:math' show max, min;

import 'package:app_rs_dart/ffi/api.dart' show FiatRate;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show LxChannelDetails;
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        ChannelBalanceBar,
        FilledCircle,
        FilledTextPlaceholder,
        ListIcon,
        LxBackButton,
        LxRefreshButton,
        ScrollableSinglePageBody,
        SplitAmountText,
        ValueStreamBuilder;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxRadius, Space;
import 'package:rxdart/rxdart.dart';

class ChannelsPage extends StatefulWidget {
  const ChannelsPage({super.key, required this.app, required this.fiatRate});

  final AppHandle app;

  /// Updating stream of fiat rates.
  final ValueStream<FiatRate?> fiatRate;

  @override
  State<ChannelsPage> createState() => _ChannelsPageState();
}

class _ChannelsPageState extends State<ChannelsPage> {
  // TODO(phlip9): impl
  final ValueNotifier<bool> isRefreshing = ValueNotifier(false);
  // final ValueNotifier<TotalChannelBalance?> totalChannelBalance =
  //     ValueNotifier(null);
  final ValueNotifier<TotalChannelBalance?> totalChannelBalance = ValueNotifier(
    const TotalChannelBalance(
      ourBalanceSats: 1775231,
      theirBalanceSats: 226787,
      fiatRate: FiatRate(fiat: "USD", rate: 63344.0),
    ),
  );
  // final ValueNotifier<ChannelsList?> channels = ValueNotifier(null);
  final ValueNotifier<ChannelsList?> channels =
      ValueNotifier(const ChannelsList(
    maxValueSats: 776231 + 226787,
    channels: [
      Channel(
        channelId:
            "261111111111111116f7e7e2d110b0c67bc1f01b9bb9a89bbe98c144f0f4b04c",
        isUsable: false,
        channelValueSats: 776231 + 226787,
        ourBalanceSats: 776231,
        theirBalanceSats: 226787,
      ),
      Channel(
        channelId:
            "262222222222222226f7e7e2d110b0c67bc1f01b9bb9a89bbe98c144f0f4b04c",
        isUsable: true,
        channelValueSats: 300231 + 477788,
        ourBalanceSats: 300231,
        theirBalanceSats: 477788,
      ),
      Channel(
        channelId:
            "263333333333333336f7e7e2d110b0c67bc1f01b9bb9a89bbe98c144f0f4b04c",
        isUsable: false,
        channelValueSats: 254116 + 43844,
        ourBalanceSats: 254116,
        theirBalanceSats: 43844,
      ),
    ],
  ));

  @override
  void dispose() {
    this.isRefreshing.dispose();
    this.totalChannelBalance.dispose();
    this.channels.dispose();

    super.dispose();
  }

  Future<void> triggerRefresh() async {
    // TODO(phlip9): impl
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
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
      body: ScrollableSinglePageBody(
        bodySlivers: [
          SliverToBoxAdapter(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                // Heading
                const Padding(
                  padding: EdgeInsets.only(top: Space.s300, bottom: Space.s100),
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
          ValueListenableBuilder(
            valueListenable: this.channels,
            builder: (context, channelsList, child) =>
                SliverFixedExtentList.list(
              itemExtent: Space.s850,
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
        ],
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

class FiatAmount {
  const FiatAmount({required this.fiat, required this.amount});

  FiatAmount.fromBtc(FiatRate rate, double amountBtc)
      : fiat = rate.fiat,
        amount = amountBtc * rate.rate;

  factory FiatAmount.fromSats(FiatRate rate, int amountSats) =>
      FiatAmount.fromBtc(rate, currency_format.satsToBtc(amountSats));

  static FiatAmount? maybeFromSats(FiatRate? rate, int? amountSats) =>
      (rate != null && amountSats != null)
          ? FiatAmount.fromSats(rate, amountSats)
          : null;

  final String fiat;
  final double amount;

  @override
  int get hashCode => this.fiat.hashCode ^ this.amount.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is FiatAmount &&
          runtimeType == other.runtimeType &&
          this.fiat == other.fiat &&
          this.amount == other.amount;
}

class TotalChannelBalance {
  const TotalChannelBalance({
    required this.ourBalanceSats,
    required this.theirBalanceSats,
    required this.fiatRate,
  });

  final int ourBalanceSats;
  final int theirBalanceSats;

  final FiatRate? fiatRate;

  @override
  int get hashCode =>
      this.ourBalanceSats.hashCode ^
      this.theirBalanceSats.hashCode ^
      this.fiatRate.hashCode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is TotalChannelBalance &&
          runtimeType == other.runtimeType &&
          this.ourBalanceSats == other.ourBalanceSats &&
          this.theirBalanceSats == other.theirBalanceSats &&
          this.fiatRate == other.fiatRate;
}

class TotalChannelBalanceWidget extends StatelessWidget {
  const TotalChannelBalanceWidget(
      {super.key, required this.totalChannelBalance});

  final TotalChannelBalance? totalChannelBalance;

  @override
  Widget build(BuildContext context) {
    final fiatRate = this.totalChannelBalance?.fiatRate;
    final ourBalanceSats = this.totalChannelBalance?.ourBalanceSats;
    final theirBalanceSats = this.totalChannelBalance?.theirBalanceSats;

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

  final Color color;

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
            style: primaryStyle,
          );

    final Widget secondaryText = (this.secondaryText != null)
        ? Text(this.secondaryText!, style: secondaryStyle)
        : const SizedBox();

    const dimCircle = Fonts.size500;
    const padCirclePrimary = Space.s200;

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        Row(
          // crossAxisAlignment: CrossAxisAlignment.baseline,
          // textBaseline: TextBaseline.alphabetic,
          children: [
            Align(
              alignment: Alignment.centerLeft,
              child: FilledCircle(size: dimCircle, color: this.color),
            ),
            const SizedBox(width: padCirclePrimary),
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
  const ChannelsList({required this.maxValueSats, required this.channels});

  final int maxValueSats;
  final List<Channel> channels;
}

class Channel {
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
}

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
  final ValueStream<FiatRate?> fiatRate;

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

    final ourBalanceFiat = ValueStreamBuilder(
      stream: this.fiatRate,
      builder: (context, fiatRate) => (fiatRate != null)
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

    final theirBalanceFiat = ValueStreamBuilder(
      stream: this.fiatRate,
      builder: (context, fiatRate) => (fiatRate != null)
          ? Text(
              currency_format.formatFiat(
                  FiatAmount.fromSats(fiatRate, this.channel.ourBalanceSats)
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

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 10.0),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          ChannelBalanceBarRow(
            value: this.channel.ourBalanceSats / this.channel.channelValueSats,
            width: this.channel.channelValueSats / this.maxValueSats,
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
