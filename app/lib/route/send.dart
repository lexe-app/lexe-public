// Send payment page

import 'dart:math' show max;

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
import '../../logger.dart' show dbg, info;
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

  final currency_format.IntInputFormatter intInputFormatter =
      currency_format.IntInputFormatter();

  void onNext() {
    final fieldState = this.amountFieldKey.currentState!;
    if (!fieldState.validate()) {
      return;
    }

    final result = this.validateAmountStr(fieldState.value).ok;
    if (result == null) {
      return;
    }
    final int amountSats = result;

    info("amountSats = $amountSats");
  }

  Result<int, String?> validateAmountStr(String? maybeAmountStr) {
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
          TextFormField(
            key: this.amountFieldKey,
            autofocus: true,
            keyboardType: const TextInputType.numberWithOptions(
              signed: false,
              decimal: false,
            ),
            textDirection: TextDirection.ltr,
            textInputAction: TextInputAction.next,
            textAlign: TextAlign.right,
            // textAlign: TextAlign.center,
            // textAlignVertical: TextAlignVertical.top,
            onEditingComplete: this.onNext,
            validator: (str) => this.validateAmountStr(str).err,
            decoration: InputDecoration(
              hintText: "0",
              hintStyle: const TextStyle(
                color: LxColors.grey750,
              ),
              filled: true,
              fillColor: LxColors.clearB0,
              hoverColor: LxColors.clearB50,
              // Remove left and right padding so we have more room for
              // amount text.
              contentPadding:
                  const EdgeInsets.only(top: Space.s300, bottom: Space.s300),
              // errorBorder: InputBorder.none,
              focusedBorder: InputBorder.none,
              // focusedErrorBorder: InputBorder.none,
              disabledBorder: InputBorder.none,
              enabledBorder: InputBorder.none,
              // Goal: I want the amount to be right-aligned, starting from the
              //       center of the screen.
              //
              // |    vvvvvvv            |
              // |    123,456| sats      |
              // |                       |
              //
              // There's probably a better way to do this, but this works. Just
              // expand the " sats" suffix so that it's
              suffix: LayoutBuilder(
                // builder: (context, constraints) =>
                builder: (context, constraints) => ConstrainedBox(
                  constraints: BoxConstraints(
                    minWidth: max(0.0, (constraints.maxWidth / 2) - Space.s200),
                  ),
                  child: const Text(" sats"),
                ),
              ),
            ),
            inputFormatters: [this.intInputFormatter],
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size800,
              fontVariations: [Fonts.weightMedium],
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
