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
        ValueStreamBuilder;
import 'package:lexeapp/currency_format.dart';
import 'package:lexeapp/input_formatter.dart';
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/show_qr.dart' show QrImage;
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxRadius, LxTheme, Space;
import 'package:rxdart/rxdart.dart';

const double minViewportWidth = 365.0;

const int lnPageIdx = 0;
const int btcPageIdx = 1;

/// The inputs used to generate a [PaymentOffer].
class PaymentOfferInputs {
  const PaymentOfferInputs({
    required this.kindByPage,
    required this.amountSats,
    required this.description,
  });

  final List<PaymentOfferKind> kindByPage;
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

  bool isBtc() => !this.isLightning();
}

@immutable
class PaymentOffer {
  const PaymentOffer({
    required this.kind,
    required this.code,
    required this.amountSats,
    required this.description,
    required this.expiresAt,
  });

  final PaymentOfferKind kind;
  final String? code;
  final int? amountSats;
  final String? description;
  final DateTime? expiresAt;

  String titleStr() => switch (this.kind) {
        PaymentOfferKind.lightningInvoice => "Lightning invoice",
        PaymentOfferKind.lightningOffer => "Lightning offer",
        // PaymentOfferKind.lightningSpontaneous => "Lightning spontaneous payment",
        PaymentOfferKind.btcAddress => "Bitcoin address",
        PaymentOfferKind.btcTaproot => "Bitcoin taproot address",
      };

  // TODO(phlip9): do this in rust, more robustly. Also uppercase for QR
  // encoding.
  String? uri() {
    final code = this.code;
    if (code == null) return null;

    final amountSats = this.amountSats;
    final description = this.description;

    if (this.kind.isLightning()) {
      return "lightning:$code";
    } else {
      final base = "bitcoin:$code";
      final params = [
        if (amountSats != null) "amount=${formatSatsToBtcForUri(amountSats)}",
        if (description != null) "message=$description",
      ];
      final String paramsStr;
      if (params.isNotEmpty) {
        paramsStr = "?${params.join('&')}";
      } else {
        paramsStr = "";
      }
      return "$base$paramsStr";
    }
  }

  PaymentOffer clone() => PaymentOffer(
        kind: this.kind,
        code: this.code,
        amountSats: this.amountSats,
        description: this.description,
        expiresAt: this.expiresAt,
      );

  @override
  String toString() {
    return 'PaymentOffer(kind: $kind, code: $code, amountSats: $amountSats, description: $description, expiresAt: $expiresAt)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == this.runtimeType &&
            other is PaymentOffer &&
            (identical(other.kind, this.kind) || other.kind == this.kind) &&
            (identical(other.code, this.code) || other.code == this.code) &&
            (identical(other.amountSats, this.amountSats) ||
                other.amountSats == this.amountSats) &&
            (identical(other.description, this.description) ||
                other.description == this.description) &&
            (identical(other.expiresAt, this.expiresAt) ||
                other.expiresAt == this.expiresAt));
  }

  @override
  int get hashCode => Object.hash(this.runtimeType, this.kind, this.code,
      this.amountSats, this.description, this.expiresAt);
}

class ReceivePaymentPage extends StatelessWidget {
  const ReceivePaymentPage({
    super.key,
    required this.app,
    required this.fiatRate,
  });

  final AppHandle app;

  /// Updating stream of fiat rates.
  final ValueStream<FiatRate?> fiatRate;

  @override
  Widget build(BuildContext context) => ReceivePaymentPageInner(
        app: this.app,
        fiatRate: this.fiatRate,
        viewportWidth:
            MediaQuery.maybeSizeOf(context)?.width ?? minViewportWidth,
      );
}

/// We need this extra intermediate "inner" widget so we can init the
/// [PageController] with a `viewportFraction` derived from the screen width.
class ReceivePaymentPageInner extends StatefulWidget {
  const ReceivePaymentPageInner({
    super.key,
    required this.app,
    required this.fiatRate,
    required this.viewportWidth,
  });

  final AppHandle app;
  final ValueStream<FiatRate?> fiatRate;

  final double viewportWidth;

  @override
  State<ReceivePaymentPageInner> createState() =>
      ReceivePaymentPageInnerState();
}

