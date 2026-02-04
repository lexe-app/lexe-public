/// Initial deposit onboarding flow.
library;

import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/api.dart' show FiatRate;
import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        MultistepFlow,
        PaymentAmountInput,
        PaymentQrCard,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/input_formatter.dart' show IntInputFormatter;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/initial_deposit/state.dart' show DepositMethod;
import 'package:lexeapp/share.dart' show LxShare;
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxRadius, Space;
import 'package:lexeapp/url.dart' as url;

/// Minimum recommended amount for Lightning deposits.
const int minLightningDepositSats = 5000;

/// URL explaining the channel reserve concept.
const String channelReserveLearnMoreUrl =
    "https://bitcoin.design/guide/how-it-works/liquidity/#what-is-a-channel-reserve";

/// The entry point for the initial deposit onboarding flow.
///
/// Set [lightningOnly] to skip method selection and go directly to the amount
/// page.
class InitialDepositPage extends StatelessWidget {
  const InitialDepositPage({
    super.key,
    this.lightningOnly = false,
    required this.fiatRate,
  });

  /// If true, skip method selection and go directly to Lightning amount page.
  final bool lightningOnly;

  /// Fiat rate for displaying fiat equivalent.
  final ValueListenable<FiatRate?> fiatRate;

  @override
  Widget build(BuildContext context) => MultistepFlow<void>(
    builder: (context) => this.lightningOnly
        ? InitialDepositAmountPage(fiatRate: this.fiatRate)
        : InitialDepositChooseMethodPage(fiatRate: this.fiatRate),
  );
}

/// Choose between Lightning or On-chain deposit method.
class InitialDepositChooseMethodPage extends StatelessWidget {
  const InitialDepositChooseMethodPage({super.key, required this.fiatRate});

  /// Fiat rate for displaying fiat equivalent.
  final ValueListenable<FiatRate?> fiatRate;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const SizedBox.shrink(),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Let's fund your wallet"),
          const SubheadingText(
            text: "Choose how you'd like to receive your first deposit.",
          ),

          const SizedBox(height: Space.s700),

          // Lightning option
          _MethodCard(
            icon: LxIcons.lightning,
            title: "Receive ₿ via Lightning",
            description:
                "Instant and low fees. "
                "Receiving funds will open a channel.",
            isPrimary: true,
            onTap: () =>
                this._onMethodSelected(context, DepositMethod.lightning),
          ),

          const SizedBox(height: Space.s400),

          // On-chain option
          _MethodCard(
            icon: LxIcons.bitcoin,
            title: "Receive ₿ on-chain",
            description: "Send from any Bitcoin wallet. Slower, higher fees.",
            isPrimary: false,
            onTap: () => this._onMethodSelected(context, DepositMethod.onchain),
          ),
        ],
      ),
    );
  }

  void _onMethodSelected(BuildContext context, DepositMethod method) {
    Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) =>
            InitialDepositAmountPage(method: method, fiatRate: this.fiatRate),
      ),
    );
  }
}

/// A card widget for selecting a deposit method.
///
/// When [isPrimary] is true, uses an emphasized CTA-style appearance.
/// When false, uses a more subdued secondary style.
class _MethodCard extends StatelessWidget {
  const _MethodCard({
    required this.icon,
    required this.title,
    required this.description,
    required this.isPrimary,
    required this.onTap,
  });

  final IconData icon;
  final String title;
  final String description;
  final bool isPrimary;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    // Primary (Lightning): CTA-style with black background
    // Secondary (On-chain): subdued appearance
    final cardBgColor = this.isPrimary
        ? LxColors.foreground
        : LxColors.grey1000;
    final iconBgColor = this.isPrimary
        ? LxColors.clearW100
        : LxColors.fgTertiary.withValues(alpha: 0.1);
    final iconColor = this.isPrimary
        ? LxColors.background
        : LxColors.fgSecondary;
    final titleColor = this.isPrimary
        ? LxColors.background
        : LxColors.foreground;
    final descriptionColor = this.isPrimary
        ? LxColors.grey700
        : LxColors.fgSecondary;
    final arrowColor = this.isPrimary
        ? LxColors.background
        : LxColors.fgTertiary;

