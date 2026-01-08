/// UI flow for users to open a new channel with the Lexe LSP.
library;

import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/api.dart'
    show
        OpenChannelRequest,
        PreflightOpenChannelRequest,
        PreflightOpenChannelResponse;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show UserChannelId;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        ItemizedAmountRow,
        ListIcon,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        MultistepFlow,
        PaymentAmountInput,
        ReceiptSeparator,
        ScrollableSinglePageBody,
        SubBalanceRow,
        SubheadingText;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/input_formatter.dart' show IntInputFormatter;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/style.dart' show LxColors, LxIcons, Space;
import 'package:lexeapp/types.dart' show BalanceKind, BalanceState, FiatAmount;

@immutable
final class OpenChannelFlowResult {
  const OpenChannelFlowResult({required this.channelId});

  final String channelId;

  @override
  String toString() => "OpenChannelFlowResult(channelId: $channelId)";
}

/// The entry point for the open channel flow.
class OpenChannelPage extends StatelessWidget {
  const OpenChannelPage({
    super.key,
    required this.app,
    required this.balanceState,
    this.designInitialAmount,
  });

  final AppHandle app;

  /// The current top-level balance.
  final ValueListenable<BalanceState> balanceState;

  /// (Design mode screenshot automation only) Pre-fill channel amount.
  final int? designInitialAmount;

  @override
  Widget build(BuildContext context) => MultistepFlow<OpenChannelFlowResult>(
    builder: (context) => OpenChannelNeedValuePage(
      app: this.app,
      balanceState: this.balanceState,
      designInitialAmount: this.designInitialAmount,
    ),
  );
}

/// The page where we collect the channel value for the new channel the user
/// wants to open.
class OpenChannelNeedValuePage extends StatefulWidget {
  const OpenChannelNeedValuePage({
    super.key,
    required this.app,
    required this.balanceState,
    this.designInitialAmount,
  });

  final AppHandle app;

  /// The current top-level balance.
  final ValueListenable<BalanceState> balanceState;

  /// (Design mode screenshot automation only) Pre-fill channel amount.
  final int? designInitialAmount;

  @override
  State<OpenChannelNeedValuePage> createState() =>
      _OpenChannelNeedValuePageState();
}

class _OpenChannelNeedValuePageState extends State<OpenChannelNeedValuePage> {
  final GlobalKey<FormFieldState<String>> valueFieldKey = GlobalKey();

  final IntInputFormatter intInputFormatter = IntInputFormatter();

  final ValueNotifier<ErrorMessage?> estimateFeeError = ValueNotifier(null);
  final ValueNotifier<bool> estimatingFee = ValueNotifier(false);

  @override
  void dispose() {
    this.estimatingFee.dispose();
    this.estimateFeeError.dispose();
    super.dispose();
  }

  Result<(), String> validateValue(int value) {
    final onchainSats = this.widget.balanceState.value.onchainSats();

    // Not connected yet? Just prevent submission.
    if (onchainSats == null) {
      return const Err("");
    }

    // Basic check against balance. More complete checks happen in preflight.
    if (value > onchainSats) {
      final onchainSatsStr = currency_format.formatSatsAmount(
        onchainSats,
        bitcoinSymbol: true,
      );
      return Err(
        "Channel value can't be larger than your on-chain balance "
        "($onchainSatsStr).",
      );
    }

    return const Ok(());
  }

