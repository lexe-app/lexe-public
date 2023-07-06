// Send payment page

import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show LxCloseButton, LxCloseButtonKind, ScrollableSinglePageBody;

import '../../logger.dart' show info;
import '../../style.dart' show Fonts, LxColors, Space;

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
        leading: const LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
        actions: [
          IconButton(
            onPressed: () => info("pressed QR button"),
            icon: const Icon(Icons.qr_code_rounded),
          ),
          const SizedBox(width: Space.s100),
        ],
      ),
      body: Form(
        key: this.formKey,
        child: ScrollableSinglePageBody(
          body: [
            const SizedBox(height: Space.s500),
            Text(
              "Who are we paying?",
              style: Fonts.fontUI.copyWith(
                fontSize: Fonts.size600,
                fontVariations: [Fonts.weightMedium],
                letterSpacing: -0.5,
              ),
            ),
            const SizedBox(height: Space.s600),
            TextFormField(
              autofocus: true,
              decoration: const InputDecoration.collapsed(
                hintText: "Bitcoin address",
                hintStyle: TextStyle(
                  color: LxColors.grey750,
                ),
              ),
              style: Fonts.fontUI.copyWith(
                fontSize: Fonts.size700,
                fontVariations: [Fonts.weightMedium],
                // Use unambiguous character alternatives (0OIl1) to avoid
                // confusion in the unfortunate event that a user has to
                // manually type in an address.
                fontFeatures: [Fonts.featDisambugation],
                letterSpacing: -0.5,
              ),
            ),
            const SizedBox(height: Space.s800),
          ],
          bottom: FilledButton(
            onPressed: () {},
            style: FilledButton.styleFrom(
              backgroundColor: LxColors.grey975,
              disabledBackgroundColor: LxColors.grey850,
              foregroundColor: LxColors.foreground,
              disabledForegroundColor: LxColors.grey725,
              fixedSize: const Size(300.0, Space.s700),
            ),
            child: Stack(
              alignment: Alignment.center,
              children: [
                Text(
                  "Next",
                  style: Fonts.fontInter.copyWith(
                    fontSize: Fonts.size300,
                    fontVariations: [Fonts.weightMedium],
                  ),
                ),
                const Align(
                  alignment: Alignment.centerRight,
                  child: Padding(
                    padding: EdgeInsets.symmetric(vertical: Space.s200),
                    child: Icon(
                      Icons.arrow_forward_rounded,
                      size: Fonts.size300,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
