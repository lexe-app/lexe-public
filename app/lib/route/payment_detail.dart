import 'dart:async' show Timer;

import 'package:flutter/material.dart';
import 'package:rxdart_ext/rxdart_ext.dart';

import '../bindings_generated_api.dart'
    show
        AppHandle,
        FiatRate,
        Payment,
        PaymentDirection,
        PaymentKind,
        PaymentStatus;
import '../components.dart'
    show LxCloseButton, ScrollableSinglePageBody, StateStreamBuilder;
import '../currency_format.dart' as currency_format;
import '../date_format.dart' as date_format;
import '../logger.dart';
import '../stream_ext.dart';
import '../style.dart' show Fonts, LxColors, Space;

/// A page for displaying a single payment, in detail.
///
/// Ex: tapping a payment in the wallet page payments list will open this page
/// for that payment.
class PaymentDetailPage extends StatefulWidget {
  const PaymentDetailPage({
    super.key,
    required this.app,
    required this.vecIdx,
  });

  final AppHandle app;
  final int vecIdx;

  @override
  State<PaymentDetailPage> createState() => _PaymentDetailPageState();
}

class _PaymentDetailPageState extends State<PaymentDetailPage> {
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

  @override
  Widget build(BuildContext context) {
    final vecIdx = this.widget.vecIdx;
    final payment = this.widget.app.getPaymentByVecIdx(vecIdx: vecIdx);

    if (payment == null) {
      throw StateError(
          "PaymentDb is in an invalid state: missing payment @ vec_idx: $vecIdx");
    }

    return PaymentDetailPageInner(
      payment: payment,
      paymentDateUpdates: this.paymentDateUpdates,
    );
  }
}

class PaymentDetailPageInner extends StatelessWidget {
  const PaymentDetailPageInner({
    super.key,
    required this.payment,
    required this.paymentDateUpdates,
  });

  final Payment payment;
  final StateStream<DateTime> paymentDateUpdates;

  @override
  Widget build(BuildContext context) {
    final kind = this.payment.kind;
    final status = this.payment.status;
    final direction = this.payment.direction;
    final createdAt =
        DateTime.fromMillisecondsSinceEpoch(this.payment.createdAt);

    final maybeAmountSat = this.payment.amountSat;

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(),
        actions: [
          IconButton(
            icon: const Icon(Icons.refresh_rounded),
            onPressed: () => info("payment detail: refresh pressed"),
          ),
          const SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(body: [
        const SizedBox(height: Space.s500),

        // Big LN/BTC icon + status badge
        Align(
          alignment: Alignment.topCenter,
          child: PaymentDetailIcon(
            kind: kind,
            status: status,
          ),
        ),

        const SizedBox(height: Space.s500),

        // Direction + short time
        StateStreamBuilder(
          stream: this.paymentDateUpdates,
          builder: (_, now) => PaymentDetailDirectionTime(
            status: status,
            direction: direction,
            createdAt: createdAt,
            now: now,
          ),
        ),
        const SizedBox(height: Space.s400),

        // If pending or failed, show a card with more info on the current
        // status.
        if (status != PaymentStatus.Completed)
          Padding(
            padding: const EdgeInsets.only(bottom: Space.s400),
            child: PaymentDetailStatusCard(
              status: status,
              statusStr: this.payment.statusStr,
            ),
          ),

        const SizedBox(height: Space.s600),

        if (maybeAmountSat != null)
          PaymentDetailPrimaryAmount(
            status: status,
            direction: direction,
            amountSat: maybeAmountSat,
            fiatName: "USD",
            fiatRate: const FiatRate(fiat: "USD", rate: 73021.29890205512),
          ),

        const SizedBox(height: Space.s600),
      ]),
    );
  }
}

class PaymentDetailIcon extends StatelessWidget {
  const PaymentDetailIcon({
    super.key,
    required this.kind,
    required this.status,
  });

  final PaymentKind kind;
  final PaymentStatus status;

  @override
  Widget build(BuildContext context) {
    final iconData = switch (this.kind) {
      PaymentKind.Onchain => Icons.currency_bitcoin_rounded,
      PaymentKind.Invoice || PaymentKind.Spontaneous => Icons.bolt_rounded,
    };

    final icon = DecoratedBox(
      decoration: const BoxDecoration(
        color: LxColors.grey825,
        borderRadius: BorderRadius.all(Radius.circular(Space.s800 / 2)),
      ),
      child: SizedBox.square(
        dimension: Space.s800,
        child: Icon(
          iconData,
          size: Space.s700,
          color: LxColors.fgSecondary,
        ),
      ),
    );

    return switch (this.status) {
      PaymentStatus.Completed => PaymentDetailIconBadge(
          icon: Icons.check_rounded,
          color: LxColors.background,
          backgroundColor: LxColors.moneyGoUp,
          child: icon,
        ),
      PaymentStatus.Pending => PaymentDetailIconBadge(
          icon: Icons.sync_rounded,
          color: LxColors.background,
          // Use "green" also for pending. Assume payments will generally be
          // successful. Don't scare users.
          // TODO(phlip9): use a warning yellow after several hours of pending?
          backgroundColor: LxColors.moneyGoUp,
          child: icon,
        ),
      PaymentStatus.Failed => PaymentDetailIconBadge(
          icon: Icons.close_rounded,
          color: LxColors.background,
          backgroundColor: LxColors.errorText,
          child: icon,
        ),
    };
  }
}

