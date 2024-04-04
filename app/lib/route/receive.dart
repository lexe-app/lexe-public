import 'dart:async' show unawaited;
import 'dart:math' show max;

import 'package:flutter/cupertino.dart' show CupertinoScrollBehavior;
import 'package:flutter/material.dart';

import 'package:lexeapp/address_format.dart' as address_format;
import 'package:lexeapp/bindings_generated_api.dart';
import 'package:lexeapp/components.dart'
    show
        CarouselIndicatorsAndButtons,
        FilledPlaceholder,
        HeadingText,
        LxBackButton,
        LxFilledButton,
        PaymentAmountInput,
        PaymentNoteInput,
        ScrollableSinglePageBody,
        SheetDragHandle,
        ValueStreamBuilder,
        baseInputDecoration;
import 'package:lexeapp/currency_format.dart';
import 'package:lexeapp/input_formatter.dart';
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/show_qr.dart' show QrImage;
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxRadius, LxTheme, Space;
import 'package:rxdart/rxdart.dart';

const double minViewportWidth = 365.0;

/// The inputs used to generate a [PaymentOffer].
class PaymentOfferInputs {
  const PaymentOfferInputs({
    required this.kind,
    required this.amountSats,
    required this.description,
  });

  final PaymentOfferKind kind;
  final int? amountSats;
  final String? description;
}

enum PaymentOfferKind {
  lightningInvoice,
  lightningOffer,
  // lightningSpontaneous,
  btcAddress,
  btcTaproot,
  ;

  bool isLightning() => switch (this) {
        PaymentOfferKind.lightningInvoice => true,
        PaymentOfferKind.lightningOffer => true,
        PaymentOfferKind.btcAddress => false,
        PaymentOfferKind.btcTaproot => false,
      };
}

class PaymentOffer {
  const PaymentOffer({
    required this.kind,
    required this.code,
    required this.uri,
    required this.amountSats,
    required this.description,
  });

  final PaymentOfferKind kind;

  final String? code;
  final String? uri;

  final int? amountSats;
  final String? description;

  String titleStr() => switch (this.kind) {
        PaymentOfferKind.lightningInvoice => "Lightning invoice",
        PaymentOfferKind.lightningOffer => "Lightning offer",
        // PaymentOfferKind.lightningSpontaneous => "Lightning spontaneous payment",
        PaymentOfferKind.btcAddress => "Bitcoin address",
        PaymentOfferKind.btcTaproot => "Bitcoin taproot address",
      };
}

class ReceivePaymentPage extends StatelessWidget {
  const ReceivePaymentPage({super.key, required this.fiatRate});

  /// Updating stream of fiat rates.
  final ValueStream<FiatRate?> fiatRate;

  @override
  Widget build(BuildContext context) => ReceivePaymentPageInner(
        viewportWidth:
            MediaQuery.maybeSizeOf(context)?.width ?? minViewportWidth,
        fiatRate: this.fiatRate,
      );
}

/// We need this extra intermediate "inner" widget so we can init the
/// [PageController] with a `viewportFraction` derived from the screen width.
class ReceivePaymentPageInner extends StatefulWidget {
  const ReceivePaymentPageInner(
      {super.key, required this.viewportWidth, required this.fiatRate});

  final double viewportWidth;

  final ValueStream<FiatRate?> fiatRate;

  @override
  State<ReceivePaymentPageInner> createState() =>
      ReceivePaymentPageInnerState();
}

class ReceivePaymentPageInnerState extends State<ReceivePaymentPageInner> {
  /// The current primary card on-screen.
  final ValueNotifier<int> selectedCardIndex = ValueNotifier(0);

  /// Controls the card [PageView].
  late PageController cardController = this.newCardController();

  final List<ValueNotifier<PaymentOffer>> paymentOffers = [
    ValueNotifier(
      const PaymentOffer(
        kind: PaymentOfferKind.lightningInvoice,
        code: null,
        uri: null,
        amountSats: null,
        description: null,
      ),
    ),
    ValueNotifier(
      const PaymentOffer(
        kind: PaymentOfferKind.btcAddress,
        code: null,
        uri: null,
        amountSats: null,
        description: null,
      ),
    ),
  ];

  @override
  void dispose() {
    for (final paymentOffer in this.paymentOffers) {
      paymentOffer.dispose();
    }
    this.cardController.dispose();
    this.selectedCardIndex.dispose();
    super.dispose();
  }

  @override
  void didUpdateWidget(ReceivePaymentPageInner oldWidget) {
    super.didUpdateWidget(oldWidget);

    if (this.widget.viewportWidth != oldWidget.viewportWidth) {
      final oldController = this.cardController;
      this.cardController = this.newCardController();
      oldController.dispose();
    }
  }

