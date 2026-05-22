/// Buy page: prompts the user for an amount, mints a Lightning invoice for
/// that amount, and opens the Cash App `lightning:` deep link to fund it.
library;

import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/api.dart' show CreateInvoiceRequest;
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
  /// Cash App's deep link will accept amounts lower than this, but we recommend
  /// a minimum of 5k sats for a good user experience.
  static const int minSats = 5000;

  final GlobalKey<FormFieldState<String>> amountFieldKey = GlobalKey();
  final IntInputFormatter intInputFormatter = IntInputFormatter();

  Result<(), String> validateAmount(int amountSats) =>
      amountSats >= minSats ? const Ok(()) : const Err("Enter at least ₿5000");

  Future<void> onConfirm() async {
    final amountState = this.amountFieldKey.currentState!;
    if (!amountState.validate()) return;

    // `allowEmpty/Zero: false` + [validateAmount] guarantee `amountSats >= minSats`.
    final amountSats = this.intInputFormatter.tryParse(amountState.value!).ok!;

    info("BuyPage: minting invoice for $amountSats sats");

    final req = CreateInvoiceRequest(
      expirySecs: 24 * 60 * 60,
      amountSats: amountSats,
      // NOTE: keep "Cash App Buy" in sync with the `is_junk` check in
      // `public/lexe-api-core/src/types/payments.rs`, which uses this exact
      // string to hide unpaid Buy invoices from the payments list.
      description: "Cash App Buy",
    );

    final result = await showModalAsyncFlow(
      context: this.context,
      future: Result.tryFfiAsync(() => this.widget.app.createInvoice(req: req)),
      errorBuilder: (context, err) => AlertDialog(
        title: const Text("Failed to create invoice"),
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
      final bolt11 = ok.invoice.string;
      info("BuyPage: opening Cash App deep link");
      unawaited(url.open("https://cash.app/launch/lightning/$bolt11"));
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

Cash App charges **zero** fees!
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
