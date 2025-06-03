// Send payment page

// ignore_for_file: camel_case_types

import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/api.dart'
    show FeeEstimate, PreflightPayOnchainResponse;
import 'package:app_rs_dart/ffi/api.ext.dart';
import 'package:app_rs_dart/ffi/types.dart'
    show ConfirmationPriority, PaymentKind;
import 'package:app_rs_dart/ffi/types.ext.dart';
import 'package:flutter/material.dart';
import 'package:lexeapp/address_format.dart' as address_format;
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        MultistepFlow,
        PaymentAmountInput,
        PaymentNoteInput,
        ReceiptSeparator,
        ScrollableSinglePageBody,
        SubheadingText,
        baseInputDecoration;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/input_formatter.dart' show IntInputFormatter;
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/scan.dart' show ScanPage;
import 'package:lexeapp/route/send/state.dart'
    show
        PreflightedPayment_Invoice,
        PreflightedPayment_Offer,
        PreflightedPayment_Onchain,
        SendFlowResult,
        SendState,
        SendState_NeedAmount,
        SendState_NeedUri,
        SendState_Preflighted;
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;

/// The entry point for the send payment flow. This will dispatch to the right
/// initial screen depending on the [SendState]. If [startNewFlow], then it
/// also sets up a new / [MultistepFlow] so navigation "close" will exit out of
/// the whole flow.
class SendPaymentPage extends StatelessWidget {
  const SendPaymentPage({
    super.key,
    required this.sendCtx,
    required this.startNewFlow,
  });

  final SendState sendCtx;
  final bool startNewFlow;

  Widget buildInnerSendPage() {
    final sendCtx = this.sendCtx;
    return switch (sendCtx) {
      SendState_Preflighted() => SendPaymentConfirmPage(sendCtx: sendCtx),
      SendState_NeedAmount() => SendPaymentAmountPage(sendCtx: sendCtx),
      SendState_NeedUri() => SendPaymentNeedUriPage(sendCtx: sendCtx),
    };
  }

  @override
  Widget build(BuildContext context) => (this.startNewFlow)
      ? MultistepFlow<SendFlowResult>(builder: (_) => this.buildInnerSendPage())
      : this.buildInnerSendPage();
}

/// If the user is just hitting the "Send" button with no extra context, then we
/// need to collect a [PaymentUri] of some kind (bitcoin address, LN invoice,
/// etc...)
class SendPaymentNeedUriPage extends StatefulWidget {
  const SendPaymentNeedUriPage({
    super.key,
    required this.sendCtx,
  });

  final SendState_NeedUri sendCtx;

  @override
  State<StatefulWidget> createState() => _SendPaymentNeedUriPageState();
}

class _SendPaymentNeedUriPageState extends State<SendPaymentNeedUriPage> {
  final GlobalKey<FormFieldState<String>> paymentUriFieldKey = GlobalKey();

  final ValueNotifier<bool> isPending = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.errorMessage.dispose();
    this.isPending.dispose();

