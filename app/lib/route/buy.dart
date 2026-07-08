/// Buy page: prompts the user for an amount, then calls the SDK's
/// `buy_with_cash_app` and opens the returned Cash App URL to fund the buy.
library;

import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/material.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:lexeapp/components.dart'
    show
        LxBackButton,
        LxFilledButton,
        PaymentAmountInput,
        ScrollableSinglePageBody,
        showModalAsyncFlow;
import 'package:lexeapp/input_formatter.dart' show IntInputFormatter;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/style.dart' show LxIcons, Space;
import 'package:lexeapp/url.dart' as url;

class BuyPage extends StatefulWidget {
  const BuyPage({super.key, required this.app});

  final AppHandle app;

  @override
  State<BuyPage> createState() => _BuyPageState();
}

class _BuyPageState extends State<BuyPage> {
  /// The Rust SDK enforces MINIMUM_BUY_SATS = 5000;
  /// mirror it here for inline validation.
  static const int minimumBuySats = 5000;

  final GlobalKey<FormFieldState<String>> amountFieldKey = GlobalKey();
  final IntInputFormatter intInputFormatter = IntInputFormatter();

  Result<(), String> validateAmount(int amountSats) =>
      amountSats >= minimumBuySats
      ? const Ok(())
      : const Err("Enter at least ₿5000");

  Future<void> onConfirm() async {
    final amountState = this.amountFieldKey.currentState!;
    if (!amountState.validate()) return;

    // `allowEmpty/Zero: false` + [validateAmount] guarantee `amountSats >= minSats`.
    final amountSats = this.intInputFormatter.tryParse(amountState.value!).ok!;

    info("BuyPage: Buying $amountSats sats with Cash App");

    final result = await showModalAsyncFlow(
      context: this.context,
      future: Result.tryFfiAsync(
        () => this.widget.app.buyWithCashApp(amountSats: amountSats),
      ),
      errorBuilder: (context, err) => AlertDialog(
        title: const Text("Failed to start Cash App buy"),
        content: Text(err.message),
        scrollable: true,
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text("Close"),
          ),
        ],
      ),
    );

    if (!this.mounted || result == null) return;
    if (result case Ok(:final ok)) {
      info("BuyPage: opening Cash App");
      unawaited(url.open(ok));
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        body: [
          const SizedBox(height: Space.s300),
          MarkdownBody(
            data: '''
# Enter an amount to buy

*You'll be redirected to Cash App to complete your Bitcoin purchase.*
*Available in the US only.*

Cash App buys are instant!
Your Bitcoin will land directly into Lexe Wallet. ⚡️

Make sure to set up Cash App on this device before you proceed.
''',
          ),
          const SizedBox(height: Space.s700),

          PaymentAmountInput(
            fieldKey: this.amountFieldKey,
            intInputFormatter: this.intInputFormatter,
            allowEmpty: false,
            allowZero: false,
            validate: this.validateAmount,
            onEditingComplete: this.onConfirm,
          ),
        ],
        bottom: LxFilledButton.tonal(
          label: const Text("To Cash App"),
          icon: const Icon(LxIcons.next),
          onTap: this.onConfirm,
        ),
      ),
    );
  }
}
