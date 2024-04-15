import 'dart:async' show unawaited;
import 'dart:math' show max;

import 'package:flutter/cupertino.dart' show CupertinoScrollBehavior;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show Clipboard, ClipboardData;
import 'package:flutter_markdown/flutter_markdown.dart';
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
        SubheadingText,
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
@immutable
class PaymentOfferInputs {
  const PaymentOfferInputs({
    required this.kindByPage,
    required this.amountSats,
    required this.description,
  });

  final List<PaymentOfferKind> kindByPage;
  final int? amountSats;
  final String? description;

  @override
  String toString() {
    return 'PaymentOfferInputs(kindByPage: $kindByPage, amountSats: $amountSats, description: $description)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == this.runtimeType &&
            other is PaymentOfferInputs &&
            (identical(other.kindByPage, this.kindByPage) ||
                other.kindByPage == this.kindByPage) &&
            (identical(other.amountSats, this.amountSats) ||
                other.amountSats == this.amountSats) &&
            (identical(other.description, this.description) ||
                other.description == this.description));
  }

  @override
  int get hashCode => Object.hash(
      this.runtimeType, this.kindByPage, this.amountSats, this.description);
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
    // assert(btcKind != PaymentOfferKind.btcTaproot);

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
    // assert(lnKind == PaymentOfferKind.lightningInvoice);

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

  /// Open the [ReceiveSettingsBottomSheet] for the user to modify the current
  /// page's receive offer settings.
  Future<void> openSettingsBottomSheet(BuildContext context) async {
    final PaymentOfferKind? kind = await showModalBottomSheet<PaymentOfferKind>(
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
    );

    if (!this.mounted || kind == null) return;

    final offerNotifier = this.currentOffer();
    final prevOffer = offerNotifier.value;
    offerNotifier.value = PaymentOffer(
      amountSats: prevOffer.amountSats,
      description: prevOffer.description,
      // Update these fields. We'll unset the code to prevent accidentally
      // scanning the old QR and indicate that the new QR is loading.
      kind: kind,
      code: null,
      expiresAt: null,
    );

    final pageIdx = this.selectedCardIndex.value;
    final prevInputs = this.paymentOfferInputs.value;
    this.paymentOfferInputs.value = PaymentOfferInputs(
      // Update the new desired offer kind for the current page.
      kindByPage: (pageIdx == 0)
          ? ([kind, prevInputs.kindByPage[1]])
          : ([prevInputs.kindByPage[0], kind]),
      amountSats: prevInputs.amountSats,
      description: prevInputs.description,
    );
  }

  Future<void> onTapSetAmount() async {
    final prev = this.paymentOfferInputs.value;
    final prevAD = (amountSats: prev.amountSats, description: prev.description);

    final ({int? amountSats, String? description})? flowResult =
        await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) => ReceivePaymentSetAmountPage(
          prevAmountSats: prevAD.amountSats,
          prevDescription: prevAD.description,
        ),
      ),
    );

    if (!this.mounted || flowResult == null || flowResult == prevAD) return;

    // LN invoice needs to be reloaded.
    final lnOffer = this.lnOffer().value;
    this.lnOffer().value = PaymentOffer(
      kind: lnOffer.kind,
      code: null,
      expiresAt: null,
      amountSats: flowResult.amountSats,
      description: flowResult.description,
    );

    this.paymentOfferInputs.value = PaymentOfferInputs(
      kindByPage: prev.kindByPage,
      amountSats: flowResult.amountSats,
      description: flowResult.description,
    );
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
            height: 575.0,
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
                const Expanded(child: Center()),
                const SizedBox(width: Space.s200),
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
                const Expanded(child: Center()),
                // Expanded(
                //   child: LxFilledButton(
                //     label: const Text("Amount"),
                //     icon: const Icon(Icons.add_rounded),
                //     onTap: this.onTapSetAmount,
                //   ),
                // ),
              ],
            ),
          ),

          const SizedBox(height: Space.s400),
        ],
      ),
    );
  }
}

class ReceivePaymentPage2 extends StatelessWidget {
  const ReceivePaymentPage2({super.key});

