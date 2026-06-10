import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/api.dart' show FiatRate;
import 'package:app_rs_dart/ffi/types.dart' show ClaimMethod_LnurlWithdraw;
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        MultistepFlow,
        PaymentAmountInput,
        PaymentNoteInput,
        ReceiptSeparator,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/currency_format.dart'
    as currency_format
    show formatFiat, formatSatsAmount, satsToBtc;
import 'package:lexeapp/input_formatter.dart' show IntInputFormatter;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/claim/state.dart'
    show
        ClaimFlowResult,
        ClaimReady_LnurlWithdraw,
        ClaimState,
        ClaimState_NeedAmount,
        ClaimState_NeedConfirm;
import 'package:lexeapp/route/send/page.dart' show MetadataRow;
import 'package:lexeapp/string_ext.dart';
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;

/// The entry point for the claim payment flow. This will dispatch to the right
/// initial screen depending on the [ClaimState]. If [startNewFlow], then it
/// also sets up a new / [MultistepFlow] so navigation "close" will exit out of
/// the whole flow.
class ClaimPaymentPage extends StatelessWidget {
  const ClaimPaymentPage({
    super.key,
    required this.claimCtx,
    required this.startNewFlow,
  });

  final ClaimState claimCtx;
  final bool startNewFlow;

  Widget buildInnerClaimPage() {
    final claimCtx = this.claimCtx;
    return switch (claimCtx) {
      ClaimState_NeedAmount() => ClaimPaymentAmountPage(claimCtx: claimCtx),
      ClaimState_NeedConfirm() => ClaimPaymentConfirmPage(claimCtx: claimCtx),
    };
  }

  @override
  Widget build(BuildContext context) => (this.startNewFlow)
      ? MultistepFlow<ClaimFlowResult>(
          builder: (_) => this.buildInnerClaimPage(),
        )
      : this.buildInnerClaimPage();
}

/// Claim payment flow: this page collects all wire-necessary information
/// for a claim payment
class ClaimPaymentAmountPage extends StatefulWidget {
  const ClaimPaymentAmountPage({super.key, required this.claimCtx});

  final ClaimState_NeedAmount claimCtx;

  @override
  State<ClaimPaymentAmountPage> createState() => _ClaimPaymentAmountPageState();
}

class _ClaimPaymentAmountPageState extends State<ClaimPaymentAmountPage> {
  static final intInputFormatter = IntInputFormatter();

  /// `true`  -> button loading animation plays;
  /// `false` -> button is normal
  final ValueNotifier<bool> buttonLoadingAnim = ValueNotifier(false);

