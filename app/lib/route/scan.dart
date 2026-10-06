// Page for scanning QR codes / barcodes

import 'package:app_rs_dart/ffi/types.dart'
    show
        AuthMethod,
        AuthMethod_LexeConnect,
        BestPaymentUriMethods,
        ClaimMethod,
        PaymentMethod;
import 'package:flutter/material.dart';
import 'package:flutter_zxing/flutter_zxing.dart'
    show Code, FixedScannerOverlay, Format, ReaderWidget;
import 'package:lexeapp/components.dart'
    show LxBackButton, LxCloseButton, LxCloseButtonKind, showModalAsyncFlow;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/claim/page.dart' show ClaimPaymentPage;
import 'package:lexeapp/route/claim/state.dart'
    show ClaimFlowResult, ClaimState;
import 'package:lexeapp/route/lexe_connect.dart'
    show LexeConnectFlowResult, LexeConnectPage;
import 'package:lexeapp/route/send/page.dart' show SendPaymentPage;
import 'package:lexeapp/route/send/state.dart' show SendFlowResult, SendState;
import 'package:lexeapp/route/uri/page.dart'
    show SendOrClaimChoiceSheet, UriChoice;
import 'package:lexeapp/route/uri/state.dart';
import 'package:lexeapp/style.dart' show LxColors, LxRadius, LxTheme, Space;

class ScanPage extends StatefulWidget {
  const ScanPage({super.key, required this.uriFlowCtx});

  final NeedUriState uriFlowCtx;

  @override
  State<ScanPage> createState() => _ScanPageState();
}

class _ScanPageState extends State<ScanPage> {
  Future<void> onScan(String text) async {
    // Try resolving the payment URI to "best" payment and claim methods
    // TODO(nicole): 2x showModalAsyncFlow causes a flicker effect; need to fix
    final resolveResult = await showModalAsyncFlow(
      context: this.context,
      future: this.widget.uriFlowCtx.resolve(text),
      errorBuilder: (context, err) => AlertDialog(
        title: const Text("Issue with resolving URI"),
        content: Text(err),
        scrollable: true,
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text("Close"),
          ),
        ],
      ),
    );

    // User canceled
    if (!this.mounted || resolveResult == null) return;

    // Check the resolve result
    final BestPaymentUriMethods best;
    switch (resolveResult) {
      case Ok(:final ok):
        best = ok;
      case Err(:final err):
        error("ScanPage: URI resolution error: $err");
        return;
    }

    // Branch accordingly
    final UriFlowResult? flowResult;
    switch ((best.paymentMethod, best.claimMethod, best.authMethod)) {
      case (final paymentMethod?, final claimMethod?, _):
        final UriChoice? userChoice = await SendOrClaimChoiceSheet.show(
          context: this.context,
          paymentMethod: paymentMethod,
          claimMethod: claimMethod,
        );
        if (!this.mounted || userChoice == null) return;
        flowResult = switch (userChoice) {
          UriChoice.send => await this._handlePaymentMethod(paymentMethod),
          UriChoice.claim => await this._handleClaimMethod(claimMethod),
        };

      case (final paymentMethod?, _, _):
        flowResult = await this._handlePaymentMethod(paymentMethod);

      case (_, final claimMethod?, _):
        flowResult = await this._handleClaimMethod(claimMethod);

      case (_, _, final authMethod?):
        flowResult = await this._handleAuthMethod(authMethod);

      case _:
        error(
          "ScanPage: Failed to resolve scanned URI -- this is a bug, please report.",
        );
        return;
    }

    if (!this.mounted || flowResult == null) return;

    // Successfully processed payment -- return result to parent page.
    await Navigator.of(this.context).maybePop(flowResult);
  }

  Future<UriFlowResult?> _handlePaymentMethod(
    PaymentMethod paymentMethod,
  ) async {
    // Enter send flow; try immediately preflighting, showing a spinner
    // during the wait and an error modal if something goes wrong.
    // TODO(nicole): 2x showModalAsyncFlow causes a flicker effect; need to fix
    final result = await showModalAsyncFlow(
      context: this.context,
      future: this.widget.uriFlowCtx.enterSendFlow(paymentMethod),
      // TODO(phlip9): error messages need work
      errorBuilder: (context, err) => AlertDialog(
        title: const Text("Issue with preflighting payment"),
        content: Text(err),
        scrollable: true,
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text("Close"),
          ),
        ],
      ),
    );
    if (!this.mounted) return null;

    // User canceled
    if (result == null) {
      return null;
    }

    // Check the results, or show an error on the page.
    final SendState sendCtx;
    switch (result) {
      case Ok(:final ok):
        sendCtx = ok;
      case Err(:final err):
        error("ScanPage: preflight error: $err");
        return null;
    }

    // If we still need an amount, then we have to collect that first.
    // Otherwise, a successful payment preflight means we can go directly to the
    // confirm page.
    final SendFlowResult? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) => SendPaymentPage(sendCtx: sendCtx, startNewFlow: false),
      ),
    );

    info(
      "SendPaymentNeedUriPage: flow result: $flowResult, mounted: ${this.mounted}",
    );
    if (!this.mounted || flowResult == null) return null;

    return UriFlowResult_Send(flowResult);
  }

  Future<UriFlowResult?> _handleClaimMethod(ClaimMethod claimMethod) async {
    final ClaimState claimCtx = this.widget.uriFlowCtx.enterClaimFlow(
      claimMethod,
    );
    final ClaimFlowResult? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) =>
            ClaimPaymentPage(claimCtx: claimCtx, startNewFlow: false),
      ),
    );
    if (!this.mounted || flowResult == null) return null;

    return UriFlowResult_Claim(flowResult);
  }

  Future<UriFlowResult?> _handleAuthMethod(AuthMethod authMethod) async {
    final request = switch (authMethod) {
      AuthMethod_LexeConnect(:final field0) => field0,
    };
    final LexeConnectFlowResult? flowResult = await Navigator.of(this.context)
        .push(
          MaterialPageRoute(
            builder: (_) => LexeConnectPage(
              app: this.widget.uriFlowCtx.app,
              request: request,
            ),
          ),
        );
    info("ScanPage (auth): flowResult: $flowResult, mounted: ${this.mounted}");
    if (!this.mounted || flowResult == null) return null;

    return const UriFlowResult_Auth();
  }

  @override
  Widget build(BuildContext context) => QrScanner(onScan: this.onScan);
}