  @override
  Widget build(BuildContext context) {
    final viewportWidth =
        MediaQuery.maybeSizeOf(context)?.width ?? minViewportWidth;

    const PaymentOffer paymentOffer1 = PaymentOffer(
      kind: PaymentOfferKind.lightningInvoice,
      code:
          "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
      amountSats: null,
      description: null,
      expiresAt: null,
    );

    const PaymentOffer paymentOffer2 = PaymentOffer(
      kind: PaymentOfferKind.lightningInvoice,
      code:
          "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
      // amountSats: 45750,
      amountSats: null,
      description: "the rice house 🍕",
      expiresAt: null,
    );

    const FiatRate fiatRate = FiatRate(fiat: "USD", rate: 69123.45);
    final ValueStream<FiatRate?> fiatRates =
        Stream.fromIterable(<FiatRate?>[fiatRate]).shareValueSeeded(fiatRate);

    final pageController = PageController(
      initialPage: 0,
      viewportFraction: minViewportWidth / max(minViewportWidth, viewportWidth),
    );

    final selectedPageIndex = ValueNotifier(0);

    final pages = <Widget>[
      PaymentOfferCard(paymentOffer: paymentOffer1, fiatRate: fiatRates),
      PaymentOfferCard(paymentOffer: paymentOffer2, fiatRate: fiatRates),
      PaymentOfferCard2(paymentOffer: paymentOffer1, fiatRate: fiatRates),
      PaymentOfferCard2(paymentOffer: paymentOffer2, fiatRate: fiatRates),
      PaymentOfferCard3(paymentOffer: paymentOffer1, fiatRate: fiatRates),
      PaymentOfferCard3(paymentOffer: paymentOffer2, fiatRate: fiatRates),
      PaymentOfferCard4(paymentOffer: paymentOffer1, fiatRate: fiatRates),
      PaymentOfferCard4(paymentOffer: paymentOffer2, fiatRate: fiatRates),
      PaymentOfferCard5(paymentOffer: paymentOffer1, fiatRate: fiatRates),
      PaymentOfferCard5(paymentOffer: paymentOffer2, fiatRate: fiatRates),
    ];

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(),
        // title: const Text(
        //   "Receive payment",
        //   style: TextStyle(
        //     color: LxColors.fgTertiary,
        //     fontSize: Fonts.size500,
        //     fontVariations: [Fonts.weightMedium],
        //     letterSpacing: -0.5,
        //     height: 1.0,
        //   ),
        // ),
      ),
      body: ScrollableSinglePageBody(
        padding: EdgeInsets.zero,
        body: [
          const SizedBox(height: Space.s200),

          // Payment offer card
          SizedBox(
            height: 650.0,
            // height: 575.0,
            child: PageView(
              controller: pageController,
              scrollBehavior: const CupertinoScrollBehavior(),
              padEnds: true,
              allowImplicitScrolling: false,
              onPageChanged: (pageIdx) => selectedPageIndex.value = pageIdx,
              children: pages,
            ),
          ),

          const SizedBox(height: Space.s400),

          // const SizedBox(height: Space.s200),
          //
          // Padding(
          //   padding: const EdgeInsets.symmetric(horizontal: Space.s600),
          //   child: Row(
          //     children: [
          //       const Expanded(child: Center()),
          //       const SizedBox(width: Space.s200),
          //       LxFilledButton(
          //         icon: const Icon(Icons.settings_rounded),
          //         onTap: () {},
          //       ),
          //       const SizedBox(width: Space.s200),
          //       LxFilledButton(
          //         icon: const Icon(Icons.share_rounded),
          //         onTap: () {},
          //       ),
          //       const SizedBox(width: Space.s200),
          //       const Expanded(child: Center()),
          //       // Expanded(
          //       //   child: LxFilledButton(
          //       //     label: const Text("Amount"),
          //       //     icon: const Icon(Icons.add_rounded),
          //       //     onTap: this.onTapSetAmount,
          //       //   ),
          //       // ),
          //     ],
          //   ),
          // ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.symmetric(horizontal: Space.s600),
          child: CarouselIndicatorsAndButtons(
            numPages: pages.length,
            selectedPageIndex: selectedPageIndex,
          ),
        ),
      ),
    );
  }
}

class PaymentOfferCard extends StatelessWidget {
  const PaymentOfferCard(
      {super.key, required this.paymentOffer, required this.fiatRate});

  final PaymentOffer paymentOffer;
  final ValueStream<FiatRate?> fiatRate;

  void showSnackBarOnCopySuccess(BuildContext context) {
    if (!context.mounted) return;
    ScaffoldMessenger.of(context)
        .showSnackBar(const SnackBar(content: Text("Copied to clipboard.")));
  }

  void showSnackBarOnCopyError(BuildContext context, Object err) {
    if (!context.mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text("Failed to copy to clipboard: $err")));
  }

  /// Copy the current card's offer code to the user clipboard.
  void onTapCopy(BuildContext context) {
    final code = this.paymentOffer.code;
    if (code == null) return;
    unawaited(
      Clipboard.setData(ClipboardData(text: code))
          .then((_) => this.showSnackBarOnCopySuccess(context))
          .catchError((err) => this.showSnackBarOnCopyError(context, err)),
    );
  }

  @override
  Widget build(BuildContext context) {
    final code = this.paymentOffer.code;
    // final code = null;
    // final code = "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r";
    // final code =
    //     "lno1pqps7sjqpgtyzm3qv4uxzmtsd3jjqer9wd3hy6tsw35k7msjzfpy7nz5yqcnygrfdej82um5wf5k2uckyypwa3eyt44h6txtxquqh7lz5djge4afgfjn7k4rgrkuag0jsd5xvxg";
    // final code = "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r";
    // final code = "bcrt1q2nfxmhd4n3c8834pj72xagvyr9gl57n5r94fsl";

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
            TextButton.icon(
              onPressed: () => this.onTapCopy(context),
              icon: Text(
                address_format.ellipsizeBtcAddress(code),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(
                  fontSize: Fonts.size100,
                  color: LxColors.grey550,
                  height: 1.0,
                ),
              ),
              label: const Icon(
                Icons.copy_rounded,
                size: Fonts.size300,
                color: LxColors.grey550,
              ),
              style: ButtonStyle(
                padding: const MaterialStatePropertyAll(EdgeInsets.zero),
                visualDensity:
                    const VisualDensity(horizontal: -3.0, vertical: -3.0),
                shape: MaterialStatePropertyAll(RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(LxRadius.r200))),
              ),
            ),
          if (code == null)
            const Padding(
              padding: EdgeInsets.symmetric(vertical: Space.s200),
              child: FilledPlaceholder(
                width: Space.s900,
                forText: true,
                height: Fonts.size100,
                color: LxColors.background,
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
                    : FilledPlaceholder(
                        key: key,
                        width: dim,
                        height: dim,
                        color: LxColors.background,
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
                      color: LxColors.background,
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

          if (description == null && amountSatsStr == null)
            Padding(
              padding: const EdgeInsets.only(top: Space.s400),
              child: OutlinedButton(
                onPressed: () {},
                style: const ButtonStyle(
                  // shape: MaterialStatePropertyAll(RoundedRectangleBorder(
                  //     borderRadius: BorderRadius.all(
                  //         Radius.circular(LxRadius.r200)))),
                  // side: MaterialStatePropertyAll(BorderSide(
                  //   color: LxColors.foreground,
                  //   width: 2.0,
                  // )),

                  padding: MaterialStatePropertyAll(
                    EdgeInsets.symmetric(
                        vertical: Space.s200, horizontal: Space.s600),
                  ),
                  visualDensity: VisualDensity.compact,
                  // textStyle: MaterialStatePropertyAll(TextStyle(
                  //   color: LxColors.foreground,
                  //   fontSize: Fonts.size300,
                  //   fontVariations: [Fonts.weightBold],
                  // )),
                  // fixedSize: MaterialStatePropertyAll(Size.fromHeight(44.0)),
                ),
                child: const Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      SizedBox(width: Space.s200),
                      Text(
                        "Amount",
                        style: TextStyle(
                          fontSize: Fonts.size300,
                        ),
                      ),
                      SizedBox(width: Space.s200),
                      Icon(Icons.add_rounded),
                    ]),
              ),
            ),
        ],
      ),
    );
  }
}

