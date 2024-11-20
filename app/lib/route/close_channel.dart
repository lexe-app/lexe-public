/// UI flow for users to close one of their open channels with the Lexe LSP.
library;

import 'package:app_rs_dart/ffi/api.dart'
    show CloseChannelRequest, FiatRate, PreflightCloseChannelResponse;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        HeadingText,
        LxBackButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SubheadingText,
        showModalAsyncFlow;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/channels.dart'
    show
        Channel,
        ChannelsList,
        ChannelsListEntry,
        ChannelsPartyChip,
        channelsListEntryHeight;
import 'package:lexeapp/style.dart' show LxColors, Space;

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
  @override
  void dispose() {
    super.dispose();
  }

  Future<void> onChannelSelected(final Channel channel) async {
    info("CloseChannelChoosePage: selected channel: ${channel.channelId}, "
        "our sats: ${channel.ourBalanceSats}");

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
    final CloseChannelFlowResult? flowResult =
        await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (context) => CloseChannelConfirmPage(
          app: this.widget.app,
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
                    text: "Move funds in a Lightning channel back on-chain."),
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
    required this.preflight,
  });

  final AppHandle app;
  final PreflightCloseChannelResponse preflight;

  @override
  State<CloseChannelConfirmPage> createState() =>
      _CloseChannelConfirmPageState();
}

class _CloseChannelConfirmPageState extends State<CloseChannelConfirmPage> {
  @override
  Widget build(BuildContext context) {
    return const Placeholder();
  }
}
