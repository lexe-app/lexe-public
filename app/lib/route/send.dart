// Send payment page

import 'package:flutter/material.dart';

import 'package:lexeapp/components.dart'
    show
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        ScrollableSinglePageBody;

import '../../bindings.dart' show api;
import '../../bindings_generated_api.dart' show Network;
import '../../currency_format.dart' as currency_format;
import '../../logger.dart' show info;
import '../../result.dart';
import '../../style.dart' show Fonts, LxColors, Space;

@immutable
final class SendContext {
  const SendContext({
    required this.configNetwork,
    required this.balanceSats,
  });

  final Network configNetwork;
  final int balanceSats;
}

class SendPaymentPage extends StatelessWidget {
  const SendPaymentPage({
    super.key,
    required this.sendCtx,
  });

  final SendContext sendCtx;

  @override
  Widget build(BuildContext context) {
    return Navigator(
      onGenerateRoute: (RouteSettings settings) => MaterialPageRoute(
        builder: (context) => SendPaymentAddressPage(sendCtx: this.sendCtx),
        settings: settings,
      ),
    );
  }
}

class SendPaymentAddressPage extends StatefulWidget {
  const SendPaymentAddressPage({
    super.key,
    required this.sendCtx,
  });

  final SendContext sendCtx;

  @override
  State<StatefulWidget> createState() => SendPaymentAddressPageState();
}

class SendPaymentAddressPageState extends State<SendPaymentAddressPage> {
  final GlobalKey<FormFieldState<String>> addressFieldKey = GlobalKey();

  void onQrPressed() {
    info("pressed QR button");
  }

  void onNext() {
    final fieldState = this.addressFieldKey.currentState!;
    if (!fieldState.validate()) {
      return;
    }

    final address = fieldState.value!;

    Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_) => SendPaymentAmountPage(
        sendCtx: this.widget.sendCtx,
        address: address,
      ),
    ));
  }

  /// Ensure the bitcoin address is properly formatted and targets the right
  /// bitcoin network (mainnet, testnet, regtest) for our build.
  String? validateBitcoinAddress(String? addressStr) {
    if (addressStr == null || addressStr.isEmpty) {
      return "Please enter an address";
    }

    return api.formValidateBitcoinAddress(
      currentNetwork: this.widget.sendCtx.configNetwork,
      addressStr: addressStr,
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

class SendPaymentAmountPage extends StatefulWidget {
  const SendPaymentAmountPage({
    super.key,
    required this.sendCtx,
    required this.address,
  });

  final SendContext sendCtx;
  final String address;

  @override
  State<SendPaymentAmountPage> createState() => _SendPaymentAmountPageState();
}

class _SendPaymentAmountPageState extends State<SendPaymentAmountPage> {
  final GlobalKey<FormFieldState<String>> amountFieldKey = GlobalKey();
  final ValueNotifier<String?> errorText = ValueNotifier(null);

  final currency_format.IntInputFormatter intInputFormatter =
      currency_format.IntInputFormatter();

  @override
  void dispose() {
    this.errorText.dispose();

    super.dispose();
  }

  void onNext() {
    final num amountSats;
    switch (this.validate()) {
      case Ok(:final ok):
        this.errorText.value = null;
        amountSats = ok;

      case Err(:final err):
        // Display the error message
        this.errorText.value = err;
        return;
    }

    info("amountSats = $amountSats");
  }

  Result<int, String?> validate() {
    final maybeAmountStr = this.amountFieldKey.currentState!.value;
    if (maybeAmountStr == null || maybeAmountStr.isEmpty) {
      return const Err(null);
    }

    final int amount;
    switch (this.intInputFormatter.tryParse(maybeAmountStr)) {
      case int x:
        amount = x;
      case null:
        return const Err("Amount must be a number");
    }

    if (amount <= 0) {
      return const Err(null);
    }

    if (amount > this.widget.sendCtx.balanceSats) {
      return const Err("You can't send more than your balance!");
    }

    return Ok(amount);
  }

  @override
  Widget build(BuildContext context) {
    final balanceStr = currency_format
        .formatSatsAmount(this.widget.sendCtx.balanceSats, satsSuffix: true);

    return Scaffold(
      appBar: AppBar(
        leading: const LxBackButton(),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.s100),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const SizedBox(height: Space.s500),
          Text(
            "How much?",
            textAlign: TextAlign.left,
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size600,
              fontVariations: [Fonts.weightMedium],
              letterSpacing: -0.5,
            ),
          ),
          const SizedBox(height: Space.s200),
          Text(
            "balance $balanceStr",
            textAlign: TextAlign.left,
            style: Fonts.fontUI.copyWith(
              color: LxColors.grey600,
              fontSize: Fonts.size300,
            ),
          ),
          const SizedBox(height: Space.s850),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceAround,
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              Expanded(
                // TODO(phlip9): figure out how to shrink the font size when we
                // overflow so the value is always on-screen.
                // TODO(phlip9): I wish I could just use the builtin error text
                // but I have no idea how to center align the text so it doesn't
                // look totally weird...
                child: TextFormField(
                  key: this.amountFieldKey,
                  autofocus: true,
                  keyboardType: const TextInputType.numberWithOptions(
                    signed: false,
                    decimal: false,
                  ),
                  textDirection: TextDirection.ltr,
                  textInputAction: TextInputAction.next,
                  textAlign: TextAlign.right,
                  onEditingComplete: this.onNext,
                  // Hide the error message again when the user starts
                  // typing again
                  onChanged: (_) => this.errorText.value = null,
                  decoration: const InputDecoration.collapsed(
                    hintText: "0",
                    hintStyle: TextStyle(
                      color: LxColors.grey750,
                    ),
                  ),
                  inputFormatters: [this.intInputFormatter],
                  style: Fonts.fontUI.copyWith(
                    fontSize: Fonts.size800,
                    fontVariations: [Fonts.weightMedium],
                    letterSpacing: -0.5,
                  ),
                ),
              ),
              const Expanded(
                child: Text(
                  " sats",
                  style: TextStyle(
                    fontSize: Fonts.size800,
                    color: LxColors.grey750,
                    // fontVariations: [Fonts.weightMedium],
                    letterSpacing: -0.5,
                  ),
                ),
              )
            ],
          ),

          const SizedBox(height: Space.s100),

          // Form validation error, if there is one. Usually I'd just use the
          // default .validator, but I can't get it to align properly.
          //
          // Wrap the text in a SizedBox so we don't reflow content below when
          // an error is shown.
          //
          // TODO(phlip9): animate this
          SizedBox(
            height: Space.s600,
            width: null,
            child: Align(
              alignment: Alignment.topCenter,
              child: ValueListenableBuilder(
                valueListenable: this.errorText,
                builder: (context, errorTextValue, child) =>
                    (errorTextValue != null)
                        ? Text(
                            errorTextValue,
                            style: Fonts.fontUI.copyWith(
                              color: LxColors.errorText,
                              fontSize: Fonts.size100,
                            ),
                          )
                        : const SizedBox(),
              ),
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
