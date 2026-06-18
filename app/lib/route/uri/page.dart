import 'package:app_rs_dart/ffi/types.dart'
    show
        ClaimMethod,
        ClaimMethod_LnurlWithdraw,
        PaymentMethod,
        PaymentMethod_Invoice,
        PaymentMethod_LnurlPay,
        PaymentMethod_Offer,
        PaymentMethod_Onchain;
import 'package:flutter/material.dart';
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxBackButton,
        LxFilledButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SheetDragHandle,
        StackedButton,
        baseInputDecoration;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/claim/page.dart' show ClaimPaymentPage;
import 'package:lexeapp/route/claim/state.dart' show ClaimFlowResult;
import 'package:lexeapp/route/scan.dart' show ScanPage;
import 'package:lexeapp/route/send/page.dart' show SendPaymentPage;
import 'package:lexeapp/route/send/state.dart' show SendFlowResult, SendState;
import 'package:lexeapp/route/uri/state.dart';
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;

class NeedUriPage extends StatelessWidget {
  const NeedUriPage({
    super.key,
    required this.uriFlowCtx,
    required this.startNewFlow,
    required this.expectClaimFlow,
  });

  final NeedUriState uriFlowCtx;
  final bool startNewFlow;
  final bool expectClaimFlow;

  @override
  Widget build(BuildContext context) => (this.startNewFlow)
      ? MultistepFlow<UriFlowResult>(
          builder: (_) => NeedUriPageInner(
            uriFlowCtx: this.uriFlowCtx,
            expectClaimFlow: this.expectClaimFlow,
          ),
        )
      : NeedUriPageInner(
          uriFlowCtx: this.uriFlowCtx,
          expectClaimFlow: this.expectClaimFlow,
        );
}

/// If the user is just hitting the "Send" button with no extra context, then we
/// need to collect a [PaymentUri] of some kind (bitcoin address, LN invoice,
/// etc...)
class NeedUriPageInner extends StatefulWidget {
  const NeedUriPageInner({
    super.key,
    required this.uriFlowCtx,
    required this.expectClaimFlow,
  });

  final NeedUriState uriFlowCtx;
  final bool expectClaimFlow;

  @override
  State<NeedUriPageInner> createState() => _NeedUriPageInnerState();
}

class _NeedUriPageInnerState extends State<NeedUriPageInner> {
  final GlobalKey<FormFieldState<String>> paymentUriFieldKey = GlobalKey();

  final ValueNotifier<bool> isPending = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.errorMessage.dispose();
    this.isPending.dispose();

