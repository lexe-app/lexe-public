import 'package:app_rs_dart/ffi/api.dart'
    show OnchainDescriptors, OnchainDescriptorsInfo;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/material.dart';
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        InfoCard,
        InfoRow,
        LxBackButton,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/show_qr.dart' show InteractiveQrImage;
import 'package:lexeapp/share.dart' show LxShare;
import 'package:lexeapp/style.dart' show LxColors, LxIcons, Space, LxTheme;

/// Shows the user's on-chain wallet descriptors, so they can import them into
/// a watch-only wallet or on-chain transaction tracker.
class OnchainDescriptorsPage extends StatefulWidget {
  const OnchainDescriptorsPage({super.key, required this.app});

  final AppHandle app;

  @override
  State<OnchainDescriptorsPage> createState() => _OnchainDescriptorsPageState();
}

class _OnchainDescriptorsPageState extends State<OnchainDescriptorsPage> {
  final ValueNotifier<FfiResult<OnchainDescriptorsInfo>?> descriptors =
      ValueNotifier(null);

  @override
  void dispose() {
    this.descriptors.dispose();
    super.dispose();
  }

  @override
  void initState() {
    super.initState();
    this.fetch();
  }

  Future<void> fetch() async {
    final result = await Result.tryFfiAsync(this.widget.app.onchainDescriptors);
    if (!this.mounted) return;
    this.descriptors.value = result;
  }

  @override
  Widget build(BuildContext context) {
    const cardPad = Space.s300;

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        padding: const EdgeInsets.symmetric(horizontal: Space.s600 - cardPad),
        body: [
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: cardPad),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                HeadingText(text: "Wallet descriptors"),
                SubheadingText(
                  text:
                      "Track your on-chain wallet funds in an external coin "
                      "tracker",
                ),
                SizedBox(height: Space.s400),
              ],
            ),
          ),

          ValueListenableBuilder(
            valueListenable: this.descriptors,
            builder: (_context, descriptors, _widget) => switch (descriptors) {
              // Loading
              null => Padding(
                padding: EdgeInsets.only(top: Space.s1000),
                child: Center(
                  child: SizedBox.square(
                    dimension: 20.0,
                    child: CircularProgressIndicator(
                      strokeWidth: 2.0,
                      color: LxTheme.resolve(context, LxColors.fgTertiary),
                    ),
                  ),
                ),
              ),

              Err(:final err) => Padding(
                padding: const EdgeInsets.symmetric(horizontal: cardPad),
                child: ErrorMessageSection(
                  ErrorMessage(
                    title: "Failed to fetch wallet descriptors",
                    message: err.message,
                  ),
                ),
              ),

              Ok(:final ok) => Column(
                children: [
                  // Our standard on-chain wallet descriptors
                  DescriptorsCard(
                    // Hide redundant header for 99% of users who don't have a
                    // legacy on-chain wallet.
                    header: ok.legacy != null ? "On-chain wallet" : null,
                    description:
                        "If your tool doesn't accept the combined Descriptor, "
                        "import Receive and Change separately.",
                    descriptors: ok.current,
                  ),

                  // Legacy on-chain wallet descriptors for nodes created <=
                  // node-v0.9.2.
                  if (ok.legacy case final legacy?)
                    Padding(
                      padding: const EdgeInsets.only(top: Space.s400),
                      child: DescriptorsCard(
                        header: "Legacy on-chain wallet",
                        description:
                            "Your node's older on-chain wallet. Import it too to "
                            "see your full history.",
                        descriptors: legacy,
                      ),
                    ),

                  const SizedBox(height: Space.s400),
                ],
              ),
            },
          ),
        ],
      ),
    );
  }
}

/// An [InfoCard] with a QR code of the multipath descriptor, followed by all
/// the descriptors in [descriptors].
class DescriptorsCard extends StatelessWidget {
  const DescriptorsCard({
    super.key,
    required this.header,
    required this.description,
    required this.descriptors,
  });

  final String? header;
  final String? description;
  final OnchainDescriptors descriptors;

  /// Copy the descriptors.
  Future<void> onTapCopy(BuildContext context) async {
    await LxClipboard.copyTextWithFeedback(
      context,
      this.descriptors.multipathDescriptor,
    );
  }

  /// Share the descriptors.
  Future<void> onTapShare(BuildContext context) async {
    await LxShare.sharePlaintext(context, this.descriptors.multipathDescriptor);
  }

  @override
  Widget build(BuildContext context) {
    final header = this.header;
    final description = this.description;
    final descriptors = this.descriptors;

    return Column(
      children: [
        // Main card. Multi-path descriptor + QR code
        InfoCard(
          header: header != null ? Text(header) : null,
          description: description != null ? Text(description) : null,
          children: [
            // QR code
            Padding(
              padding: const EdgeInsets.fromLTRB(
                Space.s300,
                Space.s200,
                Space.s300,
                Space.s300,
              ),
              child: Container(
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(6.0),
                ),
                clipBehavior: Clip.antiAlias,
                child: LayoutBuilder(
                  builder: (context, constraints) => InteractiveQrImage(
                    value: descriptors.multipathDescriptor,
                    dimension: constraints.maxWidth,
                  ),
                ),
              ),
            ),

            // Multi-path descriptor
            InfoRow(
              label: "Descriptor",
              value: descriptors.multipathDescriptor,
            ),
          ],
        ),

        // Copy + Share buttons
        Padding(
          padding: const EdgeInsets.only(top: Space.s200, bottom: Space.s300),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              // Copy descriptor
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: Space.s200),
                child: FilledButton(
                  onPressed: () => this.onTapCopy(context),
                  child: const Icon(LxIcons.copy),
                ),
              ),

              // Share descriptor
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: Space.s200),
                child: Builder(
                  // Use an extra Builder layer so the `sharePositionOrigin`
                  // is around just this button.
                  builder: (context) => FilledButton(
                    onPressed: () => this.onTapShare(context),
                    child: const Icon(LxIcons.share),
                  ),
                ),
              ),
            ],
          ),
        ),

        // Other formats for compatibility
        InfoCard(
          header: Text("Other formats"),
          children: [
            InfoRow(
              label: "Account zpub (native SegWit)",
              value: descriptors.accountZpub,
            ),
            InfoRow(
              label: "Receive descriptor",
              value: descriptors.externalDescriptor,
            ),
            InfoRow(
              label: "Change descriptor",
              value: descriptors.internalDescriptor,
            ),
          ],
        ),
      ],
    );
  }
}
