// Page for scanning QR codes / barcodes

import 'package:flutter/material.dart';
import 'package:flutter_zxing/flutter_zxing.dart'
    show Code, FixedScannerOverlay, ReaderWidget;
import 'package:lexeapp/components.dart'
    show LxBackButton, LxCloseButton, LxCloseButtonKind;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/send/page.dart' show SendPaymentPage;
import 'package:lexeapp/route/send/state.dart'
    show SendContext, SendContext_NeedUri;
import 'package:lexeapp/style.dart' show LxColors, LxRadius, LxTheme, Space;

class ScanPage extends StatefulWidget {
  const ScanPage({super.key, required this.sendCtx});

  final SendContext_NeedUri sendCtx;

  @override
  State<ScanPage> createState() => _ScanPageState();
}

class _ScanPageState extends State<ScanPage> {
  // TODO(phlip9): show a spinner while processing
  ValueNotifier<bool> isProcessing = ValueNotifier(false);

  @override
  void dispose() {
    this.isProcessing.dispose();

    super.dispose();
  }

  Future<void> onScan(final Code code) async {
    final text = code.text;

    // flutter_zxing doesn't call our callback w/ invalid codes, but `Code`
    // stuffs both valid/error cases in one struct...
    if (text == null) return;

    // Skip any new results if we're still processing a prev. scanned QR code.
    if (this.isProcessing.value) return;

    // Start loading animation
    this.isProcessing.value = true;

    // Try resolving the payment URI to a "best" payment method. Then try
    // immediately preflighting it if it already has an associated amount.
    final result = await this.widget.sendCtx.resolveAndMaybePreflight(text);
    if (!this.mounted) return;

    // Stop loading animation
    this.isProcessing.value = false;

    // Check the results, or show an error on the page.
    final SendContext sendCtx;
    switch (result) {
      case Ok(:final ok):
        sendCtx = ok;
      case Err(:final err):
        // TODO(phlip9): could probably use a better error display
        if (err != null) {
          ScaffoldMessenger.of(this.context).showSnackBar(SnackBar(
            content: Text(err),
          ));
        }
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
      extendBodyBehindAppBar: true,
      appBar: AppBar(
        // transparent bg header
        backgroundColor: LxColors.clearB0,
        scrolledUnderElevation: 0.0,
        surfaceTintColor: LxColors.clearB0,

        // X - quit scanning
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.appBarTrailingPadding),
        ],

        // * Make the top status bar transparent, so the whole screen includes
        //   the camera view.
        // * Make the bottom nav thing `foreground` instead of black.
        systemOverlayStyle: LxTheme.systemOverlayStyleLight.copyWith(
          statusBarColor: LxColors.clearW0,
          systemNavigationBarColor: LxColors.foreground,
          systemNavigationBarDividerColor: LxColors.foreground,
        ),
      ),
      // TODO(phlip9): just show a file picker or something for non-mobile
      //               OS like macOS, linux, windows.
      // We're waiting on the flutter `camera` pkg to support desktop OS's.
      body: ReaderWidget(
        onScan: this.onScan,
        showFlashlight: false,
        showToggleCamera: false,
        cropPercent: 0.50,
        actionButtonsAlignment: Alignment.bottomCenter,
        actionButtonsPadding: const EdgeInsets.all(Space.s600),
        loading: const DecoratedBox(
          decoration: BoxDecoration(color: LxColors.foreground),
          child: Center(),
        ),
        scannerOverlay: const FixedScannerOverlay(
          borderColor: LxColors.grey975,
          // grey900 x clear700
          overlayColor: Color(0xb2eff3f5),
          borderRadius: LxRadius.r400,
          borderLength: 120.0,
          borderWidth: 8.0,
          cutOutSize: 240.0,
        ),
      ),
    );
  }
}