    super.dispose();
  }

  Future<void> onScanPressed() async {
    info("pressed QR scan button");

    final SendFlowResult? flowResult =
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_context) => ScanPage(sendCtx: this.widget.sendCtx),
    ));
    if (!this.mounted || flowResult == null) return;

    // Successfully sent payment -- return result to parent page.
    // ignore: use_build_context_synchronously
    await Navigator.of(this.context).maybePop(flowResult);
  }

  Future<void> onNext() async {
    // Hide error message
    this.errorMessage.value = null;

    // Validate the payment URI field.
    final fieldState = this.paymentUriFieldKey.currentState!;
    if (!fieldState.validate()) return;

    final uriStr = fieldState.value;

    // Don't bother showing an error if the input is empty.
    if (uriStr == null || uriStr.isEmpty) return;

    // Start loading animation
    this.isPending.value = true;

    // Try resolving the payment URI to a "best" payment method. Then try
    // immediately preflighting it if it already has an associated amount.
    final result = await this.widget.sendCtx.resolveAndMaybePreflight(uriStr);
    if (!this.mounted) return;

    // Stop loading animation
    this.isPending.value = false;

    // Check the results, or show an error on the page.
    final SendState sendCtx;
    switch (result) {
      case Ok(:final ok):
        sendCtx = ok;
      case Err(:final err):
        this.errorMessage.value = ErrorMessage(message: err);
        return;
    }

    // If we still need an amount, then we have to collect that first.
    // Otherwise, a successful payment preflight means we can go directly to the
    // confirm page.
    final SendFlowResult? flowResult =
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_) => SendPaymentPage(sendCtx: sendCtx, startNewFlow: false),
    ));

    info("SendPaymentNeedUriPage: flowResult: $flowResult, mounted: $mounted");
    if (!this.mounted || flowResult == null) return;

    // Successfully sent payment -- return result to parent page.
    // ignore: use_build_context_synchronously
    await Navigator.of(this.context).maybePop(flowResult);
  }

  /// Called when the user taps the paste button
  Future<void> onPaste() async {
    // Get clipboard text
    final text = await LxClipboard.getText();
    if (!this.mounted) return;
    if (text == null || text.isEmpty) return;

    // Set payment URI field
    this.paymentUriFieldKey.currentState?.didChange(text);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(
            isLeading: true, kind: LxCloseButtonKind.closeFromRoot),
        actions: [
          IconButton(
            onPressed: this.onScanPressed,
            icon: const Icon(LxIcons.scanDetailed),
          ),
          const SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Who are we paying?"),
          const SizedBox(height: Space.s300),

          // Enter payment URI text field
          TextFormField(
            key: this.paymentUriFieldKey,
            autofocus: true,
            // `visiblePassword` gives ready access to letters + numbers
            keyboardType: TextInputType.visiblePassword,
            textDirection: TextDirection.ltr,
            textInputAction: TextInputAction.next,
            onEditingComplete: this.onNext,
            decoration: baseInputDecoration.copyWith(
                hintText: "bc1.. lnbc1.. bitcoin:.."),
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

          // Error parsing, resolving, and/or preflighting payment
          ValueListenableBuilder(
            valueListenable: this.errorMessage,
            builder: (_context, errorMessage, _widget) =>
                ErrorMessageSection(errorMessage),
          ),
        ],

        // Bottom buttons (paste, next ->)
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              Row(
                children: [
                  // Paste
                  Expanded(
                    child: GestureDetector(
                      onTap: this.onPaste,
                      child: StackedButton(
                        button: LxFilledButton(
                          onTap: this.onPaste,
                          icon: const Center(child: Icon(LxIcons.paste)),
                        ),
                        label: "Paste",
                      ),
                    ),
                  ),
                  const SizedBox(width: Space.s200),
                  // Next ->
                  Expanded(
                    child: ValueListenableBuilder(
                      valueListenable: this.isPending,
                      builder: (_context, isPending, _widget) =>
                          GestureDetector(
                        onTap: !isPending ? this.onNext : null,
                        child: StackedButton(
                          button: AnimatedFillButton(
                            label: const Icon(LxIcons.next),
                            icon: const Icon(null),
                            onTap: this.onNext,
                            loading: isPending,
                          ),
                          label: "Next",
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class StackedButton extends StatelessWidget {
  const StackedButton({super.key, required this.button, required this.label});

  final Widget button;
  final String label;

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        button,
        const SizedBox(height: Space.s400),
        Text(
          this.label,
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size300,
            color: LxColors.foreground,
            fontVariations: [Fonts.weightSemiBold],
          ),
        ),
      ],
    );
  }
}

/// Send payment flow: this page collects the [SendAmount] from the user.
class SendPaymentAmountPage extends StatefulWidget {
  const SendPaymentAmountPage({
    super.key,
    required this.sendCtx,
  });

  final SendState_NeedAmount sendCtx;

  @override
  State<SendPaymentAmountPage> createState() => _SendPaymentAmountPageState();
}

class _SendPaymentAmountPageState extends State<SendPaymentAmountPage> {
  final GlobalKey<FormFieldState<String>> amountFieldKey = GlobalKey();

  final IntInputFormatter intInputFormatter = IntInputFormatter();

  final ValueNotifier<ErrorMessage?> estimateFeeError = ValueNotifier(null);
  final ValueNotifier<bool> estimatingFee = ValueNotifier(false);

  @override
  void dispose() {
    this.estimatingFee.dispose();
    this.estimateFeeError.dispose();

    super.dispose();
  }

  Future<void> onNext() async {
    // Hide error message.
    this.estimateFeeError.value = null;

    // Validate the amount field.
    final fieldState = this.amountFieldKey.currentState!;
    if (!fieldState.validate()) return;

    final value = fieldState.value;
    if (value == null || value.isEmpty) return;

    final int amountSats;
    switch (this.intInputFormatter.tryParse(value)) {
      case Err():
        return;
      case Ok(:final ok):
        amountSats = ok;
    }

    // Only start the loading animation once the initial amount validation is
    // done.
    this.estimatingFee.value = true;

    // Preflight the payment. That means we're checking, on the node itself,
    // for enough balance, if there's a route, fees, etc...
    final result = await this.widget.sendCtx.preflight(amountSats);

    if (!this.mounted) return;

    // Reset loading animation.
    this.estimatingFee.value = false;

    // Check if preflight was successful, or show an error message.
    final SendState_Preflighted nextSendCtx;
    switch (result) {
      case Ok(:final ok):
        nextSendCtx = ok;
        this.estimateFeeError.value = null;
      case Err(:final err):
        error("Error preflighting payment: $err");
        this.estimateFeeError.value = ErrorMessage(
          title: "Error preflighting payment",
          message: err.message,
        );
        return;
    }

    // Everything looks good so far -- navigate to the confirmation page.
    final SendFlowResult? flowResult =
        // ignore: use_build_context_synchronously
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_) => SendPaymentConfirmPage(sendCtx: nextSendCtx),
    ));

    // Confirm page results:
    info("SendPaymentAmountPage: flowResult: $flowResult, mounted: $mounted");

    if (!this.mounted || flowResult == null) return;

    // ignore: use_build_context_synchronously
    await Navigator.of(this.context).maybePop(flowResult);
  }

  Result<(), String> validateAmount(int amount) {
    final kind = this.widget.sendCtx.paymentMethod.kind();
    final balance = this.widget.sendCtx.balance;
    final balanceMaxSendableSats = balance.maxSendableByKind(kind);
    if (amount > balanceMaxSendableSats) {
      final balanceMaxSendableStr = currency_format.formatSatsAmount(
        balanceMaxSendableSats,
        satsSuffix: true,
      );
      return Err("Can't send more than $balanceMaxSendableStr");
    }

    return const Ok(());
  }

  @override
  Widget build(BuildContext context) {
    final kind = this.widget.sendCtx.paymentMethod.kind();
    final balance = this.widget.sendCtx.balance;
    final balanceMaxSendableStr = currency_format.formatSatsAmount(
      balance.maxSendableByKind(kind),
      satsSuffix: true,
    );

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "How much?"),
          SubheadingText(text: "Send up to $balanceMaxSendableStr"),
          const SizedBox(height: Space.s850),

          // <amount> sats
          PaymentAmountInput(
            fieldKey: this.amountFieldKey,
            intInputFormatter: this.intInputFormatter,
            onEditingComplete: this.onNext,
            validate: this.validateAmount,
            allowEmpty: false,
          ),

          const SizedBox(height: Space.s700),

          // Error fetching fee estimate
          ValueListenableBuilder(
            valueListenable: this.estimateFeeError,
            builder: (_context, errorMessage, _widget) =>
                ErrorMessageSection(errorMessage),
          ),
        ],

        // Next ->
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.estimatingFee,
            builder: (_context, estimatingFee, _widget) => AnimatedFillButton(
              label: const Text("Next"),
              icon: const Icon(LxIcons.next),
              onTap: this.onNext,
              loading: estimatingFee,
            ),
          ),
        ),
      ),
    );
  }
}

