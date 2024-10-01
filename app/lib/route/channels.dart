import 'package:app_rs_dart/ffi/api.dart' show FiatRate;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show LxChannelDetails;
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        FilledCircle,
        FilledTextPlaceholder,
        ListIcon,
        LxBackButton,
        LxRefreshButton,
        ScrollableSinglePageBody,
        SplitAmountText;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/style.dart' show Fonts, LxColors, Space;

class ChannelsPage extends StatefulWidget {
  const ChannelsPage({super.key, required this.app});

  final AppHandle app;

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

  final ValueNotifier<List<LxChannelDetails>?> channels = ValueNotifier(null);

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
        body: [
          // Heading
          const Padding(
            padding: EdgeInsets.only(top: Space.s300, bottom: Space.s100),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: [
                ListIcon.lightning(),
                SizedBox(width: Space.s200),
                Text("Lightning channels", style: Fonts.fontHeadlineSmall),
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
        ],
      ),
    );
  }
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
      fontFeatures: [Fonts.featTabularNumbers],
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
      fontFeatures: [Fonts.featTabularNumbers],
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
