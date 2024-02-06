import 'package:flutter/material.dart';

import '../components.dart'
    show HeadingText, LxCloseButton, ScrollableSinglePageBody;

import '../style.dart' show Space;

class PaymentDetailPage extends StatefulWidget {
  const PaymentDetailPage({super.key, required this.vecIdx});

  final int vecIdx;

  @override
  State<PaymentDetailPage> createState() => _PaymentDetailPageState();
}

class _PaymentDetailPageState extends State<PaymentDetailPage> {
  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(),
      ),
      body: const PaymentDetailBody(),
    );
  }
}

class PaymentDetailBody extends StatelessWidget {
  const PaymentDetailBody({super.key});

  @override
  Widget build(BuildContext context) {
    return const ScrollableSinglePageBody(body: [
      HeadingText(text: "PaymentDetailBody"),
    ]);
  }
}
