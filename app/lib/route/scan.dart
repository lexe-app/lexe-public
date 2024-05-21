// Page for scanning QR codes / barcodes

import 'package:flutter/material.dart';
import 'package:flutter_zxing/flutter_zxing.dart'
    show Code, FixedScannerOverlay, ReaderWidget;
import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/bindings_generated_api.dart'
    show Network, PaymentMethod;
// import 'package:lexeapp/bindings_generated_api_ext.dart' show PaymentMethodExt;

import 'package:lexeapp/components.dart' show LxCloseButton;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/style.dart' show LxColors, LxRadius, LxTheme, Space;

class ScanPage extends StatefulWidget {
  const ScanPage({super.key, required this.network});

  final Network network;

  @override
  State<ScanPage> createState() => _ScanPageState();
}

class _ScanPageState extends State<ScanPage> {
  // TODO(phlip9): in the future, once resolving a code actually requires
  // network requests, let's show a loading spinner when this is true.
  bool isProcessing = false;

  Future<void> onScan(final Code code) async {
    final text = code.text;

    // flutter_zxing doesn't call our callback w/ invalid codes, but `Code`
    // stuffs both valid/error cases in one struct...
    if (text == null) return;

    // Skip any new results if we're still processing a prev. scanned QR code.
    if (this.isProcessing) return;

    this.isProcessing = true;
    try {
      info("Scanned code: \"$text\"");

      // Try to resolve the QR code into a single, "best" payment method. "Best"
      // currently means just unconditionally prefer BOLT11 invoices, but should
      // smarter in the future.
      final result = await Result.tryFfiAsync(() => api.paymentUriResolveBest(
            network: this.widget.network,
            uriStr: text,
          ));

      if (!this.mounted) return;

      final PaymentMethod paymentMethod;
      switch (result) {
        case Ok(:final ok):
          paymentMethod = ok;
        case Err(:final err):
          warn("Failed to resolve QR code: $err");
          // TODO(phlip9): could probably use a better error display
          ScaffoldMessenger.of(this.context).showSnackBar(SnackBar(
            content: Text(err.message),
          ));
          return;
      }

      info("Scanned QR with best payment method: $paymentMethod");

      // final int? amountSats = paymentMethod.amountSats();
      //
      // if (amountSats == null) {
      //
      // }

      // if (amountSats == null) {
      //
      // }

      // if the paymentMethod needs an amount, then jump to the
      // `SendPaymentAmountPage`. otherwise, if the payment already has a fixed
      // amount, we need to pre-flight the payment (look for LN route, estimate
      // fees) and jump to the `SendPaymentConfirmPage`.
    } finally {
      this.isProcessing = false;
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
        leading: const LxCloseButton(),

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