    return Material(
      color: cardBgColor,
      borderRadius: BorderRadius.circular(LxRadius.r400),
      child: InkWell(
        onTap: this.onTap,
        borderRadius: BorderRadius.circular(LxRadius.r400),
        child: Padding(
          padding: const EdgeInsets.all(Space.s400),
          child: Row(
            children: [
              // Icon container
              Container(
                width: 48,
                height: 48,
                decoration: BoxDecoration(
                  color: iconBgColor,
                  borderRadius: BorderRadius.circular(LxRadius.r300),
                ),
                child: Icon(this.icon, size: 24, color: iconColor),
              ),

              const SizedBox(width: Space.s400),

              // Title and description
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      this.title,
                      style: TextStyle(
                        fontSize: Fonts.size300,
                        fontVariations: const [Fonts.weightSemiBold],
                        color: titleColor,
                      ),
                    ),
                    const SizedBox(height: Space.s100),
                    Text(
                      this.description,
                      style: TextStyle(
                        fontSize: Fonts.size200,
                        color: descriptionColor,
                        height: 1.3,
                      ),
                    ),
                  ],
                ),
              ),

              const SizedBox(width: Space.s200),

              // Arrow indicator
              Icon(LxIcons.next, size: 20, color: arrowColor),
            ],
          ),
        ),
      ),
    );
  }
}

/// Enter the amount for a deposit.
class InitialDepositAmountPage extends StatefulWidget {
  const InitialDepositAmountPage({
    super.key,
    this.method = DepositMethod.lightning,
    required this.fiatRate,
  });

  /// The deposit method to use after amount entry.
  final DepositMethod method;

  /// Fiat rate for displaying fiat equivalent.
  final ValueListenable<FiatRate?> fiatRate;

  @override
  State<InitialDepositAmountPage> createState() =>
      _InitialDepositAmountPageState();
}

class _InitialDepositAmountPageState extends State<InitialDepositAmountPage> {
  final GlobalKey<FormFieldState<String>> amountFieldKey = GlobalKey();
  final IntInputFormatter intInputFormatter = IntInputFormatter();
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  /// Whether the current amount is valid (>= 1 sat).
  final ValueNotifier<bool> hasValidAmount = ValueNotifier(false);

  /// Whether to show the low amount warning card.
  final ValueNotifier<bool> showLowAmountWarning = ValueNotifier(false);

  /// Whether user acknowledged the low amount warning via checkbox.
  final ValueNotifier<bool> lowAmountAcknowledged = ValueNotifier(false);

  @override
  void dispose() {
    this.lowAmountAcknowledged.dispose();
    this.showLowAmountWarning.dispose();
    this.hasValidAmount.dispose();
    this.errorMessage.dispose();
    super.dispose();
  }

  /// Called when the amount input changes.
  void onAmountChanged(String value) {
    // Check if we have a valid amount (>= 1)
    switch (this.intInputFormatter.tryParse(value)) {
      case Ok(:final ok):
        this.hasValidAmount.value = ok >= 1;
      case Err():
        this.hasValidAmount.value = false;
    }

    // Reset warning state when amount changes
    if (this.showLowAmountWarning.value) {
      this.showLowAmountWarning.value = false;
      this.lowAmountAcknowledged.value = false;
    }
  }

  /// Parse and validate the amount, returning null if invalid.
  int? parseAmount() {
    this.errorMessage.value = null;

    final amountField = this.amountFieldKey.currentState;
    if (amountField == null) return null;

    final value = amountField.value;
    if (value == null || value.isEmpty) {
      this.errorMessage.value = const ErrorMessage(
        title: "Invalid amount",
        message: "Please enter an amount",
      );
      return null;
    }

    switch (this.intInputFormatter.tryParse(value)) {
      case Ok(:final ok):
        if (ok < 1) {
          this.errorMessage.value = const ErrorMessage(
            title: "Invalid amount",
            message: "Amount must be at least 1 sat",
          );
          return null;
        }
        return ok;
      case Err():
        this.errorMessage.value = const ErrorMessage(
          title: "Invalid amount",
          message: "Please enter a valid number",
        );
        return null;
    }
  }