  @override
  void dispose() {
    this.buttonLoadingAnim.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final String? description;
    final int minWithdrawableSats;
    final int maxWithdrawableSats;
    String minWithdrawableSatsStr;
    String maxWithdrawableSatsStr;

    // The data for the details section, eg:
    // Description           Coffee
    // Minimum amount          ₿100
    final List<(String, String)> detailsList;

    // Populate data
    switch (this.widget.claimCtx.claimMethod) {
      case ClaimMethod_LnurlWithdraw claimMethod:
        description = claimMethod.withdrawRequest.defaultDescription.nonEmpty();
        // Round the minWithdrawableMsat up to the nearest sat
        minWithdrawableSats =
            claimMethod.withdrawRequest.minWithdrawableMsat > 0
            ? ((claimMethod.withdrawRequest.minWithdrawableMsat - 1) ~/ 1000) +
                  1
            : 0;
        maxWithdrawableSats =
            claimMethod.withdrawRequest.maxWithdrawableMsat ~/ 1000;
        minWithdrawableSatsStr = currency_format.formatSatsAmount(
          minWithdrawableSats,
          bitcoinSymbol: true,
        );
        maxWithdrawableSatsStr = currency_format.formatSatsAmount(
          maxWithdrawableSats,
          bitcoinSymbol: true,
        );
        detailsList = [
          if (description != null) ("Description", description),
          if (minWithdrawableSats > 1)
            ("Minimum amount", minWithdrawableSatsStr),
          ("Maximum amount", maxWithdrawableSatsStr),
        ];
    }

    // The keys for the input fields
    final amountInputKey = GlobalKey<FormFieldState<String>>();
    final messageKey = GlobalKey<FormFieldState<String>>();

    Future<void> onNext() async {
      // Validate the amount input field
      final amountFieldState = amountInputKey.currentState!;
      if (!amountFieldState.validate()) return;

      final amountStr = amountFieldState.value;
      if (amountStr == null || amountStr.isEmpty) return;

      final int amountSats;
      switch (_ClaimPaymentAmountPageState.intInputFormatter.tryParse(
        amountStr,
      )) {
        case Err():
          return;
        case Ok(:final ok):
          amountSats = ok;
      }

      // Get the message from the input field
      final message = messageKey.currentState?.value?.nonEmpty();

      // Advance state
      final needConfirmCtx = this.widget.claimCtx.withAmount(
        amountSats,
        message: message,
      );

      final ClaimFlowResult? flowResult = await Navigator.push(
        context,
        MaterialPageRoute(
          builder: (context) =>
              ClaimPaymentConfirmPage(claimCtx: needConfirmCtx),
        ),
      );

      // Confirm page results:
      info(
        "ClaimPaymentAmountPage: flowResult: $flowResult, mounted: ${this.mounted}",
      );
      if (!this.mounted || flowResult == null) return;

      await Navigator.of(this.context).maybePop(flowResult);
    }

    // The validate function for the amount input field
    Result<(), String> validateAmount(int amount) {
      // Ensure min amount <= withdraw amount
      if (amount < minWithdrawableSats) {
        return Err("Must withdraw at least $minWithdrawableSatsStr");
      }

      // Ensure max amount >= withdraw amount
      final maxAmount = maxWithdrawableSats;
      if (amount > maxAmount) {
        return Err("Can't withdraw more than $maxWithdrawableSatsStr");
      }

      return const Ok(());
    }

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
          const HeadingText(text: "How much?"),
          SubheadingText(text: "Withdrawing from LNURL"),
          const SizedBox(height: Space.s600),

          // Amount input box, formatted: "₿<amount>" (en_US)
          //                              "<amount> ₿" (fr_FR)
          PaymentAmountInput(
            fieldKey: amountInputKey,
            // TODO(nicole): for LNURL-withdraw and pay, if we can't pay msat amounts,
            // we could run into an impossible request with bounds eg [1.4, 1.6] sat
            intInputFormatter: _ClaimPaymentAmountPageState.intInputFormatter,
            onEditingComplete: onNext,
            validate: validateAmount,
            allowEmpty: false,
            allowZero: false,
            initialValue: maxWithdrawableSats,
          ),

          // Details, eg: Description           Coffee
          //              Minimum amount          ₿100
          Column(
            crossAxisAlignment: CrossAxisAlignment.center,
            spacing: Space.s300,
            children: [
              for (final (title, value) in detailsList)
                MetadataRow(title: title, value: value),
            ],
          ),
          if (detailsList.isNotEmpty) const SizedBox(height: Space.s300),

          // Message (invoice description)
          const Text(
            "Optional message",
            style: TextStyle(
              fontSize: Fonts.size200,
              color: LxColors.fgTertiary,
            ),
          ),
          const SizedBox(height: Space.s200),
          PaymentNoteInput(
            fieldKey: messageKey,
            onSubmit: onNext,
            hintText: "Optional message (visible to recipient)",
            // BOLT11 invoice description limit
            maxLength: 200,
            textInputAction: TextInputAction.next,
          ),
        ],

        // Next ->
        bottom: Padding(
          padding: const EdgeInsets.symmetric(vertical: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.buttonLoadingAnim,
            builder: (_context, buttonLoadingAnim, _widget) =>
                AnimatedFillButton(
                  label: const Text("Next"),
                  icon: const Icon(LxIcons.next),
                  onTap: onNext,
                  loading: buttonLoadingAnim,
                ),
          ),
        ),
      ),
    );
  }
}

class ClaimPaymentConfirmPage extends StatefulWidget {
  const ClaimPaymentConfirmPage({super.key, required this.claimCtx});

