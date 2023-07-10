// Send payment page

import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show LxCloseButton, LxCloseButtonKind, ScrollableSinglePageBody;

import '../../bindings.dart' show api;
import '../../bindings_generated_api.dart' show Network;
import '../../logger.dart' show info;
import '../../style.dart' show Fonts, LxColors, Space;

class SendPaymentPage extends StatefulWidget {
  const SendPaymentPage({super.key, required this.configNetwork});

  final Network configNetwork;

  @override
  State<StatefulWidget> createState() => SendPaymentPageState();
}

class SendPaymentPageState extends State<SendPaymentPage> {
  final GlobalKey<FormFieldState<String>> addressFieldKey = GlobalKey();

  void onQrPressed() {
    info("pressed QR button");
  }

  void onNext() {
    final fieldState = this.addressFieldKey.currentState!;
    if (fieldState.validate()) {
      final address = fieldState.value;
      info("success: address = $address");
    }
  }

  /// Ensure the bitcoin address is properly formatted and targets the right
  /// bitcoin network (mainnet, testnet, regtest) for our build.
  String? validateBitcoinAddress(String? addressStr) {
    if (addressStr == null || addressStr.isEmpty) {
      return "Please enter an address";
    }

    return api.formValidateBitcoinAddress(
      addressStr: addressStr,
      currentNetwork: this.widget.configNetwork,
    );
  }

  @override
  Widget build(BuildContext context) {
    // TODO(phlip9): autofill address from user's clipboard if one exists

    return Scaffold(
      appBar: AppBar(
        leading: const LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
        actions: [
          IconButton(
            onPressed: this.onQrPressed,
            icon: const Icon(Icons.qr_code_rounded),
          ),
          const SizedBox(width: Space.s100),
        ],
      ),
      body: ScrollableSinglePageBody(
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
            key: this.addressFieldKey,
            autofocus: true,
            // `visiblePassword` gives ready access to letters + numbers
            keyboardType: TextInputType.visiblePassword,
            textDirection: TextDirection.ltr,
            textInputAction: TextInputAction.next,
            validator: this.validateBitcoinAddress,
            onEditingComplete: this.onNext,
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
          onPressed: this.onNext,
          style: FilledButton.styleFrom(
            backgroundColor: LxColors.grey1000,
            disabledBackgroundColor: LxColors.grey850,
            foregroundColor: LxColors.foreground,
            disabledForegroundColor: LxColors.grey725,
            maximumSize: const Size.fromHeight(Space.s700),
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
    );
  }
}