  PageController newCardController() => PageController(
        initialPage: this.selectedCardIndex.value,
        viewportFraction:
            minViewportWidth / max(minViewportWidth, this.widget.viewportWidth),
      );

  ValueNotifier<PaymentOffer> currentOffer() =>
      this.paymentOffers[this.selectedCardIndex.value];

  void openSettingsBottomSheet(BuildContext context) {
    unawaited(showModalBottomSheet(
      backgroundColor: LxColors.background,
      elevation: 0.0,
      clipBehavior: Clip.hardEdge,
      enableDrag: true,
      isDismissible: true,
      isScrollControlled: true,
      context: context,
      builder: (context) => ReceiveSettingsBottomSheet(
        kind: this.currentOffer().value.kind,
      ),
    ));
  }

  Future<void> onTapSetAmount() async {
    final PaymentOfferInputs? flowResult =
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_) => const ReceivePaymentSetAmountPage(),
    ));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(),
        title: const Text(
          "Receive payment",
          style: TextStyle(
            fontSize: Fonts.size500,
            fontVariations: [Fonts.weightMedium],
            letterSpacing: -0.5,
            height: 1.0,
          ),
        ),
      ),
      body: ScrollableSinglePageBody(
        padding: EdgeInsets.zero,
        body: [
          const SizedBox(height: Space.s500),

          // Payment offer card
          SizedBox(
            height: 545.0,
            child: PageView(
              controller: this.cardController,
              scrollBehavior: const CupertinoScrollBehavior(),
              padEnds: true,
              allowImplicitScrolling: false,
              onPageChanged: (pageIndex) {
                if (!this.mounted) return;
                this.selectedCardIndex.value = pageIndex;
              },
              children: this
                  .paymentOffers
                  .map((offer) => ValueListenableBuilder(
                        valueListenable: offer,
                        builder: (_context, offer, _child) => PaymentOfferCard(
                          paymentOffer: offer,
                          fiatRate: this.widget.fiatRate,
                        ),
                      ))
                  .toList(),
            ),
          ),

          const SizedBox(height: Space.s400),

          Padding(
            padding: const EdgeInsets.symmetric(horizontal: Space.s600),
            child: CarouselIndicatorsAndButtons(
              numPages: this.paymentOffers.length,
              selectedPageIndex: this.selectedCardIndex,
              onTapPrev: () => unawaited(this.cardController.previousPage(
                  duration: const Duration(milliseconds: 500),
                  curve: Curves.ease)),
              onTapNext: () => unawaited(this.cardController.nextPage(
                  duration: const Duration(milliseconds: 500),
                  curve: Curves.ease)),
            ),
          ),

          const SizedBox(height: Space.s200),

          Padding(
            padding: const EdgeInsets.symmetric(horizontal: Space.s600),
            child: Row(
              children: [
                LxFilledButton(
                  icon: const Icon(Icons.settings_rounded),
                  onTap: () => this.openSettingsBottomSheet(context),
                ),
                const SizedBox(width: Space.s200),
                LxFilledButton(
                  icon: const Icon(Icons.share_rounded),
                  onTap: () {},
                ),
                const SizedBox(width: Space.s200),
                Expanded(
                  child: LxFilledButton(
                    label: const Text("Amount"),
                    icon: const Icon(Icons.add_rounded),
                    onTap: this.onTapSetAmount,
                  ),
                ),
              ],
            ),
          ),

          const SizedBox(height: Space.s400),
        ],
      ),
    );
  }
}

class PaymentOfferCard extends StatelessWidget {
  const PaymentOfferCard(
      {super.key, required this.paymentOffer, required this.fiatRate});

  final PaymentOffer paymentOffer;
  final ValueStream<FiatRate?> fiatRate;