class PaymentOfferCard2 extends StatelessWidget {
  const PaymentOfferCard2(
      {super.key, required this.paymentOffer, required this.fiatRate});

  final PaymentOffer paymentOffer;
  final ValueStream<FiatRate?> fiatRate;

  @override
  Widget build(BuildContext context) {
    final code = this.paymentOffer.code;
    final uri = this.paymentOffer.uri();
    // final uri = null;
    final amountSats = this.paymentOffer.amountSats;
    final amountSatsStr = (amountSats != null)
        ? formatSatsAmount(amountSats, satsSuffix: false)
        : null;
    final description = this.paymentOffer.description;

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
            padding: const EdgeInsets.all(Space.s450),
            constraints: const BoxConstraints(maxWidth: 350.0),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.start,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                // QR code
                LayoutBuilder(
                  builder: (context, constraints) {
                    final double dim = constraints.maxWidth;
                    final key = ValueKey(uri ?? "");

                    return AnimatedSwitcher(
                      duration: const Duration(milliseconds: 250),
                      child: (uri != null)
                          ? Container(
                              decoration: BoxDecoration(
                                  borderRadius: BorderRadius.circular(6.0)),
                              clipBehavior: Clip.hardEdge,
                              child: QrImage(
                                // `AnimatedSwitcher` should also run the switch
                                // animation when the QR code contents change.
                                key: key,
                                value: uri,
                                dimension: dim.toInt(),
                                color: LxColors.foreground,
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
              ],
            ),
          ),
        ),

        // Space
        const SizedBox(height: Space.s400),

        // Info
        Container(
          decoration: BoxDecoration(
            color: LxColors.grey1000,
            borderRadius: BorderRadius.circular(LxRadius.r300),
          ),
          clipBehavior: Clip.antiAlias,
          padding: const EdgeInsets.fromLTRB(
              Space.s450, Space.s400, Space.s450, Space.s450),
          constraints: const BoxConstraints(maxWidth: 350.0),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.start,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const SizedBox(width: 350.0 - 2 * Space.s400),
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
                TextButton.icon(
                  onPressed: () {},
                  icon: Text(
                    address_format.ellipsizeBtcAddress(code),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: Fonts.size100,
                      color: LxColors.grey550,
                      height: 1.0,
                    ),
                  ),
                  label: const Icon(
                    Icons.copy_rounded,
                    size: Fonts.size300,
                    color: LxColors.grey550,
                  ),
                  style: ButtonStyle(
                    padding: const MaterialStatePropertyAll(EdgeInsets.zero),
                    visualDensity:
                        const VisualDensity(horizontal: -3.0, vertical: -3.0),
                    shape: MaterialStatePropertyAll(RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(LxRadius.r200))),
                  ),
                ),
              if (code == null)
                const Padding(
                  padding: EdgeInsets.symmetric(vertical: Space.s200),
                  child: FilledPlaceholder(
                    width: Space.s900,
                    forText: true,
                    height: Fonts.size100,
                    color: LxColors.background,
                  ),
                ),
              // const SizedBox(height: Space.s100),

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
                          color: LxColors.background,
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

              if (description == null && amountSatsStr == null)
                Padding(
                  padding: const EdgeInsets.only(top: Space.s400),
                  child: OutlinedButton(
                    onPressed: () {},
                    style: const ButtonStyle(
                      // shape: MaterialStatePropertyAll(RoundedRectangleBorder(
                      //     borderRadius: BorderRadius.all(
                      //         Radius.circular(LxRadius.r200)))),
                      // side: MaterialStatePropertyAll(BorderSide(
                      //   color: LxColors.foreground,
                      //   width: 2.0,
                      // )),

                      padding: MaterialStatePropertyAll(
                        EdgeInsets.symmetric(
                            vertical: Space.s200, horizontal: Space.s600),
                      ),
                      visualDensity: VisualDensity.compact,
                      // textStyle: MaterialStatePropertyAll(TextStyle(
                      //   color: LxColors.foreground,
                      //   fontSize: Fonts.size300,
                      //   fontVariations: [Fonts.weightBold],
                      // )),
                      // fixedSize: MaterialStatePropertyAll(Size.fromHeight(44.0)),
                    ),
                    child: const Row(
                        mainAxisAlignment: MainAxisAlignment.center,
                        children: [
                          SizedBox(width: Space.s200),
                          Text(
                            "Amount",
                            style: TextStyle(
                              fontSize: Fonts.size300,
                            ),
                          ),
                          SizedBox(width: Space.s200),
                          Icon(Icons.add_rounded),
                        ]),
                  ),
                ),
            ],
          ),
        ),
        const Expanded(child: Center()),
      ],
    );
  }
}

class PaymentOfferCard3 extends StatelessWidget {
  const PaymentOfferCard3(
      {super.key, required this.paymentOffer, required this.fiatRate});

  final PaymentOffer paymentOffer;
  final ValueStream<FiatRate?> fiatRate;

