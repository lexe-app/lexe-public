// Send payment page

import 'dart:async' show unawaited;
import 'dart:math' show max;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show MaxLengthEnforcement;

import 'package:lexeapp/components.dart'
    show
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        ScrollableSinglePageBody,
        ZigZag;

import '../../address_format.dart' as address_format;
import '../../bindings.dart' show api;
import '../../bindings_generated_api.dart'
    show AppHandle, ClientPaymentId, Network;
import '../../currency_format.dart' as currency_format;
import '../../input_formatter.dart'
    show
        AlphaNumericInputFormatter,
        IntInputFormatter,
        MaxUtf8BytesInputFormatter;
import '../../logger.dart' show info;
import '../../result.dart';
import '../../style.dart' show Fonts, LxColors, Space;

@immutable
final class SendContext {
  const SendContext({
    required this.app,
    required this.configNetwork,
    required this.balanceSats,
    required this.cid,
  });

  factory SendContext.cidFromRng({
    required AppHandle app,
    required Network configNetwork,
    required int balanceSats,
  }) =>
      SendContext(
        app: app,
        configNetwork: configNetwork,
        balanceSats: balanceSats,
        cid: api.genClientPaymentId(),
      );

  final AppHandle app;
  final Network configNetwork;
  final int balanceSats;
  final ClientPaymentId cid;
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
        style: const TextStyle(
          fontSize: Fonts.size600,
          fontVariations: [Fonts.weightMedium],
          letterSpacing: -0.5,
          height: 1.0,
        ),
      ),
    );
  }
}

// class SubheadingText extends StatelessWidget {
//   const SubheadingText({super.key});
//
//   final String text;
//
//   @override
//   Widget build(BuildContext context) {
//     Text(
//       this.text,
//       style: Fonts.fontUI.copyWith(
//         color: LxColors.grey600,
//         fontSize: Fonts.size300,
//       ),
//     );
//   }
// }

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

class NextButton extends LxFilledButton {
  const NextButton({super.key, required super.onTap})
      : super(
          label: const Text("Next"),
          icon: const Icon(Icons.arrow_forward_rounded),
        );
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
            // Bitcoin addresses are alphanumeric
            inputFormatters: [AlphaNumericInputFormatter()],
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

  final IntInputFormatter intInputFormatter = IntInputFormatter();

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
                  style: Fonts.fontUI.copyWith(color: LxColors.grey600),
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
  final GlobalKey<FormFieldState<String>> noteFieldKey = GlobalKey();

  bool isSending = false;

  void onSend() {
    this.setState(() {
      isSending = true;
    });

    unawaited(Future.delayed(const Duration(seconds: 2), () {
      this.setState(() {
        isSending = false;
      });
    }));
  }

  @override
  Widget build(BuildContext context) {
    final shortAddr = address_format.ellipsizeBtcAddress(this.widget.address);
    final amountSats = switch (this.widget.sendAmount) {
      SendAmountExact(:final amountSats) => amountSats,
      // TODO(phlip9): the exact amount will need to come from the
      // pre-validation + fee estimation request.
      SendAmountAll() => this.widget.sendCtx.balanceSats,
    };

    final amountSatsStr = currency_format.formatSatsAmount(amountSats);

    // TODO(phlip9): get est. fee from pre-validation + fee estimation request
    const feeSats = 1400;
    final feeSatsStr = currency_format.formatSatsAmount(feeSats);

    final totalSats = amountSats + feeSats;
    final totalSatsStr = currency_format.formatSatsAmount(totalSats);

    const textStylePrimary = TextStyle(
      fontSize: Fonts.size300,
      color: LxColors.foreground,
      fontVariations: [Fonts.weightMedium],
    );

    const textStyleSecondary = TextStyle(
      fontSize: Fonts.size300,
      color: LxColors.grey550,
      fontVariations: [],
    );

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
          const HeadingText(text: "Confirm payment"),
          Text(
            "Sending bitcoin on-chain",
            style: Fonts.fontUI.copyWith(
              color: LxColors.grey600,
              fontSize: Fonts.size300,
            ),
          ),
          const SizedBox(height: Space.s700),
          // To
          Row(
            mainAxisSize: MainAxisSize.max,
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Text("To", style: textStyleSecondary),
              Text(
                shortAddr,
                style: textStylePrimary
                    .copyWith(fontFeatures: [Fonts.featDisambugation]),
              ),
              // TODO(phlip9): button to expand address for full verification
              // and copy-to-clipboard
            ],
          ),

          const SizedBox(height: Space.s500),

