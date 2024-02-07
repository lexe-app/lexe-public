import 'package:flutter/material.dart';

import '../bindings_generated_api.dart'
    show AppHandle, Payment, PaymentDirection, PaymentStatus;
import '../components.dart'
    show HeadingText, LxCloseButton, ScrollableSinglePageBody, SubheadingText;
import '../style.dart' show Space;

String primaryPaymentLabel(PaymentStatus status, PaymentDirection direction) {
  if (status == PaymentStatus.Pending) {
    if (direction == PaymentDirection.Inbound) {
      return "Receiving payment";
    } else {
      return "Sending payment";
    }
  } else {
    if (direction == PaymentDirection.Inbound) {
      return "You received";
    } else {
      return "You sent";
    }
  }
}

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
    final status = this.payment.status;
    final direction = this.payment.direction;

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(),
      ),
      body: ScrollableSinglePageBody(body: [
        HeadingText(text: primaryPaymentLabel(status, direction)),
        SubheadingText(text: this.payment.note ?? ""),
      ]),
    );
  }
}
