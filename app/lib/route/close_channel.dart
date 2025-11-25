/// UI flow for users to close one of their open channels with the Lexe LSP.
library;

import 'dart:async' show unawaited;
import 'dart:math' show max;

import 'package:app_rs_dart/ffi/api.dart'
    show CloseChannelRequest, FiatRate, PreflightCloseChannelResponse;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
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
        ReceiptSeparator,
        ScrollableSinglePageBody,
        SubheadingText,
        showModalAsyncFlow;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/channels.dart'
    show
        Channel,
        ChannelsList,
        ChannelsListEntry,
        ChannelsPartyChip,
        channelsListEntryHeight;
import 'package:lexeapp/style.dart' show LxColors, LxIcons, Space;
import 'package:lexeapp/types.dart';

@immutable
final class CloseChannelFlowResult {
  const CloseChannelFlowResult({required this.channelId});

  final String channelId;

  @override
  String toString() => "CloseChannelFlowResult(channelId: $channelId)";
}

/// The entry point for the open channel flow.
class CloseChannelPage extends StatelessWidget {
  const CloseChannelPage({
    super.key,
    required this.app,
    required this.fiatRate,
    required this.channels,
  });

  final AppHandle app;

  /// Updating stream of fiat rates.
  final ValueListenable<FiatRate?> fiatRate;

  /// Updated list of current channels.
  final ValueListenable<ChannelsList?> channels;

  @override
  Widget build(BuildContext context) => MultistepFlow<CloseChannelFlowResult>(
    builder: (context) => CloseChannelChoosePage(
      app: this.app,
      fiatRate: this.fiatRate,
      channels: this.channels,
    ),
  );
}

/// On this page the user chooses which channel they want to close.
class CloseChannelChoosePage extends StatefulWidget {
  const CloseChannelChoosePage({
    super.key,
    required this.app,
    required this.fiatRate,
    required this.channels,
  });

  final AppHandle app;

  /// Updating stream of fiat rates.
  final ValueListenable<FiatRate?> fiatRate;

  /// Updated list of current channels.
  final ValueListenable<ChannelsList?> channels;

  @override
  State<CloseChannelChoosePage> createState() => _CloseChannelChoosePageState();
}

class _CloseChannelChoosePageState extends State<CloseChannelChoosePage> {
  Future<void> onChannelSelected(final Channel channel) async {
    info(
      "CloseChannelChoosePage: selected channel: ${channel.channelId}, "
      "our sats: ${channel.ourBalanceSats}",
    );

    // Preflight the channel close to get fee estimates
    final req = CloseChannelRequest(channelId: channel.channelId);
    final Result<PreflightCloseChannelResponse, FfiError>? res =
        await showModalAsyncFlow(
          context: this.context,
          future: Result.tryFfiAsync(
            () => this.widget.app.preflightCloseChannel(req: req),
          ),
          barrierDismissible: true,
          errorBuilder: (context, err) => AlertDialog(
            title: const Text("Error"),
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

    info("CloseChannelChoosePage: preflight: $res");
    if (!this.mounted || res == null) return;

    final PreflightCloseChannelResponse preflight;
    switch (res) {
      case Ok(:final ok):
        preflight = ok;
      case Err():
        return;
    }

    // Navigate to confirm page and pop with the result if successful
    final CloseChannelFlowResult? flowResult = await Navigator.of(this.context)
        .push(
          MaterialPageRoute(
            builder: (context) => CloseChannelConfirmPage(
              app: this.widget.app,
              fiatRate: this.widget.fiatRate,
              channelId: channel.channelId,
              channelOurBalanceSats: channel.ourBalanceSats,
              preflight: preflight,
            ),
          ),
        );
    info("CloseChannelChoosePage: flowResult: $flowResult");
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
        bodySlivers: [
          // Heading
          const SliverToBoxAdapter(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                HeadingText(text: "Close Lightning channel"),
                SubheadingText(
                  text: "Move funds in a Lightning channel back on-chain.",
                ),
                SizedBox(height: Space.s500),

                // You/Lexe LSP channels heading
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    ChannelsPartyChip(name: "You"),
                    ChannelsPartyChip(name: "Lexe LSP"),
                  ],
                ),
                SizedBox(height: Space.s200),
              ],
            ),
          ),

