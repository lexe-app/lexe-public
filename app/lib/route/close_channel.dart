/// UI flow for users to close one of their open channels with the Lexe LSP.
library;

import 'package:app_rs_dart/ffi/api.dart' show FiatRate;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxBackButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/logger.dart';
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
  final ValueNotifier<ErrorMessage?> estimateFeeError = ValueNotifier(null);
  final ValueNotifier<bool> estimatingFee = ValueNotifier(false);

  @override
  void dispose() {
    this.estimatingFee.dispose();
    this.estimateFeeError.dispose();
    super.dispose();
  }

  Future<void> onChannelSelected(final Channel channel) async {
    info("CloseChannelChoosePage: selected channel: ${channel.channelId}, "
        "our sats: ${channel.ourBalanceSats}");
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
          SliverToBoxAdapter(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const HeadingText(text: "Close Lightning channel"),
                const SubheadingText(
                    text: "Move funds in a Lightning channel back on-chain."),
                const SizedBox(height: Space.s500),

                // Error fetching fee estimate
                ValueListenableBuilder(
                  valueListenable: this.estimateFeeError,
                  builder: (_context, errorMessage, _widget) =>
                      ErrorMessageSection(errorMessage),
                ),
                const SizedBox(height: Space.s400),

                // You/Lexe LSP channels heading
                const Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    ChannelsPartyChip(name: "You"),
                    ChannelsPartyChip(name: "Lexe LSP"),
                  ],
                ),
                const SizedBox(height: Space.s200),
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
  const CloseChannelConfirmPage({super.key});

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
