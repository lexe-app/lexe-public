// Send payment page

// ignore_for_file: camel_case_types

import 'package:flutter/material.dart';
import 'package:lexeapp/address_format.dart' as address_format;
import 'package:lexeapp/bindings_generated_api.dart'
    show
        ConfirmationPriority,
        FeeEstimate,
        PayInvoiceRequest,
        PayOnchainRequest,
        PaymentKind,
        PreflightPayOnchainResponse;
import 'package:lexeapp/bindings_generated_api_ext.dart';
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        HeadingText,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        MultistepFlow,
        PaymentAmountInput,
        PaymentNoteInput,
        ScrollableSinglePageBody,
        SubheadingText,
        ZigZag,
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
        SendContext,
        SendContext_NeedAmount,
        SendContext_NeedUri,
        SendContext_Preflighted;
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;

/// The entry point for the send payment flow. This will dispatch to the right
/// initial screen depending on the [SendContext]. If [startNewFlow], then it
/// also sets up a new / [MultistepFlow] so navigation "close" will exit out of
/// the whole flow.
class SendPaymentPage extends StatelessWidget {
  const SendPaymentPage({
    super.key,
    required this.sendCtx,
    required this.startNewFlow,
  });

  final SendContext sendCtx;
  final bool startNewFlow;

  Widget buildInnerSendPage() {
    final sendCtx = this.sendCtx;
    return switch (sendCtx) {
      SendContext_Preflighted() => SendPaymentConfirmPage(sendCtx: sendCtx),
      SendContext_NeedAmount() => SendPaymentAmountPage(sendCtx: sendCtx),
      SendContext_NeedUri() => SendPaymentNeedUriPage(sendCtx: sendCtx),
    };
  }

  @override
  Widget build(BuildContext context) => (this.startNewFlow)
      ? MultistepFlow<bool?>(builder: (_) => this.buildInnerSendPage())
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

  final SendContext_NeedUri sendCtx;

  @override
  State<StatefulWidget> createState() => _SendPaymentNeedUriPageState();
}

class _SendPaymentNeedUriPageState extends State<SendPaymentNeedUriPage> {
  final GlobalKey<FormFieldState<String>> paymentUriFieldKey = GlobalKey();

  final ValueNotifier<bool> isPending = ValueNotifier(false);
  final ValueNotifier<String?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.errorMessage.dispose();
    this.isPending.dispose();

