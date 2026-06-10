import 'package:app_rs_dart/ffi/types.dart' show ClaimMethod, PaymentMethod;
import 'package:flutter/material.dart';
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        StackedButton,
        baseInputDecoration;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/scan.dart' show ScanPage;
import 'package:lexeapp/route/send/page.dart' show SendPaymentPage;
import 'package:lexeapp/route/send/state.dart' show SendFlowResult, SendState;
import 'package:lexeapp/route/uri/state.dart';
import 'package:lexeapp/style.dart' show Fonts, LxIcons, Space;

class NeedUriPage extends StatelessWidget {
  const NeedUriPage({
    super.key,
    required this.uriFlowCtx,
    required this.startNewFlow,
  });

  final NeedUriState uriFlowCtx;
  final bool startNewFlow;

  @override
  Widget build(BuildContext context) => (this.startNewFlow)
      ? MultistepFlow<UriFlowResult>(
          builder: (_) => NeedUriPageInner(uriFlowCtx: this.uriFlowCtx),
        )
      : NeedUriPageInner(uriFlowCtx: this.uriFlowCtx);
}

/// If the user is just hitting the "Send" button with no extra context, then we
/// need to collect a [PaymentUri] of some kind (bitcoin address, LN invoice,
/// etc...)
class NeedUriPageInner extends StatefulWidget {
  const NeedUriPageInner({super.key, required this.uriFlowCtx});

  final NeedUriState uriFlowCtx;

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
    switch ((paymentMethod, claimMethod)) {
      // We can enter the send flow if we resolved a payment method
      case (final paymentMethod?, _):
        final result = await this.widget.uriFlowCtx.enterSendFlow(
          paymentMethod,
        );
        if (!this.mounted) return;

        // Stop loading animation
        this.isPending.value = false;

        // Check the results, or show an error on the page.
        final SendState sendCtx;
        switch (result) {
          case Ok(:final ok):
            sendCtx = ok;
          case Err(:final err):
            this.errorMessage.value = ErrorMessage(message: err);
            return;
        }

        // If we still need an amount, then we have to collect that first.
        // Otherwise, a successful payment preflight means we can go directly to the
        // confirm page.
        final SendFlowResult? flowResult = await Navigator.of(this.context)
            .push(
              MaterialPageRoute(
                builder: (_) =>
                    SendPaymentPage(sendCtx: sendCtx, startNewFlow: false),
              ),
            );

        info(
          "SendPaymentNeedUriPage: flowResult: $flowResult, mounted: ${this.mounted}",
        );
        if (!this.mounted || flowResult == null) return;

        // Successfully sent payment -- return result to parent page.
        await Navigator.of(
          this.context,
        ).maybePop(UriFlowResult(sendFlowResult: flowResult));

      // Otherwise, fail and allow for another attempt
      // TODO(nicole): add claim flow and send/pay selection
      case _:
        this.isPending.value = false;
        this.errorMessage.value = ErrorMessage(
          message: "Failed to find a payment method from the URI.",
        );
        return;
    }
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
        leading: const LxCloseButton(
          isLeading: true,
          kind: LxCloseButtonKind.closeFromRoot,
        ),
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
          const HeadingText(text: "Who are we paying?"),
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
            decoration: baseInputDecoration.copyWith(
              hintText: "bc1.. lnbc1.. bitcoin:..",
            ),
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