/// A full-screen camera QR code scanner. Passes each scanned code's text to
/// [onScan], ignoring new codes until it completes.
class QrScanner extends StatefulWidget {
  const QrScanner({super.key, required this.onScan});

  final Future<void> Function(String text) onScan;

  @override
  State<QrScanner> createState() => _QrScannerState();
}

class _QrScannerState extends State<QrScanner> {
  bool isProcessing = false;

  Future<void> onScan(Code code) async {
    final text = code.text;

    // flutter_zxing doesn't call our callback w/ invalid codes, but `Code`
    // stuffs both valid/error cases in one struct...
    if (text == null || this.isProcessing) return;

    this.isProcessing = true;
    try {
      await this.widget.onScan(text);
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
        leading: const LxBackButton(isLeading: true),
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

        // Bottom "action" buttons, like "open from gallery".
        showFlashlight: false,
        showToggleCamera: false,
        cropPercent: 0.50,
        actionButtonsAlignment: Alignment.bottomCenter,
        actionButtonsPadding: const EdgeInsets.all(Space.s600),

        // Also try scanning with inverted colors (e.g. white QR on black bg).
        tryInverted: true,

        // Show this while the camera is still loading.
        loading: const DecoratedBox(
          decoration: BoxDecoration(color: LxColors.foreground),
          child: Center(),
        ),

        // The partially transparent overlay outside of the main scan region.
        scannerOverlay: const FixedScannerOverlay(
          borderColor: LxColors.grey975,
          // grey900 x clear700
          overlayColor: Color(0xb2eff3f5),
          borderRadius: LxRadius.r400,
          borderLength: 120.0,
          borderWidth: 8.0,
          cutOutSize: 240.0,
        ),

        // Code scanner parameters
        // Only scan QR codes -- this makes scanning faster than `Format.any`.
        codeFormat: Format.qrCode,
      ),
    );
  }
}