          Row(
            mainAxisSize: MainAxisSize.max,
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Text("Amount", style: textStyleSecondary),
              Text(amountSatsStr, style: textStyleSecondary),
            ],
          ),

          const SizedBox(height: Space.s100),

          Row(
            mainAxisSize: MainAxisSize.max,
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Text("Network Fee", style: textStyleSecondary),
              Text(feeSatsStr, style: textStyleSecondary),
            ],
          ),

          const SizedBox(
            height: Space.s650,
            child: ZigZag(
                color: LxColors.grey750, zigWidth: 14.0, strokeWidth: 1.0),
          ),

          Row(
            mainAxisSize: MainAxisSize.max,
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Text("Total", style: textStyleSecondary),
              Text(totalSatsStr, style: textStylePrimary),
            ],
          ),

          const SizedBox(height: Space.s700),

          TextFormField(
            key: this.noteFieldKey,

            // Disable the input field while the send request is pending.
            enabled: !this.isSending,

            autofocus: false,
            keyboardType: TextInputType.text,
            textInputAction: TextInputAction.send,
            onEditingComplete: this.onSend,
            maxLines: null,
            maxLength: 200,
            maxLengthEnforcement: MaxLengthEnforcement.enforced,

            // Silently limit input to 512 bytes. This could be a little
            // confusing if the user inputs a ton of emojis or CJK characters
            // I guess.
            inputFormatters: const [MaxUtf8BytesInputFormatter(maxBytes: 512)],

            decoration: const InputDecoration(
              hintStyle: TextStyle(color: LxColors.grey550),
              hintText: "What's this payment for? (optional)",
              counterStyle: TextStyle(color: LxColors.grey550),
              border: OutlineInputBorder(),
              enabledBorder: OutlineInputBorder(
                  borderSide: BorderSide(color: LxColors.fgTertiary)),
              focusedBorder: OutlineInputBorder(
                  borderSide: BorderSide(color: LxColors.foreground)),
            ),
            style: Fonts.fontBody.copyWith(
              fontSize: Fonts.size200,
              height: 1.5,
              color: LxColors.fgSecondary,
              letterSpacing: -0.15,
            ),
          ),
        ],
        bottom: Column(
          mainAxisSize: MainAxisSize.min,
          mainAxisAlignment: MainAxisAlignment.end,
          verticalDirection: VerticalDirection.down,
          children: [
            const Expanded(child: SizedBox(height: Space.s500)),
            // Disable the button and show a loading indicator while sending the
            // request.
            SendButton(onTap: this.onSend, loading: this.isSending),
          ],
        ),
      ),
    );
  }
}

/// The "Send" button at the bottom of the "Confirm Payment" page.
///
/// It animates into a shortened button with a loading indicator inside when
/// we're sending the payment request and awaiting the response.
class SendButton extends StatefulWidget {
  const SendButton({super.key, required this.onTap, required this.loading});

  final VoidCallback? onTap;
  final bool loading;

  bool get enabled => this.onTap != null;

  @override
  State<SendButton> createState() => _SendButtonState();
}

class _SendButtonState extends State<SendButton> {
  // @override
  // void didUpdateWidget(SendButton old) {
  //   super.didUpdateWidget(old);
  //   if (this.widget.enabled != old.enabled)
  // }

  @override
  Widget build(BuildContext context) {
    final loading = this.widget.loading;

    // When we're loading, we:
    // (1) shorten the button width
    // (2) replace the button label with a loading indicator
    // (3) hide the button icon

    return AnimatedContainer(
      duration: const Duration(milliseconds: 200),
      curve: Curves.decelerate,
      // We need to set a maximum width, since we can't interpolate between an
      // unbounded width and a finite width.
      width: (!loading) ? 450.0 : Space.s900,
      child: LxFilledButton(
        // Also disable the button while loading.
        onTap: (!loading) ? this.widget.onTap : null,
        label: AnimatedSwitcher(
          duration: const Duration(milliseconds: 150),
          child: (!loading)
              ? const Text("Send")
              : const Center(
                  child: SizedBox.square(
                    dimension: Fonts.size300,
                    child: CircularProgressIndicator(
                      strokeWidth: 2.0,
                      color: LxColors.clearB200,
                    ),
                  ),
                ),
        ),
        icon: AnimatedOpacity(
          opacity: (!loading) ? 1.0 : 0.0,
          duration: const Duration(milliseconds: 150),
          child: const Icon(Icons.arrow_forward_rounded),
        ),
        style: FilledButton.styleFrom(
          backgroundColor: LxColors.moneyGoUp,
          foregroundColor: LxColors.grey1000,
        ),
      ),
    );
  }
}