/// Send payment flow: this page shows the full payment details and asks the
/// user to confirm before finally sending.
///
/// The page also:
///
/// 1. Estimates the BTC network fee for the tx at the given tx priority.
/// 2. Collects an optional payment note for the user's record keeping.
/// 3. Allows the user to adjust the tx priority for high+fast or low+slow
///    fee/confirmation time.
class SendPaymentConfirmPage extends StatefulWidget {
  const SendPaymentConfirmPage({
    super.key,
    required this.sendCtx,
  });

  final SendState_Preflighted sendCtx;

  @override
  State<SendPaymentConfirmPage> createState() => _SendPaymentConfirmPageState();
}

class _SendPaymentConfirmPageState extends State<SendPaymentConfirmPage> {
  final GlobalKey<FormFieldState<String>> noteFieldKey = GlobalKey();

  final ValueNotifier<ErrorMessage?> sendError = ValueNotifier(null);
  final ValueNotifier<bool> isSending = ValueNotifier(false);

  // TODO(phlip9): save/load this from/to user preferences?
  final ValueNotifier<ConfirmationPriority> confPriority =
      ValueNotifier(ConfirmationPriority.normal);

  @override
  void dispose() {
    this.confPriority.dispose();
    this.isSending.dispose();
    this.sendError.dispose();
    super.dispose();
  }