  @override
  Widget build(BuildContext context) {
    final code = this.paymentOffer.code;
    // final code = "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r";
    // final code =
    //     "lno1pqps7sjqpgtyzm3qv4uxzmtsd3jjqer9wd3hy6tsw35k7msjzfpy7nz5yqcnygrfdej82um5wf5k2uckyypwa3eyt44h6txtxquqh7lz5djge4afgfjn7k4rgrkuag0jsd5xvxg";
    // final code = "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r";

    final amountSats = this.paymentOffer.amountSats;
    // final amountSats = 5300;
    final amountSatsStr = (amountSats != null)
        ? formatSatsAmount(amountSats, satsSuffix: false)
        : null;

    final description = this.paymentOffer.description;
    // final description = "the rice house 🍕";

    return CardBox(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.start,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // kind
          Text(
            this.paymentOffer.titleStr(),
            style: const TextStyle(
              color: LxColors.foreground,
              fontSize: Fonts.size300,
              fontVariations: [Fonts.weightMedium],
              letterSpacing: -0.5,
              height: 1.0,
            ),
          ),

          // raw code string + copy button
          if (code != null)
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  address_format.ellipsizeBtcAddress(code),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: Fonts.size100,
                    color: LxColors.fgTertiary,
                    height: 1.0,
                  ),
                ),
                IconButton(
                  onPressed: () {},
                  icon: const Icon(
                    Icons.copy_rounded,
                    // size: Fonts.size300,
                  ),
                  color: LxColors.fgTertiary,
                  visualDensity:
                      const VisualDensity(horizontal: -4.0, vertical: -4.0),
                  padding: EdgeInsets.zero,
                  iconSize: Fonts.size300,
                  // style: IconButton.styleFrom(fixedSize: Size.square(20.0)),
                ),
              ],
            ),
          if (code == null)
            const Padding(
              padding: EdgeInsets.symmetric(vertical: 10.0),
              child: FilledPlaceholder(
                width: Space.s900,
                forText: true,
                height: Fonts.size100,
              ),
            ),
          // const SizedBox(height: Space.s100),

          // QR code
          LayoutBuilder(
            builder: (context, constraints) {
              final double dim = constraints.maxWidth;
              if (code != null) {
                return QrImage(
                  value: code,
                  dimension: dim.toInt(),
                  color: LxColors.foreground,
                );
              } else {
                return FilledPlaceholder(width: dim, height: dim);
              }
            },
          ),

          if (amountSatsStr != null || description != null)
            const SizedBox(height: Space.s400),

          // Amount (sats)
          if (amountSatsStr != null)
            Padding(
              padding: const EdgeInsets.only(bottom: Space.s100),
              child: Text.rich(
                TextSpan(
                  children: [
                    TextSpan(text: amountSatsStr),
                    const TextSpan(
                        text: " sats",
                        style: TextStyle(color: LxColors.grey550)),
                  ],
                  style: const TextStyle(
                    fontSize: Fonts.size600,
                    letterSpacing: -0.5,
                    fontVariations: [Fonts.weightMedium],
                    height: 1.0,
                  ),
                ),
              ),
            ),

          // Amount (fiat)
          ValueStreamBuilder(
            stream: this.fiatRate,
            builder: (context, fiatRate) {
              if (amountSats == null) return const SizedBox.shrink();

              final String? amountFiatStr;
              if (fiatRate != null) {
                final amountFiat = fiatRate.rate * satsToBtc(amountSats);
                amountFiatStr = formatFiat(amountFiat, fiatRate.fiat);
              } else {
                amountFiatStr = null;
              }

              const fontSize = Fonts.size400;

              return (amountFiatStr != null)
                  ? Text(
                      "≈ $amountFiatStr",
                      style: const TextStyle(
                        color: LxColors.fgTertiary,
                        fontSize: fontSize,
                        letterSpacing: -0.5,
                        height: 1.0,
                      ),
                    )
                  : const FilledPlaceholder(
                      height: fontSize,
                      width: Space.s900,
                      forText: true,
                    );
            },
          ),

          if (amountSatsStr != null && description != null)
            const SizedBox(height: Space.s400),

          // Description
          if (description != null)
            Text(
              description,
              style: const TextStyle(
                color: LxColors.foreground,
                fontSize: Fonts.size200,
                height: 1.5,
                letterSpacing: -0.5,
              ),
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
            ),
        ],
      ),
    );
  }
}

/// Rounded card styling.
class CardBox extends StatelessWidget {
  const CardBox({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisAlignment: MainAxisAlignment.start,
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: Space.s200),
          child: Container(
            decoration: BoxDecoration(
              color: LxColors.grey1000,
              borderRadius: BorderRadius.circular(LxRadius.r300),
            ),
            clipBehavior: Clip.antiAlias,
            padding: const EdgeInsets.fromLTRB(
                Space.s500, Space.s500, Space.s500, Space.s500),
            constraints: const BoxConstraints(maxWidth: 350.0),
            child: this.child,
          ),
        ),
        const Expanded(child: Center()),
      ],
    );
  }
}

const bottomSheetBodyPadding = Space.s600;

class ReceiveSettingsBottomSheet extends StatelessWidget {
  const ReceiveSettingsBottomSheet({super.key, required this.kind});

