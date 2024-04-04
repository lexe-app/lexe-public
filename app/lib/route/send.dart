// Send payment page

import 'dart:math' show max;

import 'package:flutter/material.dart';

import 'package:lexeapp/address_format.dart' as address_format;
import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/bindings_generated_api.dart'
    show
        AppHandle,
        ClientPaymentId,
        ConfirmationPriority,
        EstimateFeeSendOnchainRequest,
        EstimateFeeSendOnchainResponse,
        FeeEstimate,
        Network,
        SendOnchainRequest;
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        DashPainter,
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
import 'package:lexeapp/input_formatter.dart'
    show AlphaNumericInputFormatter, IntInputFormatter;
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/result.dart';
import 'package:lexeapp/style.dart' show Fonts, LxColors, Space;

/// Context used during the send payment flow.
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

/// The entry point for the send payment flow.
class SendPaymentPage extends StatelessWidget {
  const SendPaymentPage({
    super.key,
    required this.sendCtx,
  });

  final SendContext sendCtx;

  @override
  Widget build(BuildContext context) {
    return MultistepFlow<bool?>(
        builder: (_) => SendPaymentAddressPage(sendCtx: sendCtx));
  }
}

class NextButton extends LxFilledButton {
  const NextButton({super.key, required super.onTap})
      : super(
          label: const Text("Next"),
          icon: const Icon(Icons.arrow_forward_rounded),
        );
}