  Future<void> onConfirm() async {
    if (this.isSending.value) return;

    // We're sending; clear the errors and disable the form inputs.
    this.isSending.value = true;
    this.sendError.value = null;

    // Actually start the payment
    final FfiResult<SendFlowResult> result =
        await this.widget.sendCtx.pay(this.note(), this.confPriority.value);

    if (!this.mounted) return;

    switch (result) {
      case Ok(:final ok):
        // The request succeeded and we're still mounted (the user hasn't
        // navigated away somehow). Let's pop ourselves off the nav stack and
        // notify our caller that we were successful.
        final flowResult = ok;
        info("SendPaymentConfirmPage: success: flowResult: $flowResult");
        // ignore: use_build_context_synchronously
        unawaited(Navigator.of(this.context).maybePop(flowResult));

      case Err(:final err):
        // The request failed. Set the error message and unset loading.
        error("SendPaymentConfirmPage: error sending on-chain payment: $err");
        this.isSending.value = false;
        this.sendError.value = ErrorMessage(
          title: "Error sending payment",
          message: err.message,
        );
    }
  }

  Future<void> chooseOnchainFeeRate(
      final PreflightedPayment_Onchain preflighted) async {
    final ConfirmationPriority? result = await showDialog(
      context: this.context,
      useRootNavigator: false,
      builder: (context) => ChooseOnchainFeeDialog(
        feeEstimates: preflighted.preflight,
        selected: this.confPriority.value,
      ),
    );

    if (!this.mounted) return;

    if (result != null) {
      this.confPriority.value = result;
    }
  }

  int amountSats() => switch (this.widget.sendCtx.preflightedPayment) {
        PreflightedPayment_Invoice(:final preflight) => preflight.amountSats,
        PreflightedPayment_Onchain(:final amountSats) => amountSats,
        PreflightedPayment_Offer() =>
          throw UnimplementedError("BOLT12 offers are unsupported"),
      };

  int feeSats() => switch (this.widget.sendCtx.preflightedPayment) {
        PreflightedPayment_Onchain(:final preflight) => switch (
              this.confPriority.value) {
            // invariant: High can not be selected if there are insufficient funds
            ConfirmationPriority.high => preflight.high!.amountSats,
            ConfirmationPriority.normal => preflight.normal.amountSats,
            ConfirmationPriority.background => preflight.background.amountSats,
          },
        PreflightedPayment_Invoice(:final preflight) => preflight.feesSats,
        PreflightedPayment_Offer() =>
          throw UnimplementedError("BOLT12 offers are unsupported"),
      };

  int totalSats() => this.amountSats() + this.feeSats();

  String payee() => switch (this.widget.sendCtx.preflightedPayment) {
        PreflightedPayment_Invoice(:final invoice) => invoice.payeePubkey,
        PreflightedPayment_Onchain(:final onchain) => onchain.address,
        PreflightedPayment_Offer() =>
          throw UnimplementedError("BOLT12 offers are unsupported"),
      };

  String? note() => this.noteFieldKey.currentState?.value;