    super.dispose();
  }

  String header() =>
      this.widget.expectClaimFlow ? "Who's paying us?" : "Who are we paying?";

  String hintText() => this.widget.expectClaimFlow
      ? "lnurl1.. lnurlw.."
      : "bc1.. lnbc1.. bitcoin:..";

  Future<void> onScanPressed() async {
    info("pressed QR scan button");

    final UriFlowResult? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_context) => ScanPage(uriFlowCtx: this.widget.uriFlowCtx),
      ),
    );
    if (!this.mounted || flowResult == null) return;

    // Successfully sent payment -- return result to parent page.
    await Navigator.of(this.context).maybePop(flowResult);
  }

  Future<UriFlowResult?> _handlePaymentMethod(
    PaymentMethod paymentMethod,
  ) async {
    final result = await this.widget.uriFlowCtx.enterSendFlow(paymentMethod);
    if (!this.mounted) return null;

    // Check the results, or show an error on the page.
    final SendState sendCtx;
    switch (result) {
      case Ok(:final ok):
        sendCtx = ok;
      case Err(:final err):
        this.errorMessage.value = ErrorMessage(message: err);
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
      "NeedUriPage (send): flowResult: $flowResult, mounted: ${this.mounted}",
    );
    if (!this.mounted || flowResult == null) return null;

    return UriFlowResult_Send(flowResult);
  }

  Future<UriFlowResult?> _handleClaimMethod(ClaimMethod claimMethod) async {
    final claimCtx = this.widget.uriFlowCtx.enterClaimFlow(claimMethod);

    final ClaimFlowResult? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) =>
            ClaimPaymentPage(claimCtx: claimCtx, startNewFlow: true),
      ),
    );
    info(
      "NeedUriPage (claim): flowResult: $flowResult, mounted: ${this.mounted}",
    );
    if (!this.mounted || flowResult == null) return null;

    return UriFlowResult_Claim(flowResult);
  }

  Future<void> onNext() async {
    // Hide error message
    this.errorMessage.value = null;

    // Validate the payment URI field.
    final fieldState = this.paymentUriFieldKey.currentState!;
    if (!fieldState.validate()) return;

    final uriStr = fieldState.value;

    // Don't bother showing an error if the input is empty.
    if (uriStr == null || uriStr.isEmpty) return;

    // Start loading animation
    this.isPending.value = true;

    // Try resolving the payment URI to the "best" payment/claim methods.
    final result = await this.widget.uriFlowCtx.resolve(uriStr);
    if (!this.mounted) return;

    // Check the results
    final PaymentMethod? paymentMethod;
    final ClaimMethod? claimMethod;
    switch (result) {
      case Ok(:final ok):
        paymentMethod = ok.$1;
        claimMethod = ok.$2;
      case Err(:final err):
        this.isPending.value = false;
        this.errorMessage.value = ErrorMessage(message: err);
        return;
    }

    // Branch accordingly
    final UriFlowResult? flowResult;
    switch ((paymentMethod, claimMethod)) {
      case (final paymentMethod?, final claimMethod?):
        final UriChoice? userChoice = await SendOrClaimChoiceSheet.show(
          context: this.context,
          paymentMethod: paymentMethod,
          claimMethod: claimMethod,
        );
        if (userChoice == null) {
          this.isPending.value = false;
          return;
        }
        flowResult = switch (userChoice) {
          UriChoice.send => await this._handlePaymentMethod(paymentMethod),
          UriChoice.claim => await this._handleClaimMethod(claimMethod),
        };

      case (final paymentMethod?, _):
        flowResult = await this._handlePaymentMethod(paymentMethod);

      case (_, final claimMethod?):
        flowResult = await this._handleClaimMethod(claimMethod);

      case _:
        this.isPending.value = false;
        this.errorMessage.value = ErrorMessage(
          message: "Failed to resolve the URI. This is a bug -- please report.",
        );
        return;
    }
    this.isPending.value = false;
    if (!this.mounted || flowResult == null) return;

    // Successfully processed payment -- return result to parent page.
    await Navigator.of(this.context).maybePop(flowResult);
  }

  /// Called when the user taps the paste button
  Future<void> onPaste() async {
    // Get clipboard text
    final text = await LxClipboard.getText();
    if (!this.mounted) return;
    if (text == null || text.isEmpty) return;

    // Set payment URI field
    this.paymentUriFieldKey.currentState?.didChange(text);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
        actions: [
          IconButton(
            onPressed: this.onScanPressed,
            icon: const Icon(LxIcons.scanDetailed),
          ),
          const SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          HeadingText(text: this.header()),
          const SizedBox(height: Space.s300),

          // Enter payment URI text field
          TextFormField(
            key: this.paymentUriFieldKey,
            autofocus: true,
            maxLines: 1,
            // `visiblePassword` gives ready access to letters + numbers
            keyboardType: TextInputType.visiblePassword,
            textDirection: TextDirection.ltr,
            textInputAction: TextInputAction.next,
            onEditingComplete: this.onNext,
            decoration: baseInputDecoration.copyWith(hintText: this.hintText()),
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size700,
              fontVariations: [Fonts.weightMedium],
              // Use unambiguous character alternatives (0OIl1) to avoid
              // confusion in the unfortunate event that a user has to
              // manually type in an address.
              fontFeatures: [Fonts.featDisambugation],
              letterSpacing: -0.5,
              // Add a bit of extra height to make the text area look nicer.
              height: 1.3,
            ),
          ),

          const SizedBox(height: Space.s800),

          // Error parsing, resolving, and/or preflighting payment
          ValueListenableBuilder(
            valueListenable: this.errorMessage,
            builder: (_context, errorMessage, _widget) =>
                ErrorMessageSection(errorMessage),
          ),
        ],

        // Bottom buttons (paste, next ->)
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              Row(
                children: [
                  // Paste
                  Expanded(
                    child: GestureDetector(
                      behavior: HitTestBehavior.translucent,
                      onTap: this.onPaste,
                      child: StackedButton(
                        button: LxFilledButton(
                          onTap: this.onPaste,
                          icon: const Center(child: Icon(LxIcons.paste)),
                        ),
                        label: "Paste",
                      ),
                    ),
                  ),
                  const SizedBox(width: Space.s200),
                  // Next ->
                  Expanded(
                    child: ValueListenableBuilder(
                      valueListenable: this.isPending,
                      builder: (_context, isPending, _widget) =>
                          GestureDetector(
                            behavior: HitTestBehavior.translucent,
                            onTap: !isPending ? this.onNext : null,
                            child: StackedButton(
                              button: AnimatedFillButton(
                                label: const Icon(LxIcons.next),
                                icon: const Icon(null),
                                onTap: this.onNext,
                                loading: isPending,
                              ),
                              label: "Next",
                            ),
                          ),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

enum UriChoice { send, claim }

/// A bottom sheet that asks the user to choose between sending or claiming,
/// and pops a [UriChoice] accordingly.
///
/// Currently, only shown when the user scans an LNURL-withdraw with an embedded
/// LNURL-pay via the payLink field.
class SendOrClaimChoiceSheet extends StatelessWidget {
  const SendOrClaimChoiceSheet({
    super.key,
    required this.paymentMethod,
    required this.claimMethod,
  });

  final PaymentMethod paymentMethod;
  final ClaimMethod claimMethod;

  /// Show the sheet and return the user's choice
  static Future<UriChoice?> show({
    required BuildContext context,
    required PaymentMethod paymentMethod,
    required ClaimMethod claimMethod,
  }) => showModalBottomSheet(
    backgroundColor: LxColors.background,
    enableDrag: true,
    isScrollControlled: true,
    isDismissible: true,
    context: context,
    builder: (context) => SendOrClaimChoiceSheet(
      paymentMethod: paymentMethod,
      claimMethod: claimMethod,
    ),
  );

  @override
  Widget build(BuildContext context) {
    final sendKind = switch (this.paymentMethod) {
      PaymentMethod_Onchain() => "onchain",
      PaymentMethod_Invoice() => "invoice",
      PaymentMethod_LnurlPay() => "LNURL",
      PaymentMethod_Offer() => "offer",
    };
    final claimKind = switch (this.claimMethod) {
      ClaimMethod_LnurlWithdraw() => "LNURL",
    };

    // TODO(nicole): could b cuter ):
    return Padding(
      padding: const EdgeInsets.only(
        left: Space.s400,
        right: Space.s400,
        bottom: Space.s700,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.center,
        spacing: Space.s400,
        children: [
          Padding(
            padding: const EdgeInsets.only(bottom: Space.s500),
            child: const SheetDragHandle(),
          ),
          LxFilledButton(
            onTap: () => Navigator.of(context).pop(UriChoice.send),
            style: FilledButton.styleFrom(
              backgroundColor: LxColors.grey1000,
              foregroundColor: LxColors.foreground,
              iconColor: LxColors.foreground,
              fixedSize: const Size(300.0, Space.s800),
            ),
            label: Text("Pay via $sendKind"),
            icon: const Icon(LxIcons.outbound),
          ),
          LxFilledButton(
            onTap: () => Navigator.of(context).pop(UriChoice.claim),
            style: FilledButton.styleFrom(
              backgroundColor: LxColors.grey1000,
              foregroundColor: LxColors.foreground,
              iconColor: LxColors.foreground,
              fixedSize: const Size(300.0, Space.s800),
            ),
            label: Text("Withdraw via $claimKind"),
            icon: const Icon(LxIcons.inbound),
          ),
          // InfoCard(
          //   children: [
          //     Row(
          //       children: [
          //         Container(
          //           width: Space.s800,
          //           height: Space.s800,
          //           decoration: BoxDecoration(
          //             color: LxColors.foreground,
          //             borderRadius: BorderRadius.circular(LxRadius.r300),
          //             // shape: BoxShape.circle,
          //           ),
          //           child: const Icon(LxIcons.inbound, color: LxColors.background),
          //         ),
          //         Text("Receive via LNURL")
          //       ]
          //     ),
          //   ],
          // ),
        ],
      ),
    );
  }
}
