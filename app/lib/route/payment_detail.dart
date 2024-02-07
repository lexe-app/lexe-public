import 'package:flutter/material.dart';

import '../bindings_generated_api.dart'
    show AppHandle, Payment, PaymentDirection, PaymentKind, PaymentStatus;
import '../components.dart'
    show HeadingText, LxCloseButton, ScrollableSinglePageBody, SubheadingText;
import '../logger.dart';
import '../style.dart' show Fonts, LxColors, Space;

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
  @override
  Widget build(BuildContext context) {
    final vecIdx = this.widget.vecIdx;
    final payment = this.widget.app.getPaymentByVecIdx(vecIdx: vecIdx);

    if (payment == null) {
      throw StateError(
          "PaymentDb is in an invalid state: missing payment @ vec_idx: $vecIdx");
    }

    return PaymentDetailPageInner(payment: payment);
  }
}

class PaymentDetailPageInner extends StatelessWidget {
  const PaymentDetailPageInner({super.key, required this.payment});

  final Payment payment;

  @override
  Widget build(BuildContext context) {
    final kind = this.payment.kind;
    final status = this.payment.status;
    final direction = this.payment.direction;

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
        Align(
          alignment: Alignment.topCenter,
          child: PaymentDetailIcon(
            kind: kind,
            status: status,
          ),
        )
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
          backgroundColor: LxColors.fgSecondary,
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