  @override
  Widget build(BuildContext context) {
    final preflighted = this.widget.sendCtx.preflightedPayment;

    final shortPayee = address_format.ellipsizeBtcAddress(this.payee());

    final amountSatsStr = currency_format.formatSatsAmount(this.amountSats());

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

    final paymentKind = this.widget.sendCtx.preflightedPayment.kind();
    final subheading = switch (paymentKind) {
      PaymentKind.onchain => "Sending bitcoin on-chain",
      PaymentKind.invoice => "Sending bitcoin via lightning invoice",
      PaymentKind.spontaneous =>
        "Sending bitcoin via lightning spontaneous payment",
      PaymentKind.offer => "Sending bitcoin via lightning offer",
    };

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Confirm payment"),
          SubheadingText(text: subheading),
          const SizedBox(height: Space.s700),

          Row(
            mainAxisSize: MainAxisSize.max,
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Text("To", style: textStyleSecondary),
              Text(
                shortPayee,
                style: textStylePrimary
                    .copyWith(fontFeatures: [Fonts.featDisambugation]),
              ),
              // TODO(phlip9): button to expand address for full verification
              // and copy-to-clipboard
              // TODO(phlip9): link to block explorer or node pubkey info
            ],
          ),

          const SizedBox(height: Space.s500),

          //
          // Amount to-be-received by the payee
          //