class ReceivePaymentPageInnerState extends State<ReceivePaymentPageInner> {
  /// The current primary card on-screen.
  final ValueNotifier<int> selectedCardIndex = ValueNotifier(0);

  /// Controls the card [PageView].
  late PageController cardController = this.newCardController();

  final ValueNotifier<PaymentOfferInputs> paymentOfferInputs = ValueNotifier(
    const PaymentOfferInputs(
      kindByPage: [
        PaymentOfferKind.lightningInvoice,
        PaymentOfferKind.btcAddress,
      ],
      amountSats: null,
      description: null,
    ),
  );

  final List<ValueNotifier<PaymentOffer>> paymentOffers = [
    ValueNotifier(
      const PaymentOffer(
        kind: PaymentOfferKind.lightningInvoice,
        code: null,
        amountSats: null,
        description: null,
        expiresAt: null,
      ),
    ),
    ValueNotifier(
      const PaymentOffer(
        kind: PaymentOfferKind.btcAddress,
        code: null,
        amountSats: null,
        description: null,
        expiresAt: null,
      ),
    ),
  ];

  @override
  void initState() {
    super.initState();

    this.paymentOfferInputs.addListener(this.doFetchLn);
    this.paymentOfferInputs.addListener(this.doFetchBtc);

    unawaited(this.doFetchLn());
    unawaited(this.doFetchBtc());
  }