  void onNext() {
    final amountSats = this.parseAmount();
    if (amountSats == null) return;

    // For low amounts, show warning and require acknowledgment
    if (amountSats < minLightningDepositSats) {
      if (!this.showLowAmountWarning.value) {
        // First tap: show the warning card
        this.showLowAmountWarning.value = true;
        return;
      }
      // Button is disabled until checkbox is checked, but just in case:
      if (!this.lowAmountAcknowledged.value) return;
    } else {
      // Amount is now sufficient; reset warning state
      this.showLowAmountWarning.value = false;
      this.lowAmountAcknowledged.value = false;
    }

    // Navigate to appropriate page based on deposit method
    final Widget nextPage = switch (this.widget.method) {
      DepositMethod.lightning => InitialDepositLightningPage(
        amountSats: amountSats,
        fiatRate: this.widget.fiatRate,
      ),
      DepositMethod.onchain => InitialDepositOnchainPage(
        amountSats: amountSats,
        fiatRate: this.widget.fiatRate,
      ),
    };

    Navigator.of(
      this.context,
    ).push(MaterialPageRoute<void>(builder: (_) => nextPage));
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
          const HeadingText(text: "How much to receive?"),

          const SizedBox(height: Space.s700),

          PaymentAmountInput(
            fieldKey: this.amountFieldKey,
            intInputFormatter: this.intInputFormatter,
            onChanged: this.onAmountChanged,
            onEditingComplete: this.onNext,
            allowEmpty: false,
            allowZero: false,
          ),

          const SizedBox(height: Space.s500),

          // Error message
          ValueListenableBuilder(
            valueListenable: this.errorMessage,
            builder: (context, errorMessage, child) =>
                ErrorMessageSection(errorMessage),
          ),

          // Low amount warning card
          ValueListenableBuilder(
            valueListenable: this.showLowAmountWarning,
            builder: (context, showWarning, child) => showWarning
                ? _LowAmountWarningCard(
                    acknowledged: this.lowAmountAcknowledged,
                  )
                : const SizedBox.shrink(),
          ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ListenableBuilder(
            listenable: Listenable.merge([
              this.hasValidAmount,
              this.showLowAmountWarning,
              this.lowAmountAcknowledged,
            ]),
            builder: (context, child) {
              final hasAmount = this.hasValidAmount.value;
              final showWarning = this.showLowAmountWarning.value;
              final acknowledged = this.lowAmountAcknowledged.value;
              final canProceed = hasAmount && (!showWarning || acknowledged);

              return LxFilledButton(
                onTap: canProceed ? this.onNext : null,
                label: Text(showWarning ? "Proceed anyway" : "Next"),
                icon: const Icon(LxIcons.next),
              );
            },
          ),
        ),
      ),
    );
  }
}

/// Warning card shown when user enters an amount below the recommended minimum.
class _LowAmountWarningCard extends StatelessWidget {
  const _LowAmountWarningCard({required this.acknowledged});

  final ValueNotifier<bool> acknowledged;