/// In the send payment flow, this page collects the user's destination bitcoin
/// address.
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

  Future<void> onNext() async {
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

    final bool? flowResult =
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_) => SendPaymentAmountPage(
        sendCtx: this.widget.sendCtx,
        address: address,
      ),
    ));

    info("SendPaymentAddressPage: flow result: $flowResult, mounted: $mounted");

    if (!this.mounted) return;

    if (flowResult == true) {
      // ignore: use_build_context_synchronously
      await Navigator.of(this.context).maybePop(flowResult);
    }
  }

  /// Ensure the bitcoin address is properly formatted and targets the right
  /// bitcoin network (mainnet, testnet, regtest) for our build.
  Result<String, String?> validateBitcoinAddress(String? addressStr) {
    // Don't show any error message if the input is empty.
    if (addressStr == null || addressStr.isEmpty) {
      return const Err(null);
    }

    // Actually try to parse as a bitcoin address.
    // TODO(phlip9): this API should return a bare error enum and flutter should
    // convert that to a human-readable error message (for translations).
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
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
        actions: [
          IconButton(
            onPressed: this.onQrPressed,
            icon: const Icon(Icons.qr_code_rounded),
          ),
          const SizedBox(width: Space.appBarTrailingPadding),
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

/// When sending on-chain, the user has the option to send either
/// (1) an exact amount
/// (2) their full wallet balance
///
/// (2) is convenient for the user to explicitly select so they don't have to do
/// any math or know the current & exact fee rate.
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

/// Send payment flow: this page collects the [SendAmount] from the user.
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
  final ValueNotifier<String?> estimateFeeError = ValueNotifier(null);
  final ValueNotifier<bool> estimatingFee = ValueNotifier(false);

  @override
  void dispose() {
    estimatingFee.dispose();
    estimateFeeError.dispose();
    sendFullBalanceEnabled.dispose();

    super.dispose();
  }

  Future<void> onNext() async {
    // Hide error message.
    this.estimateFeeError.value = null;

    // Validate the amount field.
    final SendAmount sendAmount;
    if (sendFullBalanceEnabled.value) {
      sendAmount = const SendAmountAll();
    } else {
      final fieldState = this.amountFieldKey.currentState!;
      if (!fieldState.validate()) return;

      final value = fieldState.value;
      if (value == null || value.isEmpty) return;

      switch (this.intInputFormatter.tryParse(value)) {
        case Err():
          return;
        case Ok(:final ok):
          sendAmount = SendAmountExact(ok);
      }
    }

    final amountSats = switch (sendAmount) {
      SendAmountAll() =>
        throw UnimplementedError("Send full balance not supported yet"),
      SendAmountExact(:final amountSats) => amountSats,
    };

    // Only start the loading animation once the initial amount validation is
    // done.
    this.estimatingFee.value = true;

    // Fetch the fee estimates for this potential onchain send.
    final req = EstimateFeeSendOnchainRequest(
        address: this.widget.address, amountSats: amountSats);
    final result = await Result.tryFfiAsync(
        () async => this.widget.sendCtx.app.estimateFeeSendOnchain(req: req));

    if (!this.mounted) return;

    // Reset loading animation.
    this.estimatingFee.value = false;

    final EstimateFeeSendOnchainResponse feeEstimates;
    switch (result) {
      case Ok(:final ok):
        feeEstimates = ok;
        this.estimateFeeError.value = null;
      case Err(:final err):
        error("Error fetching fee estimates: ${err.message}");
        this.estimateFeeError.value = err.message;
        return;
    }

    // Everything looks good so far -- navigate to the confirmation page.

    final bool? flowResult =
        // ignore: use_build_context_synchronously
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_) => SendPaymentConfirmPage(
        sendCtx: this.widget.sendCtx,
        address: this.widget.address,
        sendAmount: sendAmount,
        feeEstimates: feeEstimates,
      ),
    ));

    info("SendPaymentAmountPage: flow result: $flowResult, mounted: $mounted");

    if (!this.mounted) return;

    if (flowResult == true) {
      // ignore: use_build_context_synchronously
      await Navigator.of(this.context).maybePop(flowResult);
    }
  }

  Result<(), String?> validateAmount(int amount) {
    if (amount > this.widget.sendCtx.balanceSats) {
      return const Err("You can't send more than your current balance.");
    }

    return const Ok(());
  }

  @override
  Widget build(BuildContext context) {
    final balanceStr = currency_format
        .formatSatsAmount(this.widget.sendCtx.balanceSats, satsSuffix: true);

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
          ),

          const SizedBox(height: Space.s700),
        ],
        bottom: Column(
          mainAxisSize: MainAxisSize.min,
          mainAxisAlignment: MainAxisAlignment.end,
          verticalDirection: VerticalDirection.down,
          children: [
            const Expanded(child: SizedBox(height: Space.s500)),

            // Send full balance switch
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

            // Error fetching fee estimate
            ValueListenableBuilder(
              valueListenable: this.estimateFeeError,
              builder: (context, errorMessage, widget) => ErrorMessageSection(
                title: "Error fetching fee estimate",
                message: errorMessage,
              ),
            ),

            // Next ->
            ValueListenableBuilder(
              valueListenable: this.estimatingFee,
              builder: (context, estimatingFee, widget) => Padding(
                padding: const EdgeInsets.only(top: Space.s500),
                child: AnimatedFillButton(
                  label: const Text("Next"),
                  icon: const Icon(Icons.arrow_forward_rounded),
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
    required this.address,
    required this.sendAmount,
    required this.feeEstimates,
  });

  final SendContext sendCtx;
  final String address;
  final SendAmount sendAmount;
  final EstimateFeeSendOnchainResponse feeEstimates;

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

  Future<void> onSend() async {
    if (this.isSending.value) return;

    // We're sending; clear the errors and disable the form inputs.
    this.isSending.value = true;
    this.sendError.value = null;

    final amountSats = switch (this.widget.sendAmount) {
      SendAmountExact(:final amountSats) => amountSats,
      // TODO(phlip9): implement "send full balance"
      SendAmountAll() => throw UnimplementedError(),
    };
    final req = SendOnchainRequest(
      cid: this.widget.sendCtx.cid,
      address: this.widget.address,
      amountSats: amountSats,
      priority: ConfirmationPriority.Normal,
    );

    final app = this.widget.sendCtx.app;

    final result =
        await Result.tryFfiAsync(() async => app.sendOnchain(req: req));

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

  Future<void> chooseFeeRate() async {
    final ConfirmationPriority? result = await showDialog(
      context: this.context,
      useRootNavigator: false,
      builder: (context) => ChooseFeeDialog(
        feeEstimates: this.widget.feeEstimates,
        selected: this.confPriority.value,
      ),
    );

    if (!this.mounted) return;

    if (result != null) {
      this.confPriority.value = result;
    }
  }

  int amountSats() => switch (this.widget.sendAmount) {
        SendAmountExact(:final amountSats) => amountSats,
        // TODO(phlip9): the exact amount will need to come from the
        // pre-validation + fee estimation request.
        SendAmountAll() => this.widget.sendCtx.balanceSats,
      };

  int feeSats() {
    final feeEstimates = this.widget.feeEstimates;
    return switch (this.confPriority.value) {
      // invariant: High can not be selected if there are insufficient funds
      ConfirmationPriority.High => feeEstimates.high!.amountSats,
      ConfirmationPriority.Normal => feeEstimates.normal.amountSats,
      ConfirmationPriority.Background => feeEstimates.background.amountSats,
    };
  }

  int totalSats() => this.amountSats() + this.feeSats();

  @override
  Widget build(BuildContext context) {
    final shortAddr = address_format.ellipsizeBtcAddress(this.widget.address);

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
          const SubheadingText(text: "Sending bitcoin on-chain"),
          const SizedBox(height: Space.s700),

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
            mainAxisAlignment: MainAxisAlignment.start,
            children: [
              TextButton(
                onPressed: this.chooseFeeRate,
                style: TextButton.styleFrom(
                  textStyle: textStyleSecondary,
                  foregroundColor: LxColors.grey550,
                  shape: const LinearBorder(),
                  padding: const EdgeInsets.only(right: Space.s200),
                ),
                // Sadly flutter doesn't allow us to increase the space b/w the
                // text and the underline. The default text decoration looks
                // ugly af. So we have this hack to draw a dashed line...
                child: const Stack(
                  children: [
                    // dashed underline beneath text
                    Positioned(
                      left: 0.0,
                      right: 0.0,
                      bottom: 0.0,
                      child: CustomPaint(
                          painter: DashPainter(
                              color: LxColors.grey650, dashThickness: 1.5)),
                    ),
                    // Network Fee text + icon
                    Row(
                      mainAxisSize: MainAxisSize.min,
                      mainAxisAlignment: MainAxisAlignment.start,
                      children: [
                        Text("Network Fee"),
                        SizedBox(width: Space.s200),
                        Icon(
                          Icons.edit_rounded,
                          size: Fonts.size300,
                          color: LxColors.grey625,
                        ),
                      ],
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

          // Optional payment note input
          ValueListenableBuilder(
            valueListenable: this.isSending,
            builder: (context, isSending, widget) => PaymentNoteInput(
              fieldKey: this.noteFieldKey,
              onSubmit: this.onSend,
              isEnabled: !isSending,
            ),
          ),

          // Send payment error
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
                icon: const Icon(Icons.arrow_forward_rounded),
                onTap: this.onSend,
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

/// The modal dialog for the user to choose the BTC send network fee preset.
///
/// The dialog `Navigator.pop`s  a `ConfirmationPriority?`.
class ChooseFeeDialog extends StatelessWidget {
  const ChooseFeeDialog({
    super.key,
    required this.feeEstimates,
    required this.selected,
  });

  final EstimateFeeSendOnchainResponse feeEstimates;
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