  final ClaimState_NeedConfirm claimCtx;

  static const _textStylePrimary = TextStyle(
    fontSize: Fonts.size300,
    color: LxColors.foreground,
    fontVariations: [Fonts.weightMedium],
  );

  static const _textStyleSecondary = TextStyle(
    fontSize: Fonts.size300,
    color: LxColors.grey550,
    fontVariations: [],
  );

  static const _textStyleFiat = TextStyle(
    fontSize: Fonts.size200,
    color: LxColors.grey550,
  );

  /// Regex to strip "http://" or "https://" prefix and any "/trailing/path"
  static final _httpPrefixAndPathRe = RegExp(
    r'(^https?://)|((?!<:/)/.*)',
    caseSensitive: false,
  );

  @override
  State<ClaimPaymentConfirmPage> createState() =>
      _ClaimPaymentConfirmPageState();
}

class _ClaimPaymentConfirmPageState extends State<ClaimPaymentConfirmPage> {
  final GlobalKey<FormFieldState<String>> personalNoteFieldKey = GlobalKey();

  final ValueNotifier<ErrorMessage?> claimError = ValueNotifier(null);
  final ValueNotifier<bool> isClaiming = ValueNotifier(false);

  /// Frozen fiat rate captured when this page is shown.
  /// Freezing prevents confusing rate changes while the user is confirming.
  late final FiatRate? frozenFiatRate = this.widget.claimCtx.fiatRate.value;

  /// Format a sats amount as fiat, or null if no fiat rate is available.
  String? formatFiatAmount(int sats) {
    final rate = this.frozenFiatRate;
    if (rate == null) return null;
    final fiatAmount = currency_format.satsToBtc(sats) * rate.rate;
    return "≈ ${currency_format.formatFiat(fiatAmount, rate.fiat)}";
  }

  @override
  void dispose() {
    this.isClaiming.dispose();
    this.claimError.dispose();
    super.dispose();
  }

  int amountSats() => switch (this.widget.claimCtx.claimable) {
    ClaimReady_LnurlWithdraw(:final amountMsat) => amountMsat ~/ 1000,
  };

  String payer() {
    switch (this.widget.claimCtx.claimable) {
      case ClaimReady_LnurlWithdraw(:final httpUrl):
        final re = ClaimPaymentConfirmPage._httpPrefixAndPathRe;
        final stripped = httpUrl.replaceAll(re, '');
        if (stripped.length <= 15) {
          return stripped;
        }
        final substring = stripped.substring(0, 14);
        return "$substring\u2026";
    }
  }

  /// The current (non-empty) personal note field contents, if any.
  String? personalNote() =>
      this.personalNoteFieldKey.currentState?.value?.nonEmpty();

  String? description() => switch (this.widget.claimCtx.claimable) {
    ClaimReady_LnurlWithdraw(:final withdrawRequest) =>
      withdrawRequest.defaultDescription.nonEmpty(),
  };

  String? message() => switch (this.widget.claimCtx.claimable) {
    ClaimReady_LnurlWithdraw(:final description) => description?.nonEmpty(),
  };

  Future<void> onConfirm() async {
    if (this.isClaiming.value) return;

    // We're claiming; clear the errors and disable the form inputs.
    this.isClaiming.value = true;
    this.claimError.value = null;

    // Actually start the payment
    final FfiResult<ClaimFlowResult> result = await this.widget.claimCtx.claim(
      personalNote: this.personalNote(),
    );
    if (!this.mounted) return;

    switch (result) {
      case Ok(:final ok):
        // The request succeeded and we're still mounted (the user hasn't
        // navigated away somehow). Let's pop ourselves off the nav stack and
        // notify our caller that we were successful.
        final flowResult = ok;
        info("ClaimPaymentConfirmPage: success: flowResult: $flowResult");
        unawaited(Navigator.of(this.context).maybePop(flowResult));

      case Err(:final err):
        // The request failed. Set the error message and unset loading.
        error("ClaimPaymentConfirmPage: error claiming payment: $err");
        this.isClaiming.value = false;
        this.claimError.value = ErrorMessage(
          title: "Error claiming payment",
          message: err.message,
        );
    }
  }