  void onLearnMore() {
    unawaited(url.open(channelReserveLearnMoreUrl));
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: Space.s500),
      child: Card.filled(
        color: LxColors.grey1000,
        margin: EdgeInsets.zero,
        child: Padding(
          padding: const EdgeInsets.all(Space.s400),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              // Title with warning icon
              Row(
                children: [
                  Icon(LxIcons.warning, size: 20, color: LxColors.warningText),
                  const SizedBox(width: Space.s200),
                  Text(
                    "Insufficient initial deposit",
                    style: TextStyle(
                      fontSize: Fonts.size300,
                      fontVariations: [Fonts.weightSemiBold],
                      color: LxColors.foreground,
                    ),
                  ),
                ],
              ),

              const SizedBox(height: Space.s300),

              // Body text with learn more link
              Text.rich(
                TextSpan(
                  children: [
                    TextSpan(
                      text:
                          "For the best experience, we recommend depositing "
                          "at least ${currency_format.formatSatsAmount(minLightningDepositSats)} "
                          "to cover the channel reserve. ",
                    ),
                    WidgetSpan(
                      alignment: PlaceholderAlignment.baseline,
                      baseline: TextBaseline.alphabetic,
                      child: GestureDetector(
                        onTap: this.onLearnMore,
                        child: Text(
                          "Learn more",
                          style: TextStyle(
                            fontSize: Fonts.size200,
                            color: LxColors.linkText,
                            decoration: TextDecoration.underline,
                            decorationColor: LxColors.linkText,
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
                style: TextStyle(
                  fontSize: Fonts.size200,
                  color: LxColors.fgSecondary,
                  height: 1.4,
                ),
              ),

              const SizedBox(height: Space.s400),

              // Checkbox row
              ValueListenableBuilder(
                valueListenable: this.acknowledged,
                builder: (context, isChecked, child) => CheckboxListTile(
                  value: isChecked,
                  onChanged: (value) =>
                      this.acknowledged.value = value ?? false,
                  controlAffinity: ListTileControlAffinity.leading,
                  contentPadding: EdgeInsets.zero,
                  visualDensity: VisualDensity.compact,
                  materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                  title: Text(
                    "I understand that my funds might not cover "
                    "the channel reserve",
                    style: TextStyle(
                      fontSize: Fonts.size200,
                      color: LxColors.fgSecondary,
                      height: 1.3,
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Success page shown after initial deposit is received.
class InitialDepositSuccessPage extends StatelessWidget {
  const InitialDepositSuccessPage({
    super.key,
    required this.amountSats,
    required this.fiatRate,
  });

  final int amountSats;
  final ValueListenable<FiatRate?> fiatRate;

  void _onDone(BuildContext context) {
    Navigator.of(context, rootNavigator: true).pop();
  }

  @override
  Widget build(BuildContext context) {
    final amountSatsStr = currency_format.formatSatsAmount(this.amountSats);

    return Scaffold(
      appBar: AppBar(
        automaticallyImplyLeading: false,
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const SizedBox(height: Space.s500),

          // Lightning icon with success badge
          Align(
            alignment: Alignment.topCenter,
            child: Badge(
              label: const Icon(
                LxIcons.completedBadge,
                size: Fonts.size400,
                color: LxColors.background,
              ),
              backgroundColor: LxColors.moneyGoUp,
              largeSize: Space.s500,
              child: const DecoratedBox(
                decoration: BoxDecoration(
                  color: LxColors.grey825,
                  borderRadius: BorderRadius.all(
                    Radius.circular(Space.s800 / 2),
                  ),
                ),
                child: SizedBox.square(
                  dimension: Space.s800,
                  child: Icon(
                    LxIcons.lightning,
                    size: Space.s700,
                    color: LxColors.fgSecondary,
                    fill: 1.0,
                    weight: LxIcons.weightExtraLight,
                  ),
                ),
              ),
            ),
          ),

          const SizedBox(height: Space.s500),

          // "Received" label
          Text(
            "Received",
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size300,
              color: LxColors.fgTertiary,
              fontVariations: [Fonts.weightNormal],
            ),
            textAlign: TextAlign.center,
          ),

          const SizedBox(height: Space.s200),

          // Amount in sats
          Text(
            "+$amountSatsStr",
            style: Fonts.fontUI.copyWith(
              letterSpacing: -0.5,
              fontSize: Fonts.size800,
              fontVariations: [Fonts.weightNormal],
              fontFeatures: [Fonts.featSlashedZero],
              color: LxColors.moneyGoUp,
            ),
            textAlign: TextAlign.center,
          ),

          // Fiat amount
          const SizedBox(height: Space.s200),
          ValueListenableBuilder<FiatRate?>(
            valueListenable: this.fiatRate,
            builder: (context, fiatRate, child) {
              if (fiatRate == null) return const SizedBox.shrink();
              final fiatAmount =
                  currency_format.satsToBtc(this.amountSats) * fiatRate.rate;
              final fiatAmountStr = currency_format.formatFiat(
                fiatAmount,
                fiatRate.fiat,
              );
              return Text(
                "≈ $fiatAmountStr",
                style: Fonts.fontUI.copyWith(
                  fontSize: Fonts.size300,
                  color: LxColors.fgSecondary,
                ),
                textAlign: TextAlign.center,
              );
            },
          ),

          const SizedBox(height: Space.s600),

          // Subtext
          Text(
            "You're all set! Enjoy Lexe wallet.",
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size200,
              color: LxColors.fgSecondary,
            ),
            textAlign: TextAlign.center,
          ),
        ],
        bottom: LxFilledButton.strong(
          onTap: () => this._onDone(context),
          label: const Text("Done"),
          icon: const Icon(LxIcons.next),
        ),
      ),
    );
  }
}

/// Lightning deposit page showing a BOLT11 invoice QR code.
class InitialDepositLightningPage extends StatefulWidget {
  const InitialDepositLightningPage({
    super.key,
    required this.amountSats,
    required this.fiatRate,
  });

  /// The amount to request in the invoice.
  final int amountSats;

  /// Fiat rate for displaying fiat equivalent.
  final ValueListenable<FiatRate?> fiatRate;

  @override
  State<InitialDepositLightningPage> createState() =>
      _InitialDepositLightningPageState();
}

class _InitialDepositLightningPageState
    extends State<InitialDepositLightningPage> {
  /// The BOLT11 invoice string, or null while loading.
  // TODO(a-mpch): Wire up to fetch a real invoice from the node.
  final ValueNotifier<String?> invoice = ValueNotifier(null);

  /// Compute the lightning: URI from the invoice.
  static String invoiceToUri(String invoice) => "lightning:$invoice";

  @override
  void initState() {
    super.initState();
    // Simulate invoice generation with a placeholder after a short delay.
    // TODO(a-mpch): Remove this when adding real logic.
    Future.delayed(const Duration(milliseconds: 500), () {
      if (this.mounted) {
        // Placeholder invoice for UI testing
        this.invoice.value = "lnbc${this.widget.amountSats}n1pn9example";
      }
    });
  }

  @override
  void dispose() {
    this.invoice.dispose();
    super.dispose();
  }

  void onCopy(BuildContext context, String invoice) {
    final uri = invoiceToUri(invoice);
    unawaited(LxClipboard.copyTextWithFeedback(context, uri));
  }

  Future<void> onShare(BuildContext context, String invoice) async {
    final uri = invoiceToUri(invoice);
    await LxShare.sharePaymentUri(context, Uri.parse(uri));
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
      body: ValueListenableBuilder<String?>(
        valueListenable: this.invoice,
        builder: (context, invoice, child) {
          final uri = invoice != null ? invoiceToUri(invoice) : null;
          return ScrollableSinglePageBody(
            body: [
              const HeadingText(text: "Receive payment"),
              const SubheadingText(
                text:
                    "Scan this QR code with a Lightning wallet to send payment.",
              ),

              const SizedBox(height: Space.s600),

              // Invoice QR code card
              PaymentQrCard(
                uri: uri,
                code: invoice,
                amountSats: this.widget.amountSats,
                fiatRate: this.widget.fiatRate,
                onCopy: () => this.onCopy(context, invoice!),
              ),

              const SizedBox(height: Space.s400),

              // Copy and Share buttons
              Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: Space.s200),
                    child: FilledButton(
                      onPressed: uri != null
                          ? () => this.onCopy(context, invoice!)
                          : null,
                      child: const Icon(LxIcons.copy),
                    ),
                  ),
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: Space.s200),
                    child: FilledButton(
                      onPressed: uri != null
                          ? () => this.onShare(context, invoice!)
                          : null,
                      child: const Icon(LxIcons.share),
                    ),
                  ),
                ],
              ),

              const SizedBox(height: Space.s600),

              // Waiting indicator (only shown when invoice is loaded)
              if (invoice != null)
                const Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        color: LxColors.fgTertiary,
                      ),
                    ),
                    SizedBox(width: Space.s300),
                    Text(
                      "Waiting for payment...",
                      style: TextStyle(
                        fontSize: Fonts.size200,
                        color: LxColors.fgSecondary,
                      ),
                    ),
                  ],
                ),
            ],
          );
        },
      ),
    );
  }
}

