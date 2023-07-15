// Send payment page

import 'dart:math' show max;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show FilteringTextInputFormatter;

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

/// The text that sits directly beneath the AppBar.
class HeadingText extends StatelessWidget {
  const HeadingText({
    super.key,
    required this.text,
  });

  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: Space.s500, bottom: Space.s200),
      child: Text(
        this.text,
        style: Fonts.fontUI.copyWith(
          fontSize: Fonts.size600,
          fontVariations: [Fonts.weightMedium],
          letterSpacing: -0.5,
        ),
      ),
    );
  }
}

const InputDecoration baseInputDecoration = InputDecoration(
  hintStyle: TextStyle(color: LxColors.grey750),
  filled: true,
  fillColor: LxColors.clearB0,
  // hoverColor: LxColors.clearB50,
  // Remove left and right padding so we have more room for
  // amount text.
  contentPadding: EdgeInsets.symmetric(vertical: Space.s300),
  // errorBorder: InputBorder.none,
  focusedBorder: InputBorder.none,
  // focusedErrorBorder: InputBorder.none,
  disabledBorder: InputBorder.none,
  enabledBorder: InputBorder.none,
);

class NextButton extends StatelessWidget {
  const NextButton({super.key, required this.onTap});

  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return FilledButton(
      onPressed: this.onTap,
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
  State<StatefulWidget> createState() => _SendPaymentAddressPageState();
}

class _SendPaymentAddressPageState extends State<SendPaymentAddressPage> {
  final GlobalKey<FormFieldState<String>> addressFieldKey = GlobalKey();

  void onQrPressed() {
    info("pressed QR button");
  }

  void onNext() {
    final fieldState = this.addressFieldKey.currentState!;
    if (!fieldState.validate()) {
      return;
    }

    final String address;

    switch (this.validateBitcoinAddress(fieldState.value!)) {
      case Ok(:final ok):
        address = ok;
      case Err():
        return;
    }

    Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_) => SendPaymentAmountPage(
        sendCtx: this.widget.sendCtx,
        address: address,
      ),
    ));
  }

  /// Ensure the bitcoin address is properly formatted and targets the right
  /// bitcoin network (mainnet, testnet, regtest) for our build.
  Result<String, String?> validateBitcoinAddress(String? addressStr) {
    // Don't show any error message if the input is empty.
    if (addressStr == null || addressStr.isEmpty) {
      return const Err(null);
    }

    // Actually try to parse as a bitcoin address.
    // TODO(phlip9): this API should return a bare enum and flutter should
    // handle converting that to a human-readable error message.
    final maybeErrMsg = api.formValidateBitcoinAddress(
      currentNetwork: this.widget.sendCtx.configNetwork,
      addressStr: addressStr,
    );

    if (maybeErrMsg == null) {
      return Ok(addressStr);
    } else {
      return Err(maybeErrMsg);
    }
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
          const HeadingText(text: "Who are we paying?"),
          const SizedBox(height: Space.s300),
          TextFormField(
            key: this.addressFieldKey,
            autofocus: true,
            // `visiblePassword` gives ready access to letters + numbers
            keyboardType: TextInputType.visiblePassword,
            textDirection: TextDirection.ltr,
            textInputAction: TextInputAction.next,
            validator: (str) => this.validateBitcoinAddress(str).err,
            onEditingComplete: this.onNext,
            inputFormatters: [
              // Bitcoin addresses are alphanumeric
              FilteringTextInputFormatter.allow(RegExp(r'[a-zA-Z0-9]')),
            ],
            decoration:
                baseInputDecoration.copyWith(hintText: "Bitcoin address"),
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
        bottom: NextButton(onTap: this.onNext),
      ),
    );
  }
}

// If only we had real enums... sad.

sealed class SendAmount {
  const SendAmount();
}

final class SendAmountAll extends SendAmount {
  const SendAmountAll();
}

final class SendAmountExact extends SendAmount {
  const SendAmountExact(this.amountSats);
  final int amountSats;

  @override
  String toString() => "SendAmountExact(${this.amountSats})";
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

  final ValueNotifier<bool> sendFullBalanceEnabled = ValueNotifier(false);

  @override
  void dispose() {
    sendFullBalanceEnabled.dispose();

    super.dispose();
  }

  void onNext() {
    final SendAmount sendAmount;

    if (sendFullBalanceEnabled.value) {
      sendAmount = const SendAmountAll();
    } else {
      final fieldState = this.amountFieldKey.currentState!;
      if (!fieldState.validate()) {
        return;
      }

      final result = this.validateAmountStr(fieldState.value).ok;
      if (result == null) {
        return;
      }
      final int amountSats = result;

      sendAmount = SendAmountExact(amountSats);
    }

    Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_) => SendPaymentConfirmPage(
        sendCtx: this.widget.sendCtx,
        address: this.widget.address,
        sendAmount: sendAmount,
      ),
    ));
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
          const HeadingText(text: "How much?"),

          Text(
            "balance $balanceStr",
            textAlign: TextAlign.left,
            style: Fonts.fontUI.copyWith(
              color: LxColors.grey600,
              fontSize: Fonts.size300,
            ),
          ),
          const SizedBox(height: Space.s850),

          // <amount> sats
          TextFormField(
            key: this.amountFieldKey,
            autofocus: true,
            keyboardType: const TextInputType.numberWithOptions(
                signed: false, decimal: false),
            initialValue: "0",
            textDirection: TextDirection.ltr,
            textInputAction: TextInputAction.next,
            textAlign: TextAlign.right,
            onEditingComplete: this.onNext,
            validator: (str) => this.validateAmountStr(str).err,
            decoration: baseInputDecoration.copyWith(
              hintText: "0",
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
        bottom: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            // Send full balance
            ValueListenableBuilder(
              valueListenable: this.sendFullBalanceEnabled,
              builder: (context, isEnabled, _) => SwitchListTile(
                value: isEnabled,
                // TODO(phlip9): When a user selects "Send full balance", also
                // 1. deemphasize / grey out out the amount field
                // 2. set the value to the expected amount we'll send incl. fees
                // 3. if the user starts typing in the amount field again, unset
                //    the "send full balance" widget
                onChanged: (newValue) =>
                    this.sendFullBalanceEnabled.value = newValue,
                title: Text(
                  "Send full balance",
                  textAlign: TextAlign.end,
                  style: Fonts.fontUI.copyWith(color: LxColors.fgTertiary),
                ),
                contentPadding:
                    const EdgeInsets.symmetric(horizontal: Space.s550),
                inactiveTrackColor: LxColors.grey1000,
                activeTrackColor: LxColors.moneyGoUp,
                inactiveThumbColor: LxColors.background,
                controlAffinity: ListTileControlAffinity.trailing,
              ),
            ),
            const SizedBox(height: Space.s500),

            // Next ->
            NextButton(onTap: this.onNext),
          ],
        ),
      ),
    );
  }
}

class SendPaymentConfirmPage extends StatefulWidget {
  const SendPaymentConfirmPage({
    super.key,
    required this.sendCtx,
    required this.address,
    required this.sendAmount,
  });

  final SendContext sendCtx;
  final String address;
  final SendAmount sendAmount;

  @override
  State<SendPaymentConfirmPage> createState() => _SendPaymentConfirmPageState();
}

class _SendPaymentConfirmPageState extends State<SendPaymentConfirmPage> {
  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leading: const LxBackButton(),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.s100),
        ],
      ),
      body: const ScrollableSinglePageBody(
        body: [
          HeadingText(text: "Confirm payment"),
        ],
      ),
    );
  }
}