class PaymentDetailIconBadge extends StatelessWidget {
  const PaymentDetailIconBadge({
    super.key,
    required this.icon,
    required this.color,
    required this.backgroundColor,
    required this.child,
  });

  final IconData icon;
  final Color color;
  final Color backgroundColor;
  final Widget child;

  @override
  Widget build(BuildContext context) => Badge(
        label: Icon(
          this.icon,
          size: Fonts.size400,
          color: this.color,
        ),
        backgroundColor: this.backgroundColor,
        largeSize: Space.s500,
        child: this.child,
      );
}

class PaymentDetailDirectionTime extends StatelessWidget {
  const PaymentDetailDirectionTime({
    super.key,
    required this.status,
    required this.direction,
    required this.createdAt,
    required this.now,
  });

  final PaymentStatus status;
  final PaymentDirection direction;
  final DateTime createdAt;
  final DateTime now;

  @override
  Widget build(BuildContext context) {
    final String directionLabel;
    if (status == PaymentStatus.Pending) {
      if (direction == PaymentDirection.Inbound) {
        directionLabel = "Receiving";
      } else {
        directionLabel = "Sending";
      }
    } else {
      if (direction == PaymentDirection.Inbound) {
        directionLabel = "Received";
      } else {
        directionLabel = "Sent";
      }
    }

    final createdAtStr = date_format.formatDateCompact(
        then: createdAt, now: now, formatSeconds: false);

    return RichText(
      text: TextSpan(
        children: <TextSpan>[
          TextSpan(
            text: directionLabel,
            style: const TextStyle(fontVariations: [Fonts.weightSemiBold]),
          ),
          const TextSpan(text: " · "),
          TextSpan(
              text: createdAtStr,
              style: const TextStyle(color: LxColors.fgSecondary)),
        ],
        style: Fonts.fontBody.copyWith(
          // letterSpacing: -0.5,
          fontSize: Fonts.size300,
          fontVariations: [Fonts.weightMedium],
        ),
      ),
      textAlign: TextAlign.center,
    );
  }
}

class PaymentDetailStatusCard extends StatelessWidget {
  const PaymentDetailStatusCard(
      {super.key, required this.status, required this.statusStr})
      : assert(status != PaymentStatus.Completed);

  final PaymentStatus status;
  final String statusStr;

  @override
  Widget build(BuildContext context) {
    return Card(
      color: LxColors.grey1000,
      elevation: 0.0,
      margin: const EdgeInsets.all(Space.s200),
      child: Padding(
        padding: const EdgeInsets.all(Space.s400),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            Expanded(
              flex: 2,
              child: Text(
                (this.status == PaymentStatus.Pending) ? "pending" : "failed",
                style: Fonts.fontBody.copyWith(
                  fontSize: Fonts.size300,
                  color: LxColors.foreground,
                  fontVariations: [Fonts.weightSemiBold],
                  height: 1.0,
                ),
                textAlign: TextAlign.center,
              ),
            ),
            const SizedBox(width: Space.s400),
            Expanded(
              flex: 4,
              child: Text(
                this.statusStr,
                style: Fonts.fontBody.copyWith(
                  letterSpacing: -0.25,
                  fontSize: Fonts.size200,
                  color: LxColors.fgSecondary,
                  fontVariations: [Fonts.weightNormal],
                  height: 1.3,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class PaymentDetailPrimaryAmount extends StatelessWidget {
  const PaymentDetailPrimaryAmount({
    super.key,
    required this.status,
    required this.direction,
    required this.amountSat,
    required this.fiatName,
    this.fiatRate,
  });

  final PaymentStatus status;
  final PaymentDirection direction;
  final int amountSat;
  final String fiatName;
  final FiatRate? fiatRate;

  String? maybeAmountFiatStr() {
    final fiatRate = this.fiatRate;
    if (fiatRate == null) {
      return null;
    }

    final amountBtc = currency_format.satsToBtc(this.amountSat);
    final amountFiat = amountBtc * fiatRate.rate;
    return currency_format.formatFiat(amountFiat, this.fiatName);
  }

  @override
  Widget build(BuildContext context) {
    final amountSatsStr = currency_format.formatSatsAmount(this.amountSat,
        direction: this.direction, satsSuffix: true);

    final maybeAmountFiatStr = this.maybeAmountFiatStr();

    final amountColor = switch ((this.status, this.direction)) {
      ((PaymentStatus.Failed, _)) => LxColors.fgTertiary,
      ((_, PaymentDirection.Inbound)) => LxColors.moneyGoUp,
      ((_, PaymentDirection.Outbound)) => LxColors.fgSecondary,
    };

    return Column(
      mainAxisAlignment: MainAxisAlignment.start,
      children: [
        Text(
          amountSatsStr,
          style: Fonts.fontUI.copyWith(
            letterSpacing: -0.5,
            fontSize: Fonts.size800,
            fontVariations: [Fonts.weightNormal],
            fontFeatures: [Fonts.featSlashedZero],
            color: amountColor,
          ),
          textAlign: TextAlign.center,
        ),
        if (maybeAmountFiatStr != null)
          Padding(
            padding: const EdgeInsets.only(top: Space.s300),
            child: Text(
              "≈ $maybeAmountFiatStr",
              style: Fonts.fontUI.copyWith(
                letterSpacing: -0.5,
                fontSize: Fonts.size500,
                fontVariations: [Fonts.weightNormal],
                fontFeatures: [Fonts.featSlashedZero],
                color: LxColors.fgTertiary,
              ),
              textAlign: TextAlign.center,
            ),
          ),
      ],
    );
  }
}