    super.dispose();
  }

  Future<void> onScanPressed() async {
    info("pressed QR scan button");

    final bool? flowResult =
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_context) => ScanPage(sendCtx: this.widget.sendCtx),
    ));
    if (!this.mounted) return;

    // Successfully sent payment -- return result to parent page.
    if (flowResult == true) {
      // ignore: use_build_context_synchronously
      await Navigator.of(this.context).maybePop(flowResult);
    }
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
    final SendContext sendCtx;
    switch (result) {
      case Ok(:final ok):
        sendCtx = ok;
      case Err(:final err):
        this.errorMessage.value = err;
        return;
    }

    // If we still need an amount, then we have to collect that first.
    // Otherwise, a successful payment preflight means we can go directly to the
    // confirm page.
    final bool? flowResult =
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_) => SendPaymentPage(sendCtx: sendCtx, startNewFlow: false),
    ));

    info("SendPaymentNeedUriPage: flow result: $flowResult, mounted: $mounted");
    if (!this.mounted) return;

    // Successfully sent payment -- return result to parent page.
    if (flowResult == true) {
      // ignore: use_build_context_synchronously
      await Navigator.of(this.context).maybePop(flowResult);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
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
                hintText: "Address, Invoice, Node Pubkey"),
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
        bottom: Column(
          mainAxisSize: MainAxisSize.min,
          mainAxisAlignment: MainAxisAlignment.end,
          children: [
            const Expanded(child: SizedBox(height: Space.s500)),

            // Error parsing, resolving, and/or preflighting payment
            ValueListenableBuilder(
              valueListenable: this.errorMessage,
              builder: (_context, errorMessage, _widget) => ErrorMessageSection(
                title: "",
                message: errorMessage,
              ),
            ),

            // -> Next
            ValueListenableBuilder(
              valueListenable: this.isPending,
              builder: (_context, isPending, _widget) => Padding(
                padding: const EdgeInsets.only(top: Space.s500),
                child: AnimatedFillButton(
                  label: const Text("Next"),
                  icon: const Icon(LxIcons.next),
                  onTap: this.onNext,
                  loading: isPending,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Send payment flow: this page collects the [SendAmount] from the user.
class SendPaymentAmountPage extends StatefulWidget {
  const SendPaymentAmountPage({
    super.key,
    required this.sendCtx,
  });

  final SendContext_NeedAmount sendCtx;

  @override
  State<SendPaymentAmountPage> createState() => _SendPaymentAmountPageState();
}

class _SendPaymentAmountPageState extends State<SendPaymentAmountPage> {
  final GlobalKey<FormFieldState<String>> amountFieldKey = GlobalKey();

  final IntInputFormatter intInputFormatter = IntInputFormatter();

  final ValueNotifier<String?> estimateFeeError = ValueNotifier(null);
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
    final SendContext_Preflighted nextSendCtx;
    switch (result) {
      case Ok(:final ok):
        nextSendCtx = ok;
        this.estimateFeeError.value = null;
      case Err(:final err):
        error("Error preflighting payment: $err");
        this.estimateFeeError.value = err.message;
        return;
    }

    // Everything looks good so far -- navigate to the confirmation page.
    final bool? flowResult =
        // ignore: use_build_context_synchronously
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_) => SendPaymentConfirmPage(sendCtx: nextSendCtx),
    ));

    // Confirm page results:
    info("SendPaymentAmountPage: flow result: $flowResult, mounted: $mounted");

    if (!this.mounted) return;

    if (flowResult == true) {
      // ignore: use_build_context_synchronously
      await Navigator.of(this.context).maybePop(flowResult);
    }
  }

  Result<(), String?> validateAmount(int amount) {
    final balanceSats = this.widget.sendCtx.balanceSats();
    if (amount > balanceSats) {
      final kind = this.widget.sendCtx.paymentMethod.kind();
      final kindLabel = switch (kind) {
        PaymentKind.Onchain => "on-chain",
        PaymentKind.Invoice || PaymentKind.Spontaneous => "lightning",
      };
      final balanceStr =
          currency_format.formatSatsAmount(balanceSats, satsSuffix: true);
      return Err(
          "This amount is more than your bitcoin $kindLabel spendable balance of $balanceStr.");
    }

    return const Ok(());
  }

  @override
  Widget build(BuildContext context) {
    final balanceStr = currency_format
        .formatSatsAmount(this.widget.sendCtx.balanceSats(), satsSuffix: true);

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "How much?"),
          SubheadingText(text: "balance $balanceStr"),
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
        ],
        bottom: Column(
          mainAxisSize: MainAxisSize.min,
          mainAxisAlignment: MainAxisAlignment.end,
          children: [
            const Expanded(child: SizedBox(height: Space.s500)),

            // Error fetching fee estimate
            ValueListenableBuilder(
              valueListenable: this.estimateFeeError,
              builder: (_context, errorMessage, _widget) => ErrorMessageSection(
                title: "Error fetching fee estimate",
                message: errorMessage,
              ),
            ),

            // Next ->
            ValueListenableBuilder(
              valueListenable: this.estimatingFee,
              builder: (_context, estimatingFee, _widget) => Padding(
                padding: const EdgeInsets.only(top: Space.s500),
                child: AnimatedFillButton(
                  label: const Text("Next"),
                  icon: const Icon(LxIcons.next),
                  onTap: this.onNext,
                  loading: estimatingFee,
                ),
              ),
            ),
          ],
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

  final SendContext_Preflighted sendCtx;

  @override
  State<SendPaymentConfirmPage> createState() => _SendPaymentConfirmPageState();
}

class _SendPaymentConfirmPageState extends State<SendPaymentConfirmPage> {
  final GlobalKey<FormFieldState<String>> noteFieldKey = GlobalKey();

  final ValueNotifier<String?> sendError = ValueNotifier(null);
  final ValueNotifier<bool> isSending = ValueNotifier(false);

  // TODO(phlip9): save/load this from/to user preferences?
  final ValueNotifier<ConfirmationPriority> confPriority =
      ValueNotifier(ConfirmationPriority.Normal);

  @override
  void dispose() {
    this.confPriority.dispose();
    this.isSending.dispose();
    this.sendError.dispose();
    super.dispose();
  }

  Future<FfiResult<void>> doPayOnchain(
      final PreflightedPayment_Onchain preflighted) async {
    final req = PayOnchainRequest(
      cid: this.widget.sendCtx.cid,
      address: preflighted.onchain.address,
      amountSats: preflighted.amountSats,
      priority: this.confPriority.value,
      note: this.note(),
    );

    final app = this.widget.sendCtx.app;

    return Result.tryFfiAsync(() async => app.payOnchain(req: req));
  }

  Future<FfiResult<void>> doPayInvoice(
      final PreflightedPayment_Invoice preflighted) async {
    final req = PayInvoiceRequest(
      invoice: preflighted.invoice.string,
      fallbackAmountSats: preflighted.amountSats,
      note: this.note(),
    );

    final app = this.widget.sendCtx.app;
    return Result.tryFfiAsync(() async => app.payInvoice(req: req));
  }

  Future<void> onConfirm() async {
    if (this.isSending.value) return;

    // We're sending; clear the errors and disable the form inputs.
    this.isSending.value = true;
    this.sendError.value = null;

    final preflighted = this.widget.sendCtx.preflightedPayment;
    final result = switch (preflighted) {
      PreflightedPayment_Onchain() => await this.doPayOnchain(preflighted),
      PreflightedPayment_Invoice() => await this.doPayInvoice(preflighted),
      PreflightedPayment_Offer() => throw UnimplementedError(),
    };

    if (!this.mounted) return;

    switch (result) {
      case Ok():
        // The request succeeded and we're still mounted (the user hasn't
        // navigated away somehow). Let's pop ourselves off the nav stack and
        // notify our caller that we were successful.
        info("SendPaymentConfirmPage: on-chain send success");
        const flowResult = true;
        // ignore: use_build_context_synchronously
        await Navigator.of(this.context).maybePop(flowResult);
        return;

      case Err(:final err):
        // The request failed. Set the error message and unset loading.
        error("SendPaymentConfirmPage: error sending on-chain payment: $err");
        this.isSending.value = false;
        this.sendError.value = err.message;
        return;
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
            ConfirmationPriority.High => preflight.high!.amountSats,
            ConfirmationPriority.Normal => preflight.normal.amountSats,
            ConfirmationPriority.Background => preflight.background.amountSats,
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
      PaymentKind.Onchain => "Sending bitcoin on-chain",
      PaymentKind.Invoice => "Sending bitcoin via lightning invoice",
      PaymentKind.Spontaneous =>
        "Sending bitcoin via lightning spontaneous payment",
    };

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(),
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

          const SizedBox(
            height: Space.s650,
            child: ZigZag(
                color: LxColors.grey750, zigWidth: 14.0, strokeWidth: 1.0),
          ),

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
              padding: const EdgeInsets.symmetric(vertical: Space.s300),
              child: ErrorMessageSection(
                title: "Error sending payment",
                message: sendError,
              ),
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
            priority: ConfirmationPriority.High,
            isSelected: this.selected == ConfirmationPriority.High,
          ),
        ChooseFeeDialogOption(
          feeEstimate: this.feeEstimates.normal,
          priority: ConfirmationPriority.Normal,
          isSelected: this.selected == ConfirmationPriority.Normal,
        ),
        ChooseFeeDialogOption(
          feeEstimate: this.feeEstimates.background,
          priority: ConfirmationPriority.Background,
          isSelected: this.selected == ConfirmationPriority.Background,
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
      ConfirmationPriority.High => 1,
      ConfirmationPriority.Normal => 3,
      ConfirmationPriority.Background => 72,
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

class ErrorMessageSection extends StatelessWidget {
  const ErrorMessageSection({
    super.key,
    required this.title,
    required this.message,
  });

  final String title;
  final String? message;

  @override
  Widget build(BuildContext context) {
    final message = this.message;

    // TODO(phlip9): maybe tap to expand full error message?
    // TODO(phlip9): slide up animation?

    return AnimatedSwitcher(
      duration: const Duration(milliseconds: 200),
      child: (message != null)
          ? ListTile(
              contentPadding: EdgeInsets.zero,
              title: Text(
                this.title,
                style: const TextStyle(
                  color: LxColors.errorText,
                  fontVariations: [Fonts.weightMedium],
                  height: 2.0,
                ),
              ),
              subtitle: Text(
                message,
                maxLines: 3,
                style: const TextStyle(
                  color: LxColors.errorText,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            )
          : null,
    );
  }
}