  @override
  Widget build(BuildContext context) {
    const textStyleSecondary = ClaimPaymentConfirmPage._textStyleSecondary;
    const textStylePrimary = ClaimPaymentConfirmPage._textStylePrimary;
    const textStyleFiat = ClaimPaymentConfirmPage._textStyleFiat;

    final shortPayer = this.payer();

    final amountSatsStr = currency_format.formatSatsAmount(this.amountSats());
    final amountFiatStr = this.formatFiatAmount(this.amountSats());

    final description = this.description();
    final message = this.message();

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
          const HeadingText(text: "Confirm payment"),
          SubheadingText(text: "Withdrawing from LNURL"),
          const SizedBox(height: Space.s700),

          //
          // To   <address/invoice/etc...>
          //
          Row(
            mainAxisSize: MainAxisSize.max,
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Text("From", style: textStyleSecondary),
              Text(
                shortPayer,
                style: textStylePrimary.copyWith(
                  fontFeatures: [Fonts.featDisambugation],
                ),
              ),
            ],
          ),

          const SizedBox(height: Space.s400),

          //
          // Amount         XXX sats
          // Network Fee   ~YYY sats
          //
          Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              //
              // Amount to-be-received
              //
              Row(
                mainAxisSize: MainAxisSize.max,
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Text("Amount", style: textStyleSecondary),
                  Column(
                    crossAxisAlignment: CrossAxisAlignment.end,
                    children: [
                      Text(amountSatsStr, style: textStyleSecondary),
                      if (amountFiatStr != null)
                        Text(amountFiatStr, style: textStyleFiat),
                    ],
                  ),
                ],
              ),
            ],
          ),

          // separator - /\/\/\/\/\/\/\/\/\/\/
          const ReceiptSeparator(),

          //
          // Total amount to be claimed by user/payer
          //
          Row(
            mainAxisSize: MainAxisSize.max,
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text("Total", style: textStyleSecondary),
              Column(
                crossAxisAlignment: CrossAxisAlignment.end,
                children: [
                  Text(amountSatsStr, style: textStylePrimary),
                  if (amountFiatStr != null)
                    Text(amountFiatStr, style: textStyleFiat),
                ],
              ),
            ],
          ),

          const SizedBox(height: Space.s300),

          //
          // Description
          //
          if (description != null)
            MetadataRow(title: "Description", value: description),

          //
          // Message to recipient
          //
          if (message != null) MetadataRow(title: "Message", value: message),

          if (description != null || message != null)
            const SizedBox(height: Space.s450),

          //
          // Optional payment note input
          //
          ValueListenableBuilder(
            valueListenable: this.isClaiming,
            builder: (context, isClaiming, widget) => PaymentNoteInput(
              fieldKey: this.personalNoteFieldKey,
              onSubmit: this.onConfirm,
              isEnabled: !isClaiming,
            ),
          ),

          //
          // Claim payment error
          //
          ValueListenableBuilder(
            valueListenable: this.claimError,
            builder: (context, claimError, widget) => Padding(
              padding: const EdgeInsets.symmetric(vertical: Space.s400),
              child: ErrorMessageSection(claimError),
            ),
          ),
        ],
        bottom: Column(
          mainAxisSize: MainAxisSize.min,
          mainAxisAlignment: MainAxisAlignment.end,
          verticalDirection: VerticalDirection.down,
          children: [
            const Expanded(child: SizedBox(height: Space.s500)),

            // Disable the button and show a loading indicator while claiming
            // the request.
            ValueListenableBuilder(
              valueListenable: this.isClaiming,
              builder: (context, isClaiming, widget) => AnimatedFillButton(
                label: const Text("Receive"),
                icon: const Icon(LxIcons.next),
                onTap: this.onConfirm,
                loading: isClaiming,
                style: FilledButton.styleFrom(
                  backgroundColor: LxColors.moneyGoUp,
                  foregroundColor: LxColors.grey1000,
                  iconColor: LxColors.grey1000,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
