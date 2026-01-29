/// Initial deposit onboarding flow.
library;

import 'dart:async' show unawaited;

import 'package:flutter/material.dart';
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        ErrorMessage,
        ErrorMessageSection,
        FilledPlaceholder,
        HeadingText,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        MultistepFlow,
        PaymentAmountInput,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/input_formatter.dart' show IntInputFormatter;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/initial_deposit/state.dart' show DepositMethod;
import 'package:lexeapp/route/show_qr.dart' show InteractiveQrImage;
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
  const InitialDepositPage({super.key, this.lightningOnly = false});

  /// If true, skip method selection and go directly to Lightning amount page.
  final bool lightningOnly;

  @override
  Widget build(BuildContext context) => MultistepFlow<void>(
    builder: (context) => this.lightningOnly
        ? const InitialDepositAmountPage()
        : const InitialDepositChooseMethodPage(),
  );
}

/// Choose between Lightning or On-chain deposit method.
class InitialDepositChooseMethodPage extends StatelessWidget {
  const InitialDepositChooseMethodPage({super.key});

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
            title: "Lightning",
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
            title: "On-chain",
            description: "Send from any Bitcoin wallet. Slower, higher fees.",
            isPrimary: false,
            onTap: () => this._onMethodSelected(context, DepositMethod.onchain),
          ),
        ],
      ),
    );
  }

  void _onMethodSelected(BuildContext context, DepositMethod method) {
    switch (method) {
      case DepositMethod.lightning:
        Navigator.of(context).push(
          MaterialPageRoute<void>(
            builder: (_) => const InitialDepositAmountPage(),
          ),
        );
      case DepositMethod.onchain:
        Navigator.of(context).push(
          MaterialPageRoute<void>(
            builder: (_) => const InitialDepositOnchainPage(),
          ),
        );
    }
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
    // Primary (Lightning): emphasized with border and icon highlight
    // Secondary (On-chain): subdued appearance
    final borderColor = this.isPrimary
        ? LxColors.foreground
        : Colors.transparent;
    final iconBgColor = this.isPrimary
        ? LxColors.foreground
        : LxColors.fgTertiary.withValues(alpha: 0.1);
    final iconColor = this.isPrimary
        ? LxColors.background
        : LxColors.fgSecondary;

    return Material(
      color: LxColors.grey1000,
      borderRadius: BorderRadius.circular(LxRadius.r400),
      child: InkWell(
        onTap: this.onTap,
        borderRadius: BorderRadius.circular(LxRadius.r400),
        child: Container(
          decoration: BoxDecoration(
            border: Border.all(color: borderColor, width: 1.5),
            borderRadius: BorderRadius.circular(LxRadius.r400),
          ),
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
                      style: const TextStyle(
                        fontSize: Fonts.size400,
                        fontVariations: [Fonts.weightSemiBold],
                        color: LxColors.foreground,
                      ),
                    ),
                    const SizedBox(height: Space.s100),
                    Text(
                      this.description,
                      style: const TextStyle(
                        fontSize: Fonts.size200,
                        color: LxColors.fgSecondary,
                        height: 1.3,
                      ),
                    ),
                  ],
                ),
              ),

              const SizedBox(width: Space.s200),

              // Arrow indicator
              const Icon(LxIcons.next, size: 20, color: LxColors.fgTertiary),
            ],
          ),
        ),
      ),
    );
  }
}

/// Enter the amount for a Lightning deposit.
class InitialDepositAmountPage extends StatefulWidget {
  const InitialDepositAmountPage({super.key});

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

    // Navigate to Lightning page with the requested amount
    Navigator.of(this.context).push(
      MaterialPageRoute<void>(
        builder: (_) => InitialDepositLightningPage(amountSats: amountSats),
      ),
    );
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
                    const TextSpan(
                      text:
                          "For the best experience, we recommend depositing "
                          "at least ₿5,000 to cover the channel reserve. ",
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
                builder: (context, isChecked, child) => GestureDetector(
                  onTap: () => this.acknowledged.value = !isChecked,
                  behavior: HitTestBehavior.opaque,
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      SizedBox(
                        width: 24,
                        height: 24,
                        child: Checkbox(
                          value: isChecked,
                          onChanged: (value) =>
                              this.acknowledged.value = value ?? false,
                          materialTapTargetSize:
                              MaterialTapTargetSize.shrinkWrap,
                          visualDensity: VisualDensity.compact,
                        ),
                      ),
                      const SizedBox(width: Space.s200),
                      Expanded(
                        child: Padding(
                          padding: const EdgeInsets.only(top: 2),
                          child: Text(
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
            ],
          ),
        ),
      ),
    );
  }
}

/// Success page shown after initial deposit is received.
class InitialDepositSuccessPage extends StatelessWidget {
  const InitialDepositSuccessPage({super.key, required this.amountSats});

  final int amountSats;

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
  const InitialDepositLightningPage({super.key, required this.amountSats});

  /// The amount to request in the invoice.
  final int amountSats;

  @override
  State<InitialDepositLightningPage> createState() =>
      _InitialDepositLightningPageState();
}

