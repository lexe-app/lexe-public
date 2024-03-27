import 'package:flutter/material.dart';

import 'package:lexeapp/components.dart'
    show HeadingText, LxBackButton, ScrollableSinglePageBody;
import 'package:lexeapp/style.dart' show Space;

class ReceivePaymentPage extends StatefulWidget {
  const ReceivePaymentPage({super.key});

  @override
  State<ReceivePaymentPage> createState() => ReceivePaymentPageState();
}

class ReceivePaymentPageState extends State<ReceivePaymentPage> {
  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(),
      ),
      body: const ScrollableSinglePageBody(body: [
        HeadingText(text: "Receive payment"),
        SizedBox(height: Space.s400),
      ]),
    );
  }
}