  Future<void> onNext() async {
    // Hide error message
    this.estimateFeeError.value = null;

    // Validate the value field
    final fieldState = this.valueFieldKey.currentState!;
    if (!fieldState.validate()) return;

    final value = fieldState.value;
    if (value == null || value.isEmpty) return;

    final int valueSats;
    switch (this.intInputFormatter.tryParse(value)) {
      case Err():
        return;
      case Ok(:final ok):
        valueSats = ok;
    }

    // Only start the loading animation once the value validation is done.
    this.estimatingFee.value = true;

    // Preflight channel open. Check for enough balance and return est. fees
    final req = PreflightOpenChannelRequest(valueSats: valueSats);
    final result = await Result.tryFfiAsync(
      () => this.widget.app.preflightOpenChannel(req: req),
    );
    if (!this.mounted) return;

    // Reset loading animation
    this.estimatingFee.value = false;

    // Check if preflight was successful, or show an error message.
    final PreflightOpenChannelResponse resp;
    switch (result) {
      case Ok(:final ok):
        this.estimateFeeError.value = null;
        resp = ok;
      case Err(:final err):
        error("Error preflighting channel open: $err");
        this.estimateFeeError.value = ErrorMessage(
          title: "Error preflighting channel open",
          message: err.message,
        );
        return;
    }

    info("preflight_open_channel($valueSats) -> fees: ${resp.feeEstimateSats}");

    // Navigate to confirm page and pop with the result if successful
    final OpenChannelFlowResult? flowResult = await Navigator.of(this.context)
        .push(
          MaterialPageRoute(
            builder: (context) => OpenChannelConfirmPage(
              app: this.widget.app,
              balanceState: this.widget.balanceState,
              channelValueSats: valueSats,
              userChannelId: UserChannelId.genNew(),
              preflight: resp,
            ),
          ),
        );
    info("OpenChannelNeedValuePage: flowResult: $flowResult");
    if (!this.mounted || flowResult == null) return;

    await Navigator.of(this.context).maybePop(flowResult);
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
          const HeadingText(text: "Open Lightning channel"),
          const SubheadingText(
            text:
                "Move on-chain Bitcoin into a Lightning channel to send "
                "payments instantly.",
          ),

          const SizedBox(height: Space.s500),

          // On-chain balance
          ValueListenableBuilder(
            valueListenable: this.widget.balanceState,
            builder: (context, balanceState, child) =>
                SubBalanceRow(kind: BalanceKind.onchain, balance: balanceState),
          ),

          const SizedBox(height: Space.s600),

          // <amount> sats
          PaymentAmountInput(
            fieldKey: this.valueFieldKey,
            intInputFormatter: this.intInputFormatter,
            onEditingComplete: this.onNext,
            validate: this.validateValue,
            allowEmpty: false,
            allowZero: false,
            initialValue: this.widget.designInitialAmount,
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

/// Ask the user to confirm the channel open fees after preflighting.
class OpenChannelConfirmPage extends StatefulWidget {
  const OpenChannelConfirmPage({
    super.key,
    required this.app,
    required this.balanceState,
    required this.channelValueSats,
    required this.userChannelId,
    required this.preflight,
  });

  final AppHandle app;

  /// The current top-level balance.
  final ValueListenable<BalanceState> balanceState;

  /// The channel value from the previous page.
  final int channelValueSats;

  /// The idempotency key.
  final UserChannelId userChannelId;

  /// The estimated fees for this channel open.
  final PreflightOpenChannelResponse preflight;

  @override
  State<OpenChannelConfirmPage> createState() => _OpenChannelConfirmPageState();
}

class _OpenChannelConfirmPageState extends State<OpenChannelConfirmPage> {
  final ValueNotifier<ErrorMessage?> openError = ValueNotifier(null);
  final ValueNotifier<bool> isPending = ValueNotifier(false);

  @override
  void dispose() {
    this.isPending.dispose();
    this.openError.dispose();
    super.dispose();
  }

  /// Try to open the channel after the user confirms.
  Future<void> onConfirm() async {
    // Don't allow submission while a channel open is pending.
    if (this.isPending.value) return;

    // Start loading and reset any errors.
    this.isPending.value = true;
    this.openError.value = null;

    // Open the channel.
    final req = OpenChannelRequest(
      userChannelId: this.widget.userChannelId,
      valueSats: this.widget.channelValueSats,
    );
    final result = await Result.tryFfiAsync(
      () => this.widget.app.openChannel(req: req),
    );

    if (!this.mounted) return;

    final OpenChannelFlowResult flowResult;
    switch (result) {
      case Ok(:final ok):
        info("OpenChannelConfirmPage: success: flowResult: $ok");
        flowResult = OpenChannelFlowResult(channelId: ok.channelId);
      case Err(:final err):
        error("OpenChannelConfirmPage: error: ${err.message}");
        this.isPending.value = false;
        this.openError.value = ErrorMessage(
          title: "Failed to open channel",
          message: err.message,
        );
        return;
    }

    unawaited(Navigator.of(this.context).maybePop(flowResult));
  }

  @override
  Widget build(BuildContext context) {
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
          const HeadingText(text: "Confirm channel open"),
          const SubheadingText(
            text: "Moving on-chain Bitcoin into a Lightning channel.",
          ),

          const SizedBox(height: Space.s700),

          // Show the "itemized" receipt for this channel open.
          //
          // Most importantly, the user needs to confirm the on-chain fee.
          // Secondly, we need to communicate that the on-chain balance is
          // getting "converted" into their lightning balance.
          ValueListenableBuilder(
            valueListenable: this.widget.balanceState,
            builder: (context, balanceState, child) {
              final fiatRate = balanceState.fiatRate;
              final channelSats = this.widget.channelValueSats;
              final channelFiat = FiatAmount.maybeFromSats(
                fiatRate,
                channelSats,
              );
              final feeSats = this.widget.preflight.feeEstimateSats;
              final feeFiat = FiatAmount.maybeFromSats(fiatRate, feeSats);

              return Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  // In: On-chain balance
                  ItemizedAmountRow(
                    fiatAmount: channelFiat,
                    satsAmount: channelSats,
                    title: "On-chain",
                    // subtitle: "Channel deposit",
                    subtitle: "",
                    icon: const ListIcon.bitcoin(),
                  ),

                  // In: Miner fee
                  ItemizedAmountRow(
                    fiatAmount: feeFiat,
                    satsAmount: feeSats,
                    title: "Miner fee",
                    subtitle: "",
                    // subtitle: "Paid to the BTC network",
                    icon: const SizedBox.square(dimension: Space.s650),
                    // icon: const ListIcon.bitcoin(),
                  ),

                  const ReceiptSeparator(),

                  // Out: Lightning balance
                  ItemizedAmountRow(
                    fiatAmount: channelFiat,
                    satsAmount: channelSats,
                    title: "Lightning",
                    subtitle: "",
                    // subtitle: "New channel",
                    icon: const ListIcon.lightning(),
                  ),
                ],
              );
            },
          ),

          const SizedBox(height: Space.s700),

          // Error opening channel
          ValueListenableBuilder(
            valueListenable: this.openError,
            builder: (_context, errorMessage, _widget) =>
                ErrorMessageSection(errorMessage),
          ),
        ],

        // Open channel ->
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.isPending,
            builder: (_context, estimatingFee, _widget) => AnimatedFillButton(
              label: const Text("Open channel"),
              icon: const Icon(LxIcons.next),
              onTap: this.onConfirm,
              loading: estimatingFee,
              style: FilledButton.styleFrom(
                backgroundColor: LxColors.moneyGoUp,
                foregroundColor: LxColors.grey1000,
                iconColor: LxColors.grey1000,
              ),
            ),
          ),
        ),
      ),
    );
  }
}
