// Send payment page

import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart' show LxCloseButton, LxCloseButtonKind;

import '../../style.dart' show LxColors;

class SendPaymentPage extends StatefulWidget {
  const SendPaymentPage({super.key});

  @override
  State<StatefulWidget> createState() => SendPaymentPageState();
}

class SendPaymentPageState extends State<SendPaymentPage> {
  final formKey = GlobalKey<FormState>();

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        // header shadow effect
        // not sure I like how this looks...
        scrolledUnderElevation: 1.0,
        shadowColor: LxColors.background,
        surfaceTintColor: LxColors.clearB0,

        leading: const LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),

        title: const Text("Send payment"),

        centerTitle: false,
      ),
      body: const Center(),
      //
      // body: Padding(
      //   padding: const EdgeInsets.symmetric(
      //     horizontal: Space.s200,
      //     vertical: Space.s100,
      //   ),
      //   child: Column(
      //     children: [
      //       Text()
      //     ],
      //   ),
      // )
      //
      // body: Form(
      //   key: this.formKey,
      //   child: Padding(
      //     padding: const EdgeInsets.symmetric(horizontal: Space.s200),
      //     child: Column(
      //       children: [
      //         // Bitcoin address
      //         TextFormField(
      //           decoration: const InputDecoration(
      //             labelText: "Bitcoin address",
      //           ),
      //           validator: (value) {
      //             if (value == null || value.isEmpty) {
      //               return "Bitcoin address field can't be empty";
      //             }
      //             return null;
      //           },
      //         ),
      //
      //         // Bitcoin address
      //         TextFormField(
      //           decoration: const InputDecoration(
      //             labelText: "Bitcoin address",
      //           ),
      //           validator: (value) {
      //             if (value == null || value.isEmpty) {
      //               return "Bitcoin address field can't be empty";
      //             }
      //             return null;
      //           },
      //         ),
      //       ],
      //     ),
      //   ),
      // ),
    );
  }
}