/// On-chain deposit page showing a Bitcoin address QR code.
class InitialDepositOnchainPage extends StatefulWidget {
  const InitialDepositOnchainPage({
    super.key,
    required this.amountSats,
    required this.fiatRate,
  });

  /// The requested amount in satoshis.
  final int amountSats;

  /// Fiat rate for displaying fiat equivalent.
  final ValueListenable<FiatRate?> fiatRate;

  @override
  State<InitialDepositOnchainPage> createState() =>
      _InitialDepositOnchainPageState();
}

class _InitialDepositOnchainPageState extends State<InitialDepositOnchainPage> {
  /// The Bitcoin address, or null while loading.
  // TODO(a-mpch): Wire up to fetch a real address from the node.
  final ValueNotifier<String?> address = ValueNotifier(null);

  /// Compute the BIP21 URI from the address and amount.
  String addressToUri(String address) {
    final amountBtc = currency_format.satsToBtc(this.widget.amountSats);
    return "bitcoin:$address?amount=$amountBtc";
  }

  @override
  void initState() {
    super.initState();
    // Simulate address generation with a placeholder after a short delay.
    // TODO(a-mpch): Remove this when adding real logic.
    Future.delayed(const Duration(milliseconds: 500), () {
      if (this.mounted) {
        this.address.value = "bc1qexampleaddressforuitesting0000000000";
      }
    });
  }