          Row(
            mainAxisSize: MainAxisSize.max,
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Text("Amount", style: textStyleSecondary),
              Text(amountSatsStr, style: textStyleSecondary),
            ],
          ),

          const SizedBox(height: Space.s100),

          //
          // Network Fee
          //

          if (preflighted case PreflightedPayment_Onchain())
            Row(
              mainAxisSize: MainAxisSize.max,
              mainAxisAlignment: MainAxisAlignment.start,
              children: [
                TextButton(
                  onPressed: () async => this.chooseOnchainFeeRate(preflighted),
                  style: TextButton.styleFrom(
                    textStyle: textStyleSecondary,
                    foregroundColor: LxColors.grey550,
                    shape: const LinearBorder(),
                    padding: const EdgeInsets.only(right: Space.s200),
                  ),
                  // Sadly flutter doesn't allow us to increase the space b/w the
                  // text and the underline. The default text decoration looks
                  // ugly af. So we have this hack to draw a dashed line...
                  child: const Row(
                    mainAxisSize: MainAxisSize.min,
                    mainAxisAlignment: MainAxisAlignment.start,
                    children: [
                      Text("Network Fee"),
                      SizedBox(width: Space.s200),
                      Icon(
                        LxIcons.edit,
                        size: Fonts.size300,
                        color: LxColors.grey625,
                      ),
                    ],
                  ),
                ),
                const Expanded(child: SizedBox()),
                ValueListenableBuilder(
                    valueListenable: this.confPriority,
                    builder: (context, confPriority, child) {
                      final feeSatsStr =
                          currency_format.formatSatsAmount(this.feeSats());
                      return Text(
                        "~$feeSatsStr",
                        style: textStyleSecondary,
                      );
                    })
              ],
            ),

          if (preflighted case PreflightedPayment_Invoice(:final preflight))
            Row(
              mainAxisSize: MainAxisSize.max,
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                const Text("Network Fee", style: textStyleSecondary),
                Text(
                  currency_format.formatSatsAmount(preflight.feesSats),
                  style: textStyleSecondary,
                ),
              ],
            ),

          // sparator - /\/\/\/\/\/\/\/\/\/\/

          const ReceiptSeparator(),

          //
          // Total amount sent by user/payer
          //

          Row(
            mainAxisSize: MainAxisSize.max,
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Text("Total", style: textStyleSecondary),
              ValueListenableBuilder(
                valueListenable: this.confPriority,
                builder: (context, confPriority, child) => Text(
                  currency_format.formatSatsAmount(this.totalSats()),
                  style: textStylePrimary,
                ),
              ),
            ],
          ),

          const SizedBox(height: Space.s700),

          //
          // Optional payment note input
          //

          ValueListenableBuilder(
            valueListenable: this.isSending,
            builder: (context, isSending, widget) => PaymentNoteInput(
              fieldKey: this.noteFieldKey,
              onSubmit: this.onConfirm,
              isEnabled: !isSending,
            ),
          ),

          //
          // Send payment error
          //

          ValueListenableBuilder(
            valueListenable: this.sendError,
            builder: (context, sendError, widget) => Padding(
              padding: const EdgeInsets.symmetric(vertical: Space.s400),
              child: ErrorMessageSection(sendError),
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
            ValueListenableBuilder(
              valueListenable: this.isSending,
              builder: (context, isSending, widget) => AnimatedFillButton(
                label: const Text("Send"),
                icon: const Icon(LxIcons.next),
                onTap: this.onConfirm,
                loading: isSending,
                style: FilledButton.styleFrom(
                  backgroundColor: LxColors.moneyGoUp,
                  foregroundColor: LxColors.grey1000,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class NextButton extends LxFilledButton {
  const NextButton({super.key, required super.onTap})
      : super(
          label: const Text("Next"),
          icon: const Icon(LxIcons.next),
        );
}

/// The modal dialog for the user to choose the BTC send network fee preset.
///
/// The dialog `Navigator.pop`s  a `ConfirmationPriority?`.
class ChooseOnchainFeeDialog extends StatelessWidget {
  const ChooseOnchainFeeDialog({
    super.key,
    required this.feeEstimates,
    required this.selected,
  });

  final PreflightPayOnchainResponse feeEstimates;
  final ConfirmationPriority selected;

  @override
  Widget build(BuildContext context) {
    final feeEstimatesHigh = this.feeEstimates.high;

    return SimpleDialog(
      backgroundColor: LxColors.background,
      title: const HeadingText(text: "Select network fee"),
      contentPadding: const EdgeInsets.only(bottom: Space.s500),
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(
              horizontal: Space.s500, vertical: Space.s200),
          child: Text(
            "Your payment will complete faster with a higher fee.",
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size200,
              color: LxColors.fgSecondary,
              height: 1.5,
            ),
          ),
        ),
        const SizedBox(height: Space.s200),
        // Just hide the "High" option if the user doesn't have enough funds
        // for it.
        if (feeEstimatesHigh != null)
          ChooseFeeDialogOption(
            feeEstimate: feeEstimatesHigh,
            priority: ConfirmationPriority.high,
            isSelected: this.selected == ConfirmationPriority.high,
          ),
        ChooseFeeDialogOption(
          feeEstimate: this.feeEstimates.normal,
          priority: ConfirmationPriority.normal,
          isSelected: this.selected == ConfirmationPriority.normal,
        ),
        ChooseFeeDialogOption(
          feeEstimate: this.feeEstimates.background,
          priority: ConfirmationPriority.background,
          isSelected: this.selected == ConfirmationPriority.background,
        ),
      ],
    );
  }
}

class ChooseFeeDialogOption extends StatelessWidget {
  const ChooseFeeDialogOption({
    super.key,
    required this.feeEstimate,
    required this.priority,
    required this.isSelected,
  });

  final bool isSelected;
  final FeeEstimate feeEstimate;
  final ConfirmationPriority priority;

  @override
  Widget build(BuildContext context) {
    final feeSatsStr = currency_format.formatSatsAmount(feeEstimate.amountSats);

    // TODO(phlip9): extract common rust definition from `lexe_ln::esplora`
    // The target block height (offset from the current chain tip) that we want
    // our txn confirmed.
    final confBlockTarget = switch (this.priority) {
      ConfirmationPriority.high => 1,
      ConfirmationPriority.normal => 3,
      ConfirmationPriority.background => 72,
    };
    final confDuration = Duration(minutes: 10 * confBlockTarget);
    final confDurationStr = date_format.formatDurationCompact(
      confDuration,
      abbreviated: false,
      addAgo: false,
    );

    return ListTile(
      selected: this.isSelected,
      selectedTileColor: LxColors.moneyGoUp.withAlpha(0x33),
      contentPadding: const EdgeInsets.symmetric(horizontal: Space.s500),
      visualDensity: VisualDensity.standard,
      dense: false,
      title: Row(
        mainAxisSize: MainAxisSize.max,
        mainAxisAlignment: MainAxisAlignment.start,
        children: [
          Text(this.priority.name, style: Fonts.fontUI),
          const Expanded(child: SizedBox()),
          Text(
            "~$feeSatsStr",
            style: Fonts.fontUI,
          ),
        ],
      ),
      subtitle: Row(
          mainAxisSize: MainAxisSize.max,
          mainAxisAlignment: MainAxisAlignment.start,
          children: [
            Text(
              "~$confDurationStr",
              style: Fonts.fontUI.copyWith(
                fontSize: Fonts.size200,
                color: LxColors.grey450,
              ),
            ),
            const Expanded(child: SizedBox()),
            // TODO(phlip9): fee estimate fiat value
          ]),
      onTap: () => Navigator.of(context).pop(priority),
    );
  }
}