  @override
  void dispose() {
    this.paymentOfferInputs.dispose();
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
  ValueNotifier<PaymentOffer> lnOffer() => this.paymentOffers[lnPageIdx];
  ValueNotifier<PaymentOffer> btcOffer() => this.paymentOffers[btcPageIdx];

  /// Fetch a bitcoin address for the given [PaymentOfferInputs] and return a
  /// full [PaymentOffer].
  ///
  /// Will skip actually sending a new request if only the `inputs.amountSats`
  /// or `inputs.description` changed.
  Future<PaymentOffer?> fetchBtcOffer(
    PaymentOfferInputs inputs,
    PaymentOffer prev,
  ) async {
    final btcKind = inputs.kindByPage[btcPageIdx];

    // sanity check
    assert(btcKind.isBtc());

    // TODO(phlip9): actually add ability to fetch a taproot address
    assert(btcKind != PaymentOfferKind.btcTaproot);

    info("ReceivePaymentPage: fetchBtcOffer: kind: $btcKind, prev: $prev");

    // We only need to fetch a new address code if the address kind changed.
    // Otherwise, we can skip the extra request to the user's node.
    if (prev.code != null && prev.kind == btcKind) {
      return PaymentOffer(
        kind: prev.kind,
        code: prev.code,
        expiresAt: prev.expiresAt,

        // Just update the amount/description
        amountSats: inputs.amountSats,
        description: inputs.description,
      );
    }

    final result = await Result.tryFfiAsync(this.widget.app.getAddress);

    final String address;
    switch (result) {
      case Err(:final err):
        // TODO(phlip9): error display
        error("ReceivePaymentPage: fetchBtcOffer: failed to getAddress: $err");
        return null;

      case Ok(:final ok):
        address = ok;
        info("ReceivePaymentPage: fetchBtcOffer: getAddress => '$address'");
    }

    return PaymentOffer(
      kind: btcKind,
      code: address,
      amountSats: prev.amountSats,
      description: prev.description,
      expiresAt: prev.expiresAt,
    );
  }

  /// Fetch the Lightning invoice/offer for the given `PaymentOfferInputs`.
  Future<PaymentOffer?> fetchLnOffer(
    PaymentOfferInputs inputs,
    PaymentOffer prev,
  ) async {
    final lnKind = inputs.kindByPage[0];

    // sanity check
    assert(lnKind.isLightning());
    // TODO(phlip9): actually support BOLT12 offers.
    assert(lnKind == PaymentOfferKind.lightningInvoice);

    final req = CreateInvoiceRequest(
      // TODO(phlip9): choose a good default expiration
      expirySecs: 3600,
      amountSats: inputs.amountSats,
      description: inputs.description,
    );

    info(
        "ReceivePaymentPage: doFetchLn: kind: $lnKind, req: { amountSats: ${req.amountSats}, exp: ${req.expirySecs} }");

    final result =
        await Result.tryFfiAsync(() => this.widget.app.createInvoice(req: req));

    final Invoice invoice;
    switch (result) {
      case Err(:final err):
        // TODO(phlip9): error display
        error(
            "ReceivePaymentPage: doFetchLn: failed to create invoice: $err, req: req: { amountStas: ${req.amountSats}, exp: ${req.expirySecs} }");
        return null;

      case Ok(:final ok):
        invoice = ok.invoice;
        info("ReceivePaymentPage: doFetchLn: createInvoice => done");
    }

    return PaymentOffer(
      kind: lnKind,
      code: invoice.string,
      amountSats: invoice.amountSats,
      description: invoice.description,
      expiresAt: DateTime.fromMillisecondsSinceEpoch(invoice.expiresAt),
    );
  }

  Future<void> doFetchBtc() async {
    final inputs = this.paymentOfferInputs.value;
    final btcOfferNotifier = this.btcOffer();
    final prev = btcOfferNotifier.value;

    final offer = await this.fetchBtcOffer(inputs, prev);

    // Canceled / navigated away => ignore
    if (!this.mounted) return;

    // Error => ignore (TODO: handle)
    if (offer == null) return;

    // Stale request => ignore
    if (prev != btcOfferNotifier.value) {
      info("ReceivePaymentPage: doFetchBtc: stale request, ignoring response");
      return;
    }

    btcOfferNotifier.value = offer;
  }

  Future<void> doFetchLn() async {
    final inputs = this.paymentOfferInputs.value;
    final lnOfferNotifier = this.lnOffer();
    final prev = lnOfferNotifier.value;

    final offer = await this.fetchLnOffer(inputs, prev);

    // Canceled / navigated away => ignore
    if (!this.mounted) return;

    // Error => ignore (TODO: handle)
    if (offer == null) return;

    // Stale request => ignore
    if (prev != lnOfferNotifier.value) {
      info("ReceivePaymentPage: doFetchLn: stale request, ignoring response");
      return;
    }

    lnOfferNotifier.value = offer;
  }

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
    // final PaymentOfferInputs? _flowResult =
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
    // final code = "bcrt1q2nfxmhd4n3c8834pj72xagvyr9gl57n5r94fsl";
    // final code = null;

    final uri = this.paymentOffer.uri();
    // final uri = "lightning:lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r";
    // final uri =
    //     "lightning:lno1pqps7sjqpgtyzm3qv4uxzmtsd3jjqer9wd3hy6tsw35k7msjzfpy7nz5yqcnygrfdej82um5wf5k2uckyypwa3eyt44h6txtxquqh7lz5djge4afgfjn7k4rgrkuag0jsd5xvxg";
    // final uri = "lightning:lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r";
    // final uri = "bitcoin:bcrt1q2nfxmhd4n3c8834pj72xagvyr9gl57n5r94fsl";
    // final uri = null;

    final amountSats = this.paymentOffer.amountSats;
    // final amountSats = 5300;
    // final amountSats = null;
    final amountSatsStr = (amountSats != null)
        ? formatSatsAmount(amountSats, satsSuffix: false)
        : null;

    final description = this.paymentOffer.description;
    // final description = "the rice house 🍕";
    // final description = null;

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
              final key = ValueKey(uri ?? "");

              return AnimatedSwitcher(
                duration: const Duration(milliseconds: 250),
                child: (uri != null)
                    ? QrImage(
                        // `AnimatedSwitcher` should also run the switch
                        // animation when the QR code contents change.
                        key: key,
                        value: uri,
                        dimension: dim.toInt(),
                        color: LxColors.foreground,
                      )
                    : FilledPlaceholder(key: key, width: dim, height: dim),
              );
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

          const SizedBox(height: Space.s850),

          PaymentNoteInput(fieldKey: this.descriptionFieldKey, onSubmit: () {}),
        ],
        bottom: LxFilledButton(
          label: const Text("Confirm"),
          icon: const Icon(Icons.arrow_forward_rounded),
          onTap: () {},
        ),
      ),
    );
  }
}