class _InitialDepositLightningPageState
    extends State<InitialDepositLightningPage> {
  /// The invoice URI to display, or null while loading.
  // TODO(a-mpch): Wire up to fetch a real invoice from the node.
  final ValueNotifier<String?> invoiceUri = ValueNotifier(null);

  @override
  void initState() {
    super.initState();
    // Simulate invoice generation with a placeholder after a short delay.
    // TODO(a-mpch): Remove this when adding real logic.
    Future.delayed(const Duration(milliseconds: 500), () {
      if (this.mounted) {
        // Placeholder invoice for UI testing
        this.invoiceUri.value =
            "lightning:lnbc${this.widget.amountSats}n1pn9example";
      }
    });
  }

  @override
  void dispose() {
    this.invoiceUri.dispose();
    super.dispose();
  }

  void onCopy(BuildContext context, String? uri) {
    if (uri == null) return;
    unawaited(LxClipboard.copyTextWithFeedback(context, uri));
  }

  Future<void> onShare(BuildContext context, String? uri) async {
    if (uri == null) return;
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
        valueListenable: this.invoiceUri,
        builder: (context, uri, child) => ScrollableSinglePageBody(
          body: [
            const HeadingText(text: "Receive payment"),
            const SubheadingText(
              text:
                  "Scan this QR code with a Lightning wallet to send payment.",
            ),

            const SizedBox(height: Space.s600),

            // Invoice QR code card
            _PaymentQrCard(
              uri: uri,
              onCopy: () => this.onCopy(context, uri),
              onShare: () => this.onShare(context, uri),
            ),

            const SizedBox(height: Space.s600),

            // Waiting indicator (only shown when QR is loaded)
            if (uri != null)
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
        ),
      ),
    );
  }
}

/// QR card widget for displaying a payment code with copy/share actions.
class _PaymentQrCard extends StatelessWidget {
  const _PaymentQrCard({
    required this.uri,
    required this.onCopy,
    required this.onShare,
  });

  /// The URI to display in the QR code, or null while loading.
  final String? uri;
  final VoidCallback onCopy;
  final VoidCallback onShare;

  @override
  Widget build(BuildContext context) {
    final uri = this.uri;

    return Container(
      margin: const EdgeInsets.symmetric(horizontal: Space.s200),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.start,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Card with QR code
          Container(
            decoration: BoxDecoration(
              color: LxColors.grey1000,
              borderRadius: BorderRadius.circular(LxRadius.r300),
            ),
            padding: const EdgeInsets.all(Space.s450),
            clipBehavior: Clip.antiAlias,
            child: LayoutBuilder(
              builder: (context, constraints) {
                final double dim = constraints.maxWidth;
                final key = ValueKey(uri ?? "");
                return AnimatedSwitcher(
                  duration: const Duration(milliseconds: 250),
                  child: (uri != null)
                      ? Container(
                          decoration: BoxDecoration(
                            borderRadius: BorderRadius.circular(6.0),
                          ),
                          clipBehavior: Clip.antiAlias,
                          child: InteractiveQrImage(
                            key: key,
                            value: uri,
                            dimension: dim,
                          ),
                        )
                      : FilledPlaceholder(
                          key: key,
                          width: dim,
                          height: dim,
                          color: LxColors.background,
                          borderRadius: 6.0,
                          child: const Center(
                            child: SizedBox.square(
                              dimension: Fonts.size800,
                              child: CircularProgressIndicator(
                                strokeWidth: 3.0,
                                color: LxColors.clearB200,
                              ),
                            ),
                          ),
                        ),
                );
              },
            ),
          ),

          const SizedBox(height: Space.s400),

          // Copy and Share buttons
          Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: Space.s200),
                child: FilledButton(
                  onPressed: uri != null ? this.onCopy : null,
                  child: const Icon(LxIcons.copy),
                ),
              ),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: Space.s200),
                child: FilledButton(
                  onPressed: uri != null ? this.onShare : null,
                  child: const Icon(LxIcons.share),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// On-chain deposit page showing a Bitcoin address QR code.
class InitialDepositOnchainPage extends StatefulWidget {
  const InitialDepositOnchainPage({super.key});

  @override
  State<InitialDepositOnchainPage> createState() =>
      _InitialDepositOnchainPageState();
}

class _InitialDepositOnchainPageState extends State<InitialDepositOnchainPage> {
  /// The Bitcoin address URI, or null while loading.
  // TODO(a-mpch): Wire up to fetch a real address from the node.
  final ValueNotifier<String?> addressUri = ValueNotifier(null);

  @override
  void initState() {
    super.initState();
    // Simulate address generation with a placeholder after a short delay.
    // TODO(a-mpch): Remove this when adding real logic.
    Future.delayed(const Duration(milliseconds: 500), () {
      if (this.mounted) {
        // Placeholder address for UI testing
        this.addressUri.value =
            "bitcoin:bc1qexampleaddressforuitesting0000000000";
      }
    });
  }

  @override
  void dispose() {
    this.addressUri.dispose();
    super.dispose();
  }

  void onCopy(BuildContext context, String? uri) {
    if (uri == null) return;
    unawaited(LxClipboard.copyTextWithFeedback(context, uri));
  }

  Future<void> onShare(BuildContext context, String? uri) async {
    if (uri == null) return;
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
        valueListenable: this.addressUri,
        builder: (context, uri, child) => ScrollableSinglePageBody(
          body: [
            const HeadingText(text: "Receive Bitcoin"),
            const SubheadingText(
              text: "Send Bitcoin to this address from any wallet.",
            ),

            const SizedBox(height: Space.s600),

            // Address QR code card
            _PaymentQrCard(
              uri: uri,
              onCopy: () => this.onCopy(context, uri),
              onShare: () => this.onShare(context, uri),
            ),

            const SizedBox(height: Space.s600),

            // Waiting indicator (only shown when QR is loaded)
            if (uri != null)
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
        ),
      ),
    );
  }
}
