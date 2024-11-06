import 'package:app_rs_dart/ffi/api.dart'
    show PreflightOpenChannelRequest, PreflightOpenChannelResponse;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show ChannelId;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxBackButton,
        MultistepFlow,
        PaymentAmountInput,
        ScrollableSinglePageBody,
        SubBalanceRow,
        SubheadingText;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/input_formatter.dart' show IntInputFormatter;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/style.dart' show LxIcons, Space;
import 'package:lexeapp/types.dart' show BalanceKind, BalanceState;

@immutable
final class OpenChannelFlowResult {
  const OpenChannelFlowResult({required this.channelId});

  final ChannelId channelId;

  @override
  String toString() => "OpenChannelFlowResult(channelId: $channelId)";
}

/// The entry point for the open channel flow.
class OpenChannelPage extends StatelessWidget {
  const OpenChannelPage({
    super.key,
    required this.app,
    required this.balanceState,
  });

  final AppHandle app;

  /// The current top-level balance.
  final ValueListenable<BalanceState> balanceState;

  @override
  Widget build(BuildContext context) => MultistepFlow<OpenChannelFlowResult>(
        builder: (context) => OpenChannelNeedValuePage(
          app: this.app,
          balanceState: this.balanceState,
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
  });

  final AppHandle app;

  /// The current top-level balance.
  final ValueListenable<BalanceState> balanceState;

  @override
  State<OpenChannelNeedValuePage> createState() =>
      _OpenChannelNeedValuePageState();
}

class _OpenChannelNeedValuePageState extends State<OpenChannelNeedValuePage> {
  final GlobalKey<FormFieldState<String>> valueFieldKey = GlobalKey();

  final IntInputFormatter intInputFormatter = IntInputFormatter();

  final ValueNotifier<ErrorMessage?> estimateFeeError = ValueNotifier(null);
  final ValueNotifier<bool> estimatingFee = ValueNotifier(false);

  Result<(), String?> validateValue(int value) {
    final onchainSats = this.widget.balanceState.value.onchainSats();

    // Not connected yet? Just prevent submission.
    if (onchainSats == null) {
      return const Err(null);
    }

    // Basic check against balance. More complete checks happen in preflight.
    if (value > onchainSats) {
      final onchainSatsStr =
          currency_format.formatSatsAmount(onchainSats, satsSuffix: true);
      return Err("Channel value can't be larger than your on-chain balance "
          "($onchainSatsStr).");
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

    // TODO(phlip9): navigate to confirmation page
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
              text: "Move on-chain Bitcoin into a Lightning channel to send "
                  "payments instantly."),

          const SizedBox(height: Space.s500),

          // On-chain balance
          ValueListenableBuilder(
            valueListenable: this.widget.balanceState,
            builder: (context, balanceState, child) => SubBalanceRow(
              kind: BalanceKind.onchain,
              balance: balanceState,
            ),
          ),

          const SizedBox(height: Space.s600),

          // <amount> sats
          PaymentAmountInput(
            fieldKey: this.valueFieldKey,
            intInputFormatter: this.intInputFormatter,
            onEditingComplete: this.onNext,
            validate: this.validateValue,
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
              builder: (_context, errorMessage, _widget) =>
                  ErrorMessageSection(errorMessage),
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