  @override
  Widget build(BuildContext context) {
    final code = this.paymentOffer.code;
    final uri = this.paymentOffer.uri();
    final amountSats = this.paymentOffer.amountSats;
    final amountSatsStr = (amountSats != null)
        ? formatSatsAmount(amountSats, satsSuffix: false)
        : null;
    final description = this.paymentOffer.description;

    return Column(
      mainAxisAlignment: MainAxisAlignment.start,
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: Space.s200),
          child: Container(
            // padding: const EdgeInsets.all(Space.s450),
            constraints: const BoxConstraints(maxWidth: 350.0),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.start,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(
                    Space.s450,
                    Space.s0,
                    Space.s450,
                    Space.s400,
                  ),
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.start,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      //   // const SizedBox(width: 300.0 - 2 * Space.s400),
                      //   // kind
                      Text(
                        this.paymentOffer.titleStr(),
                        style: const TextStyle(
                          color: LxColors.foreground,
                          fontSize: Fonts.size300,
                          fontVariations: [Fonts.weightMedium],
                          letterSpacing: -0.5,
                          height: 1.5,
                        ),
                      ),
                      const Text(
                        "Receive Bitcoin instantly with Lightning",
                        style: TextStyle(
                          color: LxColors.grey600,
                          fontSize: Fonts.size100,
                          // fontVariations: [Fonts.weightMedium],
                          // letterSpacing: -0.5,
                          height: 1.2,
                        ),
                      ),
                    ],
                  ),
                ),

                // QR code
                Container(
                  decoration: BoxDecoration(
                    color: LxColors.grey1000,
                    borderRadius: BorderRadius.circular(LxRadius.r300),
                  ),
                  padding: const EdgeInsets.fromLTRB(
                    Space.s450,
                    Space.s450,
                    Space.s450,
                    Space.s200,
                  ),
                  clipBehavior: Clip.antiAlias,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.end,
                    children: [
                      LayoutBuilder(
                        builder: (context, constraints) {
                          final double dim = constraints.maxWidth;
                          final key = ValueKey(uri ?? "");

                          return AnimatedSwitcher(
                            duration: const Duration(milliseconds: 250),
                            child: (uri != null)
                                ? Container(
                                    decoration: BoxDecoration(
                                        borderRadius:
                                            BorderRadius.circular(6.0)),
                                    clipBehavior: Clip.hardEdge,
                                    child: QrImage(
                                      // `AnimatedSwitcher` should also run the switch
                                      // animation when the QR code contents change.
                                      key: key,
                                      value: uri,
                                      dimension: dim.toInt(),
                                      color: LxColors.foreground,
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

                      // raw code string + copy button
                      if (code != null)
                        TextButton.icon(
                          onPressed: () {},
                          icon: Text(
                            address_format.ellipsizeBtcAddress(code),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(
                              fontSize: Fonts.size100,
                              color: LxColors.grey550,
                              height: 1.0,
                            ),
                          ),
                          label: const Icon(
                            Icons.copy_rounded,
                            size: Fonts.size300,
                            color: LxColors.grey550,
                          ),
                          style: ButtonStyle(
                            padding:
                                const MaterialStatePropertyAll(EdgeInsets.zero),
                            visualDensity: const VisualDensity(
                                horizontal: -3.0, vertical: -3.0),
                            shape: MaterialStatePropertyAll(
                                RoundedRectangleBorder(
                                    borderRadius:
                                        BorderRadius.circular(LxRadius.r200))),
                          ),
                        ),
                      if (code == null)
                        const Padding(
                          padding: EdgeInsets.symmetric(vertical: Space.s200),
                          child: FilledPlaceholder(
                            width: Space.s900,
                            forText: true,
                            height: Fonts.size100,
                            color: LxColors.background,
                          ),
                        ),
                    ],
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(
                      horizontal: Space.s450, vertical: Space.s450),
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.start,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
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
                          if (amountSats == null) {
                            return const SizedBox.shrink();
                          }

                          final String? amountFiatStr;
                          if (fiatRate != null) {
                            final amountFiat =
                                fiatRate.rate * satsToBtc(amountSats);
                            amountFiatStr =
                                formatFiat(amountFiat, fiatRate.fiat);
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
                                  color: LxColors.background,
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

                      if (description == null && amountSatsStr == null)
                        Padding(
                          padding: const EdgeInsets.only(top: Space.s100),
                          child: Row(
                            mainAxisAlignment: MainAxisAlignment.center,
                            children: [
                              LxFilledButton.strong(
                                onTap: () {},
                                style: const ButtonStyle(
                                  // shape: MaterialStatePropertyAll(RoundedRectangleBorder(
                                  //     borderRadius: BorderRadius.all(
                                  //         Radius.circular(LxRadius.r200)))),
                                  // side: MaterialStatePropertyAll(BorderSide(
                                  //   color: LxColors.foreground,
                                  //   width: 2.0,
                                  // )),

                                  padding: MaterialStatePropertyAll(
                                    EdgeInsets.symmetric(
                                        vertical: Space.s200,
                                        horizontal: Space.s600),
                                  ),
                                  visualDensity: VisualDensity.compact,
                                  // textStyle: MaterialStatePropertyAll(TextStyle(
                                  //   color: LxColors.foreground,
                                  //   fontSize: Fonts.size300,
                                  //   fontVariations: [Fonts.weightBold],
                                  // )),
                                  // fixedSize: MaterialStatePropertyAll(Size.fromHeight(44.0)),
                                ),
                                label: const Row(
                                    mainAxisAlignment: MainAxisAlignment.center,
                                    children: [
                                      SizedBox(width: Space.s200),
                                      Text(
                                        "Amount",
                                        style: TextStyle(
                                          fontSize: Fonts.size300,
                                        ),
                                      ),
                                      SizedBox(width: Space.s200),
                                      Icon(Icons.add_rounded),
                                    ]),
                              ),
                            ],
                          ),
                        ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),

        // Space
        const SizedBox(height: Space.s400),
        const Expanded(child: Center()),
      ],
    );
  }
}

class PaymentOfferCard4 extends StatelessWidget {
  const PaymentOfferCard4(
      {super.key, required this.paymentOffer, required this.fiatRate});

  final PaymentOffer paymentOffer;
  final ValueStream<FiatRate?> fiatRate;

  @override
  Widget build(BuildContext context) {
    final code = this.paymentOffer.code;
    final uri = this.paymentOffer.uri();
    final amountSats = this.paymentOffer.amountSats;
    final amountSatsStr = (amountSats != null)
        ? formatSatsAmount(amountSats, satsSuffix: false)
        : null;
    final description = this.paymentOffer.description;

    return Column(
      mainAxisAlignment: MainAxisAlignment.start,
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: Space.s200),
          child: Container(
            // padding: const EdgeInsets.all(Space.s450),
            constraints: const BoxConstraints(maxWidth: 350.0),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.start,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(
                    Space.s450,
                    Space.s0,
                    Space.s450,
                    Space.s400,
                  ),
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.start,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      //   // const SizedBox(width: 300.0 - 2 * Space.s400),
                      //   // kind
                      Text(
                        this.paymentOffer.titleStr(),
                        style: const TextStyle(
                          color: LxColors.foreground,
                          fontSize: Fonts.size300,
                          fontVariations: [Fonts.weightMedium],
                          letterSpacing: -0.5,
                          height: 1.5,
                        ),
                      ),
                      const Text(
                        "Receive Bitcoin instantly with Lightning",
                        style: TextStyle(
                          color: LxColors.grey600,
                          fontSize: Fonts.size100,
                          // fontVariations: [Fonts.weightMedium],
                          // letterSpacing: -0.5,
                          height: 1.2,
                        ),
                      ),
                    ],
                  ),
                ),

                // QR code
                Container(
                  decoration: BoxDecoration(
                    color: LxColors.grey1000,
                    borderRadius: BorderRadius.circular(LxRadius.r300),
                  ),
                  padding: const EdgeInsets.fromLTRB(
                    Space.s450,
                    Space.s450,
                    Space.s450,
                    Space.s200,
                  ),
                  clipBehavior: Clip.antiAlias,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.end,
                    children: [
                      LayoutBuilder(
                        builder: (context, constraints) {
                          final double dim = constraints.maxWidth;
                          final key = ValueKey(uri ?? "");

                          return AnimatedSwitcher(
                            duration: const Duration(milliseconds: 250),
                            child: (uri != null)
                                ? Container(
                                    decoration: BoxDecoration(
                                        borderRadius:
                                            BorderRadius.circular(6.0)),
                                    clipBehavior: Clip.hardEdge,
                                    child: QrImage(
                                      // `AnimatedSwitcher` should also run the switch
                                      // animation when the QR code contents change.
                                      key: key,
                                      value: uri,
                                      dimension: dim.toInt(),
                                      color: LxColors.foreground,
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
                      Row(
                        mainAxisAlignment: MainAxisAlignment.spaceBetween,
                        children: [
                          // raw code string + copy button
                          if (code != null)
                            TextButton.icon(
                              onPressed: () {},
                              label: Text(
                                address_format.ellipsizeBtcAddress(code),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: const TextStyle(
                                  fontSize: Fonts.size100,
                                  color: LxColors.grey550,
                                ),
                              ),
                              icon: const Icon(
                                Icons.copy_rounded,
                                size: Fonts.size300,
                                color: LxColors.grey550,
                              ),
                              // style: ButtonStyle(
                              //   // padding: const MaterialStatePropertyAll(
                              //   //     EdgeInsets.zero),
                              //   // visualDensity: const VisualDensity(
                              //   //     horizontal: -3.0, vertical: -3.0),
                              //   shape: MaterialStatePropertyAll(
                              //       RoundedRectangleBorder(
                              //           borderRadius: BorderRadius.circular(
                              //               LxRadius.r200))),
                              // ),
                            ),
                          if (code == null)
                            const Padding(
                              padding:
                                  EdgeInsets.symmetric(vertical: Space.s200),
                              child: FilledPlaceholder(
                                width: Space.s900,
                                forText: true,
                                height: Fonts.size100,
                                color: LxColors.background,
                              ),
                            ),

                          // if (!(description == null && amountSatsStr == null))
                          const Center(),

                          // TODO(phlip9): edit button when amount/description
                          // are set

                          if (description == null && amountSatsStr == null)
                            TextButton.icon(
                              onPressed: () {},
                              icon: const Icon(Icons.add_rounded),
                              label: const Text(
                                "Amount",
                                style: TextStyle(
                                  fontVariations: [Fonts.weightMedium],
                                ),
                              ),
                              style: const ButtonStyle(
                                foregroundColor:
                                    MaterialStatePropertyAll(LxColors.linkText),
                                // backgroundColor: MaterialStatePropertyAll(
                                //     LxColors.foreground),
                                // overlayColor: MaterialStatePropertyAll(
                                //     LxColors.clearW200),
                              ),
                            ),

                          // Padding(
                          //   padding: const EdgeInsets.only(top: Space.s100),
                          //   child: Row(
                          //     mainAxisAlignment: MainAxisAlignment.center,
                          //     children: [
                          //       LxFilledButton.strong(
                          //         onTap: () {},
                          //         style: const ButtonStyle(
                          //           // shape: MaterialStatePropertyAll(RoundedRectangleBorder(
                          //           //     borderRadius: BorderRadius.all(
                          //           //         Radius.circular(LxRadius.r200)))),
                          //           // side: MaterialStatePropertyAll(BorderSide(
                          //           //   color: LxColors.foreground,
                          //           //   width: 2.0,
                          //           // )),
                          //
                          //           padding: MaterialStatePropertyAll(
                          //             EdgeInsets.symmetric(
                          //                 vertical: Space.s100,
                          //                 horizontal: Space.s300),
                          //           ),
                          //           // visualDensity: VisualDensity.compact,
                          //           visualDensity: VisualDensity(
                          //               horizontal: -3.0, vertical: -3.0),
                          //           // textStyle: MaterialStatePropertyAll(TextStyle(
                          //           //   color: LxColors.foreground,
                          //           //   fontSize: Fonts.size300,
                          //           //   fontVariations: [Fonts.weightBold],
                          //           // )),
                          //           // fixedSize: MaterialStatePropertyAll(Size.fromHeight(44.0)),
                          //         ),
                          //         label: const Row(
                          //             mainAxisAlignment:
                          //                 MainAxisAlignment.center,
                          //             children: [
                          //               SizedBox(width: Space.s200),
                          //               Text(
                          //                 "Amount",
                          //                 style: TextStyle(
                          //                   fontSize: Fonts.size200,
                          //                 ),
                          //               ),
                          //               SizedBox(width: Space.s200),
                          //               Icon(
                          //                 Icons.add_rounded,
                          //                 // size: Fonts.size,
                          //               ),
                          //             ]),
                          //       ),
                          //     ],
                          //   ),
                          // ),
                        ],
                      ),

                      // const SizedBox(height: Space.s400),
                      //
                      // Center(
                      //   child: LxFilledButton.tonal(
                      //     onTap: () {},
                      //     label: const Row(
                      //       mainAxisAlignment: MainAxisAlignment.center,
                      //       children: [
                      //         Icon(Icons.add_rounded),
                      //         SizedBox(width: Space.s200),
                      //         Text(
                      //           "Amount",
                      //           style: TextStyle(
                      //             fontSize: Fonts.size300,
                      //           ),
                      //         ),
                      //       ],
                      //     ),
                      //     style: const ButtonStyle(
                      //       // fixedSize: MaterialStatePropertyAll(
                      //       //     Size.fromHeight(44.0)),
                      //       visualDensity:
                      //           VisualDensity(horizontal: -3.0, vertical: -3.0),
                      //     ),
                      //   ),
                      // ),
                      //
                      // const SizedBox(height: Space.s200),

                      // Row(
                      //   children: [
                      //     LxFilledButton.strong(
                      //       onTap: () {},
                      //
                      //     )
                      //   ],
                      // )
                    ],
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.symmetric(
                      horizontal: Space.s450, vertical: Space.s450),
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.start,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
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
                          if (amountSats == null) {
                            return const SizedBox.shrink();
                          }

                          final String? amountFiatStr;
                          if (fiatRate != null) {
                            final amountFiat =
                                fiatRate.rate * satsToBtc(amountSats);
                            amountFiatStr =
                                formatFiat(amountFiat, fiatRate.fiat);
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
                                  color: LxColors.background,
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
                ),
              ],
            ),
          ),
        ),

        // Space
        const SizedBox(height: Space.s400),
        const Expanded(child: Center()),
      ],
    );
  }
}

class PaymentOfferCard5 extends StatelessWidget {
  const PaymentOfferCard5(
      {super.key, required this.paymentOffer, required this.fiatRate});

  final PaymentOffer paymentOffer;
  final ValueStream<FiatRate?> fiatRate;

  @override
  Widget build(BuildContext context) {
    final code = this.paymentOffer.code;
    final uri = this.paymentOffer.uri();
    final amountSats = this.paymentOffer.amountSats;
    final amountSatsStr = (amountSats != null)
        ? formatSatsAmount(amountSats, satsSuffix: false)
        : null;
    final description = this.paymentOffer.description;

    return Container(
      margin: const EdgeInsets.symmetric(horizontal: Space.s300),
      // padding: const EdgeInsets.all(Space.s450),
      constraints: const BoxConstraints(maxWidth: 350.0),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.start,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(
              Space.s450,
              Space.s0,
              Space.s450,
              Space.s400,
            ),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.start,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                //   // const SizedBox(width: 300.0 - 2 * Space.s400),
                //   // kind
                Text(
                  this.paymentOffer.titleStr(),
                  style: const TextStyle(
                    color: LxColors.foreground,
                    fontSize: Fonts.size400,
                    fontVariations: [Fonts.weightMedium],
                    letterSpacing: -0.5,
                    height: 1.5,
                  ),
                ),
                const Text(
                  "Receive Bitcoin instantly with Lightning",
                  style: TextStyle(
                    color: LxColors.grey600,
                    fontSize: Fonts.size100,
                    // fontVariations: [Fonts.weightMedium],
                    // letterSpacing: -0.5,
                    height: 1.2,
                  ),
                ),
              ],
            ),
          ),

          // Card
          Container(
            decoration: BoxDecoration(
              color: LxColors.grey1000,
              borderRadius: BorderRadius.circular(LxRadius.r300),
            ),
            padding: const EdgeInsets.fromLTRB(
              Space.s450,
              Space.s100,
              Space.s450,
              Space.s450,
            ),
            clipBehavior: Clip.antiAlias,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                // code + tertiary icons
                Row(
                  mainAxisAlignment: MainAxisAlignment.start,
                  children: [
                    // raw code string + copy button
                    if (code != null)
                      Transform.translate(
                        offset: const Offset(-Space.s300, Space.s0),
                        child: TextButton.icon(
                          onPressed: () {},
                          icon: Text(
                            address_format.ellipsizeBtcAddress(code),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(
                              fontSize: Fonts.size100,
                              color: LxColors.grey550,
                            ),
                          ),
                          label: const Icon(
                            Icons.copy_rounded,
                            size: Fonts.size300,
                            color: LxColors.grey550,
                          ),
                        ),
                      ),
                    if (code == null)
                      const Padding(
                        padding: EdgeInsets.symmetric(vertical: Space.s200),
                        child: FilledPlaceholder(
                          width: Space.s900,
                          forText: true,
                          height: Fonts.size100,
                          color: LxColors.background,
                        ),
                      ),

                    const Expanded(child: Center()),

                    Transform.translate(
                      offset: const Offset(Space.s200, 0.0),
                      child: IconButton(
                        onPressed: () {},
                        icon: const Icon(Icons.more_horiz_rounded),
                        visualDensity: VisualDensity.compact,
                      ),
                    ),
                  ],
                ),

                // QR code
                LayoutBuilder(
                  builder: (context, constraints) {
                    final double dim = constraints.maxWidth;
                    final key = ValueKey(uri ?? "");

                    return AnimatedSwitcher(
                      duration: const Duration(milliseconds: 250),
                      child: (uri != null)
                          ? Container(
                              decoration: BoxDecoration(
                                  borderRadius: BorderRadius.circular(6.0)),
                              clipBehavior: Clip.hardEdge,
                              child: QrImage(
                                // `AnimatedSwitcher` should also run the switch
                                // animation when the QR code contents change.
                                key: key,
                                value: uri,
                                dimension: dim.toInt(),
                                color: LxColors.foreground,
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

                // + Amount button
                if (amountSatsStr == null && description == null)
                  Padding(
                    padding: const EdgeInsets.only(top: Space.s400),
                    child: Row(
                      mainAxisAlignment: MainAxisAlignment.end,
                      children: [
                        // IconButton(
                        //   onPressed: () {},
                        //   icon: const Icon(Icons.copy_rounded),
                        //   color: LxColors.fgSecondary,
                        //   iconSize: 20.0,
                        // ),
                        // IconButton(
                        //   onPressed: () {},
                        //   icon: const Icon(Icons.share_rounded),
                        //   color: LxColors.fgSecondary,
                        //   iconSize: 20.0,
                        // ),
                        // const Expanded(child: Center()),
                        OutlinedButton(
                          onPressed: () {},
                          style: const ButtonStyle(
                            // fixedSize: MaterialStatePropertyAll(
                            //     Size.fromHeight(44.0)),
                            visualDensity:
                                VisualDensity(horizontal: -3.0, vertical: -3.0),
                            // shape: MaterialStatePropertyAll(
                            //     RoundedRectangleBorder(
                            //   borderRadius:
                            //       BorderRadius.circular(LxRadius.r200),
                            // )),
                          ),
                          child: const Row(
                            mainAxisAlignment: MainAxisAlignment.center,
                            children: [
                              SizedBox(width: Space.s200),
                              Icon(Icons.add_rounded),
                              SizedBox(width: Space.s200),
                              Text(
                                "Amount",
                                style: TextStyle(
                                  fontSize: Fonts.size300,
                                ),
                              ),
                              SizedBox(width: Space.s400),
                            ],
                          ),
                        ),
                      ],
                    ),
                  ),
                if (amountSatsStr != null || description != null)
                  const SizedBox(height: Space.s400),

                if (amountSatsStr != null || description != null)
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    mainAxisSize: MainAxisSize.max,
                    children: [
                      // Amount and/or description
                      Column(
                        mainAxisSize: MainAxisSize.max,
                        children: [
                          // Amount (sats)
                          if (amountSatsStr != null)
                            Padding(
                              padding:
                                  const EdgeInsets.only(bottom: Space.s100),
                              child: Text.rich(
                                TextSpan(
                                  children: [
                                    TextSpan(text: amountSatsStr),
                                    const TextSpan(
                                        text: " sats",
                                        style:
                                            TextStyle(color: LxColors.grey550)),
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
                              if (amountSats == null) {
                                return const SizedBox.shrink();
                              }

                              final String? amountFiatStr;
                              if (fiatRate != null) {
                                final amountFiat =
                                    fiatRate.rate * satsToBtc(amountSats);
                                amountFiatStr =
                                    formatFiat(amountFiat, fiatRate.fiat);
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
                                      color: LxColors.background,
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

                      // // edit icon
                      // Transform.translate(
                      //   offset: const Offset(Space.s200, 0.0),
                      //   child: IconButton(
                      //     onPressed: () {},
                      //     icon: const Icon(
                      //       Icons.edit_outlined,
                      //       size: Fonts.size500,
                      //       color: LxColors.fgSecondary,
                      //     ),
                      //   ),
                      // ),

                      // edit icon
                      Transform.translate(
                        offset: const Offset(Space.s200, 0.0),
                        child: TextButton.icon(
                          onPressed: () {},
                          label: const Text(
                            "Edit",
                            style: TextStyle(fontSize: Fonts.size200),
                          ),
                          icon: const Icon(
                            Icons.edit_square,
                            size: Fonts.size200,
                          ),
                        ),
                      ),
                    ],
                  ),
              ],
            ),
          ),

          const SizedBox(height: Space.s450),

          // Container(
          //   margin: const EdgeInsets.symmetric(horizontal: Space.s450),
          //   padding: const EdgeInsets.all(Space.s200),
          //   color: LxColors.moneyGoUp,
          //   child: const Text.rich(
          //     TextSpan(children: [
          //       TextSpan(
          //           text:
          //               "Receiving via Lightning will incur an initial setup fee of "),
          //       TextSpan(
          //         text: "2,000 sats",
          //         style: TextStyle(fontVariations: [Fonts.weightSemiBold]),
          //       )
          //     ]),
          //   ),
          // ),

          // good

          Padding(
            padding: const EdgeInsets.only(left: Space.s450, right: Space.s200),
            child: Row(
              children: [
                const Expanded(
                  child: Text.rich(
                    TextSpan(children: [
                      // Pay invoice once
                      TextSpan(
                          text:
                              "Invoices can only be paid once. Reusing an invoice may result in lost payments. "),
                      TextSpan(
                        text: "Read more",
                        style: TextStyle(
                          decoration: TextDecoration.underline,
                          decorationColor: LxColors.grey550,
                          decorationThickness: 1.0,
                        ),
                      ),

                      // // Zero-conf ()
                      // TextSpan(text: "Receiving more than "),
                      // TextSpan(
                      //   text: "150,000 sats",
                      //   style: TextStyle(
                      //     fontVariations: [Fonts.weightSemiBold],
                      //   ),
                      // ),
                      // TextSpan(text: " will incur an initial setup fee of "),
                      // TextSpan(
                      //   text: "2,500 sats",
                      //   style: TextStyle(
                      //     fontVariations: [Fonts.weightSemiBold],
                      //   ),
                      // ),
                      // TextSpan(text: "."),
                    ]),
                    style: TextStyle(
                      color: LxColors.grey550,
                      fontSize: Fonts.size100,
                      // letterSpacing: -0.2,
                    ),
                  ),
                ),
                IconButton(
                  onPressed: () {},
                  icon: const Icon(
                    Icons.close_rounded,
                  ),
                  color: LxColors.grey650,
                )
              ],
            ),
          ),

          // const Padding(
          //   padding: EdgeInsets.symmetric(horizontal: Space.s450),
          //   child: Row(
          //     children: [
          //       Padding(
          //         padding: EdgeInsets.only(right: Space.s400),
          //         child: Icon(
          //           Icons.warning_rounded,
          //           color: LxColors.grey550,
          //         ),
          //       ),
          //       Expanded(
          //         child: Text.rich(
          //           TextSpan(children: [
          //             // TextSpan(
          //             //     text: "Watch out! ",
          //             //     style: TextStyle(fontVariations: [Fonts.weightBold])),
          //             // TextSpan(text: "Invoices can only be paid once."),
          //             // TextSpan(
          //             //   text: "WARNING:  ",
          //             //   style:
          //             //       TextStyle(fontVariations: [Fonts.weightMedium]),
          //             // ),
          //             TextSpan(
          //                 text:
          //                     "Invoices can only be paid once. Reusing an invoice may result in lost payments. "),
          //             // TextSpan(
          //             //   text: "Why?",
          //             //   style: TextStyle(
          //             //     decoration: TextDecoration.underline,
          //             //     decorationColor: LxColors.grey550,
          //             //     decorationThickness: 1.5,
          //             //   ),
          //             // ),
          //           ]),
          //           style: TextStyle(
          //             color: LxColors.grey550,
          //             fontSize: Fonts.size100,
          //           ),
          //         ),
          //       ),
          //     ],
          //   ),
          // ),

          // Space
          // const SizedBox(height: Space.s400),

          // Push elements outside page to bottom
          const Expanded(child: Center()),
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
                Space.s500, Space.s450, Space.s500, Space.s500),
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

  void onKindSelected(BuildContext context, PaymentOfferKind flowResult) {
    info("ReceiveSettingsBottomSheet: selected kind: $flowResult");
    unawaited(Navigator.of(context).maybePop(flowResult));
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
              onChanged: (kind) => this.onKindSelected(context, kind),
            ),
          if (this.kind.isLightning())
            PaymentOfferKindRadio(
              kind: PaymentOfferKind.lightningOffer,
              selected: this.kind,
              title: const Text("Lightning offer"),
              subtitle: const Text(
                  "New. Offers can be paid many times. Paste one on your twitter!"),
              // TODO(phlip9): uncomment when BOLT12 offers are supported.
              // onChanged: (kind) => this.onKindSelected(context, kind),
              onChanged: null,
            ),

          // BTC
          if (!this.kind.isLightning())
            PaymentOfferKindRadio(
              kind: PaymentOfferKind.btcAddress,
              selected: this.kind,
              title: const Text("Bitcoin SegWit address"),
              subtitle: const Text("Recommended. Supported by most wallets."),
              onChanged: (kind) => this.onKindSelected(context, kind),
            ),
          if (!this.kind.isLightning())
            PaymentOfferKindRadio(
              kind: PaymentOfferKind.btcTaproot,
              selected: this.kind,
              title: const Text("Bitcoin Taproot address"),
              subtitle: const Text(
                  "Newer format. Reduced fees and increased privacy."),
              // TODO(phlip9): uncomment when taproot addresses are supported.
              // onChanged: (kind) => this.onKindSelected(context, kind),
              onChanged: null,
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
  const ReceivePaymentSetAmountPage({
    super.key,
    required this.prevAmountSats,
    required this.prevDescription,
  });

  final int? prevAmountSats;
  final String? prevDescription;

  @override
  State<ReceivePaymentSetAmountPage> createState() =>
      _ReceivePaymentSetAmountPageState();
}

class _ReceivePaymentSetAmountPageState
    extends State<ReceivePaymentSetAmountPage> {
  final GlobalKey<FormFieldState<String>> amountFieldKey = GlobalKey();
  final GlobalKey<FormFieldState<String>> descriptionFieldKey = GlobalKey();

  final IntInputFormatter intInputFormatter = IntInputFormatter();

  void onConfirm() {
    final amountState = this.amountFieldKey.currentState!;
    if (!amountState.validate()) return;

    final String? amountStr = amountState.value;
    final int? amountSats;
    if (amountStr != null) {
      final a = this.intInputFormatter.tryParse(amountStr).ok;
      if (a != 0) {
        amountSats = a;
      } else {
        amountSats = null;
      }
    } else {
      amountSats = null;
    }

    final descriptionState = this.descriptionFieldKey.currentState!;
    if (!descriptionState.validate()) return;

    final String? d = descriptionState.value;
    final String? description;
    if (d != null) {
      // "" => null
      description = (d.isNotEmpty) ? d : null;
    } else {
      description = null;
    }

    final flowResult = (amountSats: amountSats, description: description);
    unawaited(Navigator.of(this.context).maybePop(flowResult));
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
          const SizedBox(height: Space.s800),

          // <amount> sats
          PaymentAmountInput(
            fieldKey: this.amountFieldKey,
            intInputFormatter: this.intInputFormatter,
            allowEmpty: true,
            initialValue: this.widget.prevAmountSats,
          ),

          const SizedBox(height: Space.s800),

          PaymentNoteInput(
            fieldKey: this.descriptionFieldKey,
            onSubmit: this.onConfirm,
            initialNote: this.widget.prevDescription,
          ),

          const SizedBox(height: Space.s400),
        ],
        bottom: LxFilledButton(
          label: const Text("Confirm"),
          icon: const Icon(Icons.arrow_forward_rounded),
          onTap: this.onConfirm,
        ),
      ),
    );
  }
}