          // Channels
          ValueListenableBuilder(
            valueListenable: this.widget.channels,
            builder: (context, channelsList, child) =>
                SliverFixedExtentList.list(
                  itemExtent: channelsListEntryHeight,
                  children: (channelsList != null)
                      ? channelsList.channels
                            .map(
                              (channel) => Material(
                                elevation: 0.0,
                                color: LxColors.clearW0,
                                child: InkWell(
                                  onTap: () => this.onChannelSelected(channel),
                                  child: ChannelsListEntry(
                                    maxValueSats: channelsList.maxValueSats,
                                    channel: channel,
                                    fiatRate: this.widget.fiatRate,
                                  ),
                                ),
                              ),
                            )
                            .toList()
                      : [],
                ),
          ),
        ],
      ),
    );
  }
}

/// On this page the user can see the fees they'll have to pay to close.
/// They can then confirm actually close the channel.
class CloseChannelConfirmPage extends StatefulWidget {
  const CloseChannelConfirmPage({
    super.key,
    required this.app,
    required this.fiatRate,
    required this.channelId,
    required this.channelOurBalanceSats,
    required this.preflight,
  });

  final AppHandle app;

  /// Updating stream of fiat rates.
  final ValueListenable<FiatRate?> fiatRate;

  final String channelId;

  /// Our balance for the selected channel.
  final int channelOurBalanceSats;

  /// The preflight/fee estimate for closing this channel.
  final PreflightCloseChannelResponse preflight;

  @override
  State<CloseChannelConfirmPage> createState() =>
      _CloseChannelConfirmPageState();
}

class _CloseChannelConfirmPageState extends State<CloseChannelConfirmPage> {
  final ValueNotifier<ErrorMessage?> closeError = ValueNotifier(null);
  final ValueNotifier<bool> isPending = ValueNotifier(false);

  @override
  void dispose() {
    this.isPending.dispose();
    this.closeError.dispose();
    super.dispose();
  }

  /// Try to close the channel after the user confirms.
  Future<void> onConfirm() async {
    // Don't allow submission while a channel close is pending.
    if (this.isPending.value) return;

    // Start loading and reset any errors.
    this.isPending.value = true;
    this.closeError.value = null;

    // Close the channel.
    final channelId = this.widget.channelId;
    final req = CloseChannelRequest(channelId: channelId);
    final result = await Result.tryFfiAsync(
      () => this.widget.app.closeChannel(req: req),
    );

    if (!this.mounted) return;

    final CloseChannelFlowResult flowResult;
    switch (result) {
      case Ok():
        flowResult = CloseChannelFlowResult(channelId: channelId);
        info("CloseChannelConfirmPage: success: $flowResult");
      case Err(:final err):
        error("CloseChannelConfirmPage: error: ${err.message}");
        this.isPending.value = false;
        this.closeError.value = ErrorMessage(
          title: "Failed to close channel",
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
          const HeadingText(text: "Confirm channel close"),
          const SubheadingText(
            text: "Moving Lightning channel funds back on-chain.",
          ),

          const SizedBox(height: Space.s700),

          // Show the "itemized" receipt for this channel close.
          ValueListenableBuilder(
            valueListenable: this.widget.fiatRate,
            builder: (context, fiatRate, child) {
              final channelSats = this.widget.channelOurBalanceSats;
              final channelFiat = FiatAmount.maybeFromSats(
                fiatRate,
                channelSats,
              );

              final feeSats = this.widget.preflight.feeEstimateSats;
              final feeFiat = FiatAmount.maybeFromSats(fiatRate, feeSats);

              final onchainSats = max(0, channelSats - feeSats);
              final onchainFiat = FiatAmount.maybeFromSats(
                fiatRate,
                onchainSats,
              );

              return Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  // In: On-chain balance
                  ItemizedAmountRow(
                    fiatAmount: channelFiat,
                    satsAmount: channelSats,
                    title: "Lightning",
                    // subtitle: "Channel deposit",
                    subtitle: "",
                    icon: const ListIcon.lightning(),
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
                    fiatAmount: onchainFiat,
                    satsAmount: onchainSats,
                    title: "On-chain",
                    subtitle: "",
                    // subtitle: "New channel",
                    icon: const ListIcon.lightning(),
                  ),
                ],
              );
            },
          ),

          const SizedBox(height: Space.s700),

          // Error closing channel
          ValueListenableBuilder(
            valueListenable: this.closeError,
            builder: (_context, errorMessage, _widget) =>
                ErrorMessageSection(errorMessage),
          ),
        ],

        // Close channel ->
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.isPending,
            builder: (_context, estimatingFee, _widget) => AnimatedFillButton(
              label: const Text("Close channel"),
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