  final PaymentOfferKind kind;

  void onKindSelected(PaymentOfferKind kind) {
    info("ReceiveSettingsBottomSheet: selected kind: $kind");
  }

  @override
  Widget build(BuildContext context) {
    return Theme(
      data: LxTheme.light(),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const SheetDragHandle(),
          const SizedBox(height: Space.s200),
          const Padding(
            padding: EdgeInsets.symmetric(
                horizontal: bottomSheetBodyPadding, vertical: Space.s300),
            child: HeadingText(text: "Receive settings"),
          ),

          // Lightning
          if (this.kind.isLightning())
            PaymentOfferKindRadio(
              kind: PaymentOfferKind.lightningInvoice,
              selected: this.kind,
              title: const Text("Lightning invoice"),
              subtitle: const Text(
                  "Widely supported. Invoices can only be paid once!"),
              onChanged: this.onKindSelected,
            ),
          if (this.kind.isLightning())
            PaymentOfferKindRadio(
              kind: PaymentOfferKind.lightningOffer,
              selected: this.kind,
              title: const Text("Lightning offer"),
              subtitle: const Text(
                  "New. Offers can be paid many times. Paste one on your twitter!"),
              onChanged: this.onKindSelected,
            ),

          // BTC
          if (!this.kind.isLightning())
            PaymentOfferKindRadio(
              kind: PaymentOfferKind.btcAddress,
              selected: this.kind,
              title: const Text("Bitcoin SegWit address"),
              subtitle: const Text("Recommended. Supported by most wallets."),
              onChanged: this.onKindSelected,
            ),
          if (!this.kind.isLightning())
            PaymentOfferKindRadio(
              kind: PaymentOfferKind.btcTaproot,
              selected: this.kind,
              title: const Text("Bitcoin Taproot address"),
              subtitle: const Text(
                  "Newer format. Reduced fees and increased privacy."),
              onChanged: this.onKindSelected,
            ),
          const SizedBox(height: Space.s600),
        ],
      ),
    );
  }
}

class PaymentOfferKindRadio extends StatelessWidget {
  const PaymentOfferKindRadio({
    super.key,
    required this.kind,
    required this.selected,
    required this.title,
    required this.subtitle,
    required this.onChanged,
  });

  final PaymentOfferKind kind;
  final PaymentOfferKind selected;

  final Widget title;
  final Widget subtitle;

  final void Function(PaymentOfferKind)? onChanged;

  @override
  Widget build(BuildContext context) {
    final onChanged = this.onChanged;

    return RadioListTile<PaymentOfferKind>(
      toggleable: false,
      controlAffinity: ListTileControlAffinity.trailing,
      contentPadding:
          const EdgeInsets.symmetric(horizontal: bottomSheetBodyPadding),
      value: this.kind,
      groupValue: this.selected,
      onChanged: (onChanged != null) ? (kind) => onChanged(kind!) : null,
      title: this.title,
      subtitle: this.subtitle,
    );
  }
}

/// A page for the user to set a desired amount and optional description on
/// their payment offer.
class ReceivePaymentSetAmountPage extends StatefulWidget {
  const ReceivePaymentSetAmountPage({super.key});

  @override
  State<ReceivePaymentSetAmountPage> createState() =>
      _ReceivePaymentSetAmountPageState();
}

class _ReceivePaymentSetAmountPageState
    extends State<ReceivePaymentSetAmountPage> {
  final GlobalKey<FormFieldState<String>> amountFieldKey = GlobalKey();
  final GlobalKey<FormFieldState<String>> descriptionFieldKey = GlobalKey();

  final IntInputFormatter intInputFormatter = IntInputFormatter();

  Result<int, String?> validateAmountStr(String? maybeAmountStr) {
    if (maybeAmountStr == null || maybeAmountStr.isEmpty) {
      return const Err(null);
    }

    final int amount;
    switch (this.intInputFormatter.tryParse(maybeAmountStr)) {
      case Ok(:final ok):
        amount = ok;
      case Err():
        return const Err("Amount must be a number.");
    }

    // Don't show any error message if the field is effectively empty.
    if (amount <= 0) {
      return const Err(null);
    }

    return Ok(amount);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(),
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Set receive amount"),
          const SizedBox(height: Space.s850),

          // <amount> sats
          PaymentAmountInput(
            fieldKey: this.amountFieldKey,
            intInputFormatter: this.intInputFormatter,
          ),

          const SizedBox(height: Space.s700),

          PaymentNoteInput(fieldKey: this.descriptionFieldKey, onSubmit: () {}),
        ],
      ),
    );
  }
}