  @override
  void dispose() {
    this.address.dispose();
    super.dispose();
  }

  void onCopy(BuildContext context, String address) {
    final uri = this.addressToUri(address);
    unawaited(LxClipboard.copyTextWithFeedback(context, uri));
  }

  Future<void> onShare(BuildContext context, String address) async {
    final uri = this.addressToUri(address);
    await LxShare.sharePaymentUri(context, Uri.parse(uri));
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
      body: ValueListenableBuilder<String?>(
        valueListenable: this.address,
        builder: (context, address, child) {
          final uri = address != null ? this.addressToUri(address) : null;
          return ScrollableSinglePageBody(
            body: [
              const HeadingText(text: "Receive Bitcoin"),
              const SubheadingText(
                text: "Send Bitcoin to this address from any wallet.",
              ),

              const SizedBox(height: Space.s600),

              // Address QR code card
              PaymentQrCard(
                uri: uri,
                code: address,
                amountSats: this.widget.amountSats,
                fiatRate: this.widget.fiatRate,
                onCopy: () => this.onCopy(context, address!),
              ),

              const SizedBox(height: Space.s400),

              // Copy and Share buttons
              Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: Space.s200),
                    child: FilledButton(
                      onPressed: uri != null
                          ? () => this.onCopy(context, address!)
                          : null,
                      child: const Icon(LxIcons.copy),
                    ),
                  ),
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: Space.s200),
                    child: FilledButton(
                      onPressed: uri != null
                          ? () => this.onShare(context, address!)
                          : null,
                      child: const Icon(LxIcons.share),
                    ),
                  ),
                ],
              ),

              const SizedBox(height: Space.s600),

              // Waiting indicator (only shown when address is loaded)
              if (address != null)
                const Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        color: LxColors.fgTertiary,
                      ),
                    ),
                    SizedBox(width: Space.s300),
                    Text(
                      "Waiting for payment...",
                      style: TextStyle(
                        fontSize: Fonts.size200,
                        color: LxColors.fgSecondary,
                      ),
                    ),
                  ],
                ),
            ],
          );
        },
      ),
    );
  }
}
