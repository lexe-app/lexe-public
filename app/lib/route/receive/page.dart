/// Receive payment page.
library;

import 'dart:async' show unawaited;
import 'dart:math' show min;

import 'package:app_rs_dart/ffi/api.dart'
    show CreateInvoiceRequest, CreateOfferRequest, FiatRate;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show Invoice, Offer;
import 'package:flutter/cupertino.dart' show CupertinoScrollBehavior;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lexeapp/address_format.dart' as address_format;
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        CarouselIndicatorsAndButtons,
        FilledPlaceholder,
        FilledTextPlaceholder,
        HeadingText,
        LxBackButton,
        LxFilledButton,
        PaymentAmountInput,
        PaymentNoteInput,
        ScrollableSinglePageBody,
        SubheadingText,
        VoidContextCallback;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/feature_flags.dart' show FeatureFlags;
import 'package:lexeapp/input_formatter.dart' show IntInputFormatter;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/receive/state.dart'
    show
        BtcAddrInputs,
        BtcAddrKind,
        LnInvoiceInputs,
        LnOfferInputs,
        PaymentOffer,
        PaymentOfferKind;
import 'package:lexeapp/route/show_qr.dart' show InteractiveQrImage;
import 'package:lexeapp/share.dart' show LxShare;
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxRadius, Space;

/// The viewport breakpoint at which we cap the inner page width and possibly
/// show multiple [PaymentOfferPage]s on-screen simultaneously.
const double maxViewportWidth = 450.0;

/// Each [PaymentOfferPage] should occupy at most this proportion of the screen
/// width. This value is chosen so that a sliver of the next/prev page is always
/// visible to hint to the user that they can swipe left/right.
const double targetPagePropWidth = 0.9;

/// The computed max width of a [PaymentOfferPage] on-screen. Ensure this is a
/// whole number.
const double maxPageWidth = targetPagePropWidth * maxViewportWidth;

/// The index of each [PaymentOfferPage] kind in the [PageView].
const int lnInvoicePageIdx = 0;
const int lnOfferPageIdx = 1;
const int btcPageIdx = 2;

class ReceivePaymentPage extends StatelessWidget {
  const ReceivePaymentPage({
    super.key,
    required this.app,
    required this.featureFlags,
    required this.fiatRate,
  });

  final AppHandle app;
  final FeatureFlags featureFlags;

  /// Updating stream of fiat rates.
  final ValueListenable<FiatRate?> fiatRate;

  @override
  Widget build(BuildContext context) => ReceivePaymentPageInner(
    app: this.app,
    featureFlags: this.featureFlags,
    fiatRate: this.fiatRate,
    viewportWidth: MediaQuery.sizeOf(context).width,
  );
}

/// We need this extra intermediate "inner" widget so we can init the
/// [PageController] with a `viewportFraction` derived from the screen width.
class ReceivePaymentPageInner extends StatefulWidget {
  const ReceivePaymentPageInner({
    super.key,
    required this.app,
    required this.featureFlags,
    required this.fiatRate,
    required this.viewportWidth,
  });

  final AppHandle app;
  final FeatureFlags featureFlags;
  final ValueListenable<FiatRate?> fiatRate;

  final double viewportWidth;

  @override
  State<ReceivePaymentPageInner> createState() =>
      ReceivePaymentPageInnerState();
}

class ReceivePaymentPageInnerState extends State<ReceivePaymentPageInner> {
  /// Whether we should show the experimental BOLT12 offers recv page.
  late bool showOffer = this.widget.featureFlags.showBolt12OffersRecvPage;

  /// Controls the [PageView].
  late PageController pageController = this.newPageController();

  /// The current primary page on-screen.
  final ValueNotifier<int> selectedPageIndex = ValueNotifier(0);

  /// Inputs that determine when we should fetch a new lightning invoice.
  final ValueNotifier<LnInvoiceInputs> lnInvoiceInputs = ValueNotifier(
    const LnInvoiceInputs(amountSats: null, description: null),
  );

  /// Inputs that determine when we should fetch a new lightning offer.
  final ValueNotifier<LnOfferInputs> lnOfferInputs = ValueNotifier(
    const LnOfferInputs(amountSats: null, description: null),
  );

  /// Inputs that determine when we should fetch a new bitcoin address.
  final ValueNotifier<BtcAddrInputs> btcAddrInputs = ValueNotifier(
    const BtcAddrInputs(kind: BtcAddrKind.segwit),
  );

  /// Each page offer.
  // TODO(phlip9): make final again once offers always enabled
  late List<ValueNotifier<PaymentOffer>> paymentOffers = [
    ValueNotifier(
      const PaymentOffer.unloaded(kind: PaymentOfferKind.lightningInvoice),
    ),
    if (this.showOffer)
      ValueNotifier(
        const PaymentOffer.unloaded(kind: PaymentOfferKind.lightningOffer),
      ),
    ValueNotifier(
      const PaymentOffer.unloaded(kind: PaymentOfferKind.btcAddress),
    ),
  ];

  // TODO(phlip9): once offers always enabled, make these constants again.
  int get lnInvoicePageIdx => 0;
  int get lnOfferPageIdx => this.showOffer ? 1 : -1;
  int get btcPageIdx => this.showOffer ? 2 : 1;

  @override
  void initState() {
    super.initState();

    // Fetch a new lightning invoice when its inputs change.
    this.lnInvoiceInputs.addListener(this.doFetchLnInvoice);

    // Fetch a new lightning offer when its inputs change.
    if (this.showOffer) {
      this.lnOfferInputs.addListener(this.doFetchLnOffer);
    }

    // Fetch a new btc address when certain BTC inputs change.
    this.btcAddrInputs.addListener(this.doFetchBtc);

    // Kick us off by fetching an initial zero-amount invoice, offer, and btc
    // address.

    unawaited(this.doFetchLnInvoice());
    if (this.showOffer) {
      unawaited(this.doFetchLnOffer());
    }
    unawaited(this.doFetchBtc());
  }

  @override
  void dispose() {
    this.pageController.dispose();
    this.selectedPageIndex.dispose();

    this.lnInvoiceInputs.dispose();
    this.lnOfferInputs.dispose();
    this.btcAddrInputs.dispose();

    for (final paymentOffer in this.paymentOffers) {
      paymentOffer.dispose();
    }

    super.dispose();
  }

  @override
  void didUpdateWidget(ReceivePaymentPageInner oldWidget) {
    super.didUpdateWidget(oldWidget);

    // We need to rebuild the [PageController] when the window resizes.
    if (this.widget.viewportWidth != oldWidget.viewportWidth) {
      final oldController = this.pageController;
      this.pageController = this.newPageController();
      oldController.dispose();
    }
  }

  /// Create a new inner [PageController] for the carousel of [PaymentOfferPage]s.
  PageController newPageController() => PageController(
    initialPage: this.selectedPageIndex.value,
    // Use at most `targetPagePropWidth` of the screen width to ensure
    // we always show a small sliver of the next page.
    viewportFraction: min(
      targetPagePropWidth,
      maxPageWidth / this.widget.viewportWidth,
    ),
  );

  ValueNotifier<PaymentOffer> currentPaymentOffer() =>
      this.paymentOffers[this.selectedPageIndex.value];

  ValueNotifier<PaymentOffer> lnInvoicePaymentOffer() =>
      this.paymentOffers[lnInvoicePageIdx];
  ValueNotifier<PaymentOffer> lnOfferPaymentOffer() =>
      this.paymentOffers[lnOfferPageIdx];
  ValueNotifier<PaymentOffer> btcPaymentOffer() =>
      this.paymentOffers[btcPageIdx];

  /// Fetch a bitcoin address for the given [BtcAddrInputs] and return a
  /// full [PaymentOffer].
  Future<PaymentOffer?> fetchBtc(BtcAddrInputs inputs) async {
    // TODO(phlip9): actually add ability to fetch a taproot address
    // assert(btcKind != PaymentOfferKind.btcTaproot);

    info("ReceivePaymentPage: fetchBtc: inputs: $inputs");

    final result = await Result.tryFfiAsync(this.widget.app.getAddress);

    final String address;
    switch (result) {
      case Err(:final err):
        // TODO(phlip9): error display
        error("ReceivePaymentPage: fetchBtc: failed to getAddress: $err");
        return null;

      case Ok(:final ok):
        address = ok;
        info("ReceivePaymentPage: fetchBtc: getAddress => done");
    }

    return PaymentOffer(
      kind: PaymentOfferKind.btcAddress,
      code: address,
      amountSats: null,
      description: null,
      expiresAt: null,
    );
  }

  Future<void> doFetchBtc() async {
    // TODO(phlip9): UI indicator that we're fetching
    final inputs = this.btcAddrInputs.value;
    final btcPaymentOffer = this.btcPaymentOffer();
    final prev = btcPaymentOffer.value;

    final offer = await this.fetchBtc(inputs);

    // Canceled / navigated away => ignore
    if (!this.mounted) return;

    // Error => ignore (TODO(phlip9): handle)
    if (offer == null) return;

    // Stale request => ignore
    if (prev != btcPaymentOffer.value) {
      info("ReceivePaymentPage: doFetchBtc: stale request, ignoring response");
      return;
    }

    // Everything's good -> update our current BTC page offer
    btcPaymentOffer.value = offer;
  }

  /// When the user hits the refresh button, we fetch a new invoice. Keep the
  /// amount and description set in the UI.
  Future<void> doRefreshLnInvoice() async {
    // Reset invoice
    final lnInvoicePaymentOffer = this.lnInvoicePaymentOffer();
    final prev = lnInvoicePaymentOffer.value;
    lnInvoicePaymentOffer.value = prev.resetForRefresh();

    // Fetch new invoice w/ same inputs
    await this.doFetchLnInvoice();
  }

  Future<void> doFetchLnInvoice() async {
    // TODO(phlip9): UI indicator that we're fetching
    final inputs = this.lnInvoiceInputs.value;
    final lnInvoicePaymentOffer = this.lnInvoicePaymentOffer();
    final prev = lnInvoicePaymentOffer.value;

    final invoice = await this.fetchLnInvoice(inputs);

    // Canceled / navigated away => ignore
    if (!this.mounted) return;

    // Error => ignore (TODO(phlip9): handle)
    if (invoice == null) return;

    // Stale request => ignore
    if (prev != lnInvoicePaymentOffer.value) {
      info(
        "ReceivePaymentPage: doFetchLnInvoice: stale request, ignoring response",
      );
      return;
    }

    // Everything's good -> update our current LN page offer
    lnInvoicePaymentOffer.value = invoice;
  }

  /// Fetch the Lightning invoice for the given inputs.
  Future<PaymentOffer?> fetchLnInvoice(LnInvoiceInputs inputs) async {
    final req = CreateInvoiceRequest(
      expirySecs: 24 * 60 * 60,
      amountSats: inputs.amountSats,
      description: inputs.description,
    );

    info("ReceivePaymentPage: fetchLnInvoice: inputs: $inputs");

    final result = await Result.tryFfiAsync(
      () => this.widget.app.createInvoice(req: req),
    );

    final Invoice invoice;
    switch (result) {
      case Err(:final err):
        // TODO(phlip9): error display
        error(
          "ReceivePaymentPage: doFetchLnInvoice: failed to create invoice: $err",
        );
        return null;

      case Ok(:final ok):
        invoice = ok.invoice;
        info("ReceivePaymentPage: doFetchLnInvoice: createInvoice => done");
    }

    return PaymentOffer(
      kind: PaymentOfferKind.lightningInvoice,
      code: invoice.string,
      amountSats: invoice.amountSats,
      description: invoice.description,
      expiresAt: DateTime.fromMillisecondsSinceEpoch(invoice.expiresAt),
    );
  }

  Future<void> doFetchLnOffer() async {
    // TODO(phlip9): UI indicator that we're fetching
    final inputs = this.lnOfferInputs.value;
    final lnOfferPaymentOffer = this.lnOfferPaymentOffer();
    final prev = lnOfferPaymentOffer.value;

    final offer = await this.fetchLnOffer(inputs);

    // Canceled / navigated away => ignore
    if (!this.mounted) return;

    // Error => ignore (TODO(phlip9): handle)
    if (offer == null) return;

    // Stale request => ignore
    if (prev != lnOfferPaymentOffer.value) {
      info(
        "ReceivePaymentPage: doFetchLnOffer: stale request, ignoring response",
      );
      return;
    }

    // Everything's good -> update our current LN page offer
    lnOfferPaymentOffer.value = offer;
  }

  /// Fetch the Lightning offer for the given inputs.
  Future<PaymentOffer?> fetchLnOffer(LnOfferInputs inputs) async {
    final req = CreateOfferRequest(
      expirySecs: null,
      amountSats: inputs.amountSats,
      description: inputs.description,
    );

    info("ReceivePaymentPage: fetchLnOffer: inputs: $inputs");

    final result = await Result.tryFfiAsync(
      () => this.widget.app.createOffer(req: req),
    );

    final Offer offer;
    switch (result) {
      case Err(:final err):
        // TODO(phlip9): error display
        error(
          "ReceivePaymentPage: fetchLnOffer: failed to create offer: $err, req: req: { amountStas: ${req.amountSats}, exp: ${req.expirySecs} }",
        );
        return null;

      case Ok(:final ok):
        offer = ok.offer;
        info("ReceivePaymentPage: fetchLnOffer: createOffer => done");
    }

    final expiresAt = offer.expiresAt;

    return PaymentOffer(
      kind: PaymentOfferKind.lightningOffer,
      code: offer.string,
      amountSats: offer.amountSats,
      description: offer.description,
      expiresAt: (expiresAt != null)
          ? DateTime.fromMillisecondsSinceEpoch(expiresAt)
          : null,
    );
  }

  // /// Open the [ReceiveSettingsBottomSheet] for the user to modify the current
  // /// page's receive offer settings.
  // Future<void> openSettingsBottomSheet(BuildContext context) async {
  //   final PaymentOfferKind? kind = await showModalBottomSheet<PaymentOfferKind>(
  //     backgroundColor: LxColors.background,
  //     elevation: 0.0,
  //     clipBehavior: Clip.hardEdge,
  //     enableDrag: true,
  //     isDismissible: true,
  //     isScrollControlled: true,
  //     context: context,
  //     builder: (context) => ReceiveSettingsBottomSheet(
  //       kind: this.currentOffer().value.kind,
  //     ),
  //   );
  //
  //   if (!this.mounted || kind == null) return;
  //
  //   final offerNotifier = this.currentOffer();
  //   final prevOffer = offerNotifier.value;
  //   offerNotifier.value = PaymentOffer(
  //     amountSats: prevOffer.amountSats,
  //     description: prevOffer.description,
  //     // Update these fields. We'll unset the code to prevent accidentally
  //     // scanning the old QR and indicate that the new QR is loading.
  //     kind: kind,
  //     code: null,
  //     expiresAt: null,
  //   );
  //
  //   final pageIdx = this.selectedPageIndex.value;
  //   final prevInputs = this.paymentOfferInputs.value;
  //   this.paymentOfferInputs.value = PaymentOfferInputs(
  //     // Update the new desired offer kind for the current page.
  //     kindByPage: (pageIdx == 0)
  //         ? ([kind, prevInputs.kindByPage[1]])
  //         : ([prevInputs.kindByPage[0], kind]),
  //     amountSats: prevInputs.amountSats,
  //     description: prevInputs.description,
  //   );
  // }

  // Open an edit page when we press a "+ Amount" or "Edit" button for the given
  // page.
  Future<void> openEditPage(PaymentOfferKind kind) async {
    // Only support setting amount/desc for invoices atm.
    if (kind != PaymentOfferKind.lightningInvoice) return;

    switch (kind) {
      case PaymentOfferKind.lightningInvoice:
        await this.openEditInvoicePage();
        return;

      // Other kinds don't support editing amount/description yet.
      default:
        return;
    }
  }

  Future<void> openEditInvoicePage() async {
    final prev = this.lnInvoiceInputs.value;

    final LnInvoiceInputs? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) => ReceivePaymentEditInvoicePage(prev: prev),
      ),
    );

    if (!this.mounted || flowResult == null || flowResult == prev) return;

    // Clear LN invoice code so it's clear we're fetching a new one.
    final lnInvoicePaymentOffer = this.lnInvoicePaymentOffer();
    final lnInvoice = lnInvoicePaymentOffer.value;
    lnInvoicePaymentOffer.value = PaymentOffer(
      kind: lnInvoice.kind,
      code: null,
      expiresAt: null,
      amountSats: flowResult.amountSats,
      description: flowResult.description,
    );

    // Update inputs to fetch new invoice.
    this.lnInvoiceInputs.value = flowResult;
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
        title: const Text(
          "Receive",
          // "Receive payment",
          style: TextStyle(
            color: LxColors.foreground,
            fontSize: Fonts.size500,
            fontVariations: [Fonts.weightMedium],
            letterSpacing: -0.5,
            height: 1.0,
          ),
        ),
      ),
      body: ScrollableSinglePageBody(
        padding: EdgeInsets.zero,
        useFullWidth: true,
        body: [
          // Payment offer pages (LN invoice, BTC address)
          Padding(
            padding: const EdgeInsets.only(top: Space.s200, bottom: Space.s400),
            child: SizedBox(
              height: 660.0,
              // height: 575.0,
              child: PageView(
                controller: this.pageController,
                clipBehavior: Clip.none,
                scrollBehavior: const CupertinoScrollBehavior(),
                padEnds: true,
                allowImplicitScrolling: false,
                onPageChanged: (pageIdx) {
                  if (!this.mounted) return;
                  this.selectedPageIndex.value = pageIdx;
                },
                children: this.paymentOffers
                    .map(
                      (offer) => ValueListenableBuilder(
                        valueListenable: offer,
                        builder: (_context, offer, _child) => PaymentOfferPage(
                          paymentOffer: offer,
                          fiatRate: this.widget.fiatRate,
                          openSetAmountPage: () =>
                              this.openEditPage(offer.kind),
                          refreshPaymentOffer:
                              // Only invoices need to be refresh-able since
                              // they're single-use.
                              offer.kind == PaymentOfferKind.lightningInvoice
                              ? this.doRefreshLnInvoice
                              : null,
                        ),
                      ),
                    )
                    .toList(),
              ),
            ),
          ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.symmetric(horizontal: Space.s600),
          child: CarouselIndicatorsAndButtons(
            numPages: this.paymentOffers.length,
            selectedPageIndex: this.selectedPageIndex,
            onTapPrev: () => unawaited(
              this.pageController.previousPage(
                duration: const Duration(milliseconds: 500),
                curve: Curves.ease,
              ),
            ),
            onTapNext: () => unawaited(
              this.pageController.nextPage(
                duration: const Duration(milliseconds: 500),
                curve: Curves.ease,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class PaymentOfferPage extends StatelessWidget {
  const PaymentOfferPage({
    super.key,
    required this.paymentOffer,
    required this.fiatRate,
    required this.openSetAmountPage,
    required this.refreshPaymentOffer,
  });

  final PaymentOffer paymentOffer;
  final ValueListenable<FiatRate?> fiatRate;

  final VoidCallback openSetAmountPage;
  final VoidCallback? refreshPaymentOffer;

  void onTapSetAmount() {
    openSetAmountPage();
  }

  void onTapEdit() {
    openSetAmountPage();
  }

  /// Copy the current page's offer code to the user clipboard.
  void onTapCopy(BuildContext context) {
    final code = this.paymentOffer.code;
    if (code == null) return;
    unawaited(LxClipboard.copyTextWithFeedback(context, code));
  }

  /// Try sharing the payment URI.
  Future<void> onTapShare(BuildContext context) async {
    final uri = this.paymentOffer.uri();
    if (uri == null) return;

    await LxShare.sharePaymentUri(context, uri);
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
    // final uri = null;
    // final uri = "lightning:lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r";
    // final uri =
    //     "lightning:lno1pqps7sjqpgtyzm3qv4uxzmtsd3jjqer9wd3hy6tsw35k7msjzfpy7nz5yqcnygrfdej82um5wf5k2uckyypwa3eyt44h6txtxquqh7lz5djge4afgfjn7k4rgrkuag0jsd5xvxg";
    // final uri = "lightning:lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r";
    // final uri = "bitcoin:bcrt1q2nfxmhd4n3c8834pj72xagvyr9gl57n5r94fsl";

    final amountSats = this.paymentOffer.amountSats;
    // final amountSats = 5300;
    // final amountSats = null;
    final amountSatsStr = (amountSats != null)
        ? currency_format.formatSatsAmount(amountSats, satsSuffix: false)
        : null;

    final description = this.paymentOffer.description;
    // final description = "the rice house 🍕";
    // final description = null;

    final isInvoice =
        this.paymentOffer.kind == PaymentOfferKind.lightningInvoice;
    final isOffer = this.paymentOffer.kind == PaymentOfferKind.lightningOffer;
    final isEditable = isInvoice || isOffer;

    final String? warningStr = this.paymentOffer.warningStr();

    return Container(
      margin: const EdgeInsets.symmetric(horizontal: Space.s200),
      constraints: const BoxConstraints(maxWidth: 350.0),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.start,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Offer kind title + info line
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
                Text(
                  this.paymentOffer.titleStr(),
                  style: const TextStyle(
                    color: LxColors.foreground,
                    fontSize: Fonts.size300,
                    fontVariations: [Fonts.weightMedium],
                    letterSpacing: -0.25,
                    height: 1.5,
                  ),
                ),
                Text(
                  this.paymentOffer.subtitleStr(),
                  style: const TextStyle(
                    color: LxColors.grey600,
                    fontSize: Fonts.size100,
                    height: 1.2,
                  ),
                ),
              ],
            ),
          ),

          // Card
          CardBox(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                // code + tertiary icons
                Row(
                  mainAxisAlignment: MainAxisAlignment.start,
                  children: [
                    // `<code> <copy-icon>` button
                    CopyCodeButtonOrPlaceholder(
                      code: code,
                      onTapCopy: this.onTapCopy,
                    ),

                    const Expanded(child: Center()),

                    // // TODO(phlip9): use "..." to show actions w/
                    // // human-readable labels
                    // Transform.translate(
                    //   offset: const Offset(Space.s200, 0.0),
                    //   child: IconButton(
                    //     onPressed: () {},
                    //     icon: const Icon(
                    //       LxIcons.moreHoriz,
                    //       opticalSize: LxIcons.opszSemiDense,
                    //     ),
                    //     visualDensity: VisualDensity.compact,
                    //   ),
                    // ),
                  ],
                ),
                const SizedBox(height: Space.s100),

                // QR code
                LayoutBuilder(
                  builder: (context, constraints) {
                    final double dim = constraints.maxWidth;
                    final key = ValueKey(uri ?? "");

                    // TODO(phlip9): likely perf issue with `clipBehavior` and
                    // AnimatedSwitcher. Pre-render QR with borderRadius?
                    // Use `FadeInImage`? Build custom `ImageProvider` for QR?
                    return AnimatedSwitcher(
                      duration: const Duration(milliseconds: 250),
                      child: (uri != null)
                          ? Container(
                              decoration: BoxDecoration(
                                borderRadius: BorderRadius.circular(6.0),
                              ),
                              clipBehavior: Clip.antiAlias,
                              child: InteractiveQrImage(
                                // `AnimatedSwitcher` should also run the switch
                                // animation when the QR code contents change.
                                key: key,
                                value: uri.toString(),
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

                if (!isEditable && amountSatsStr == null && description == null)
                  const SizedBox(height: Space.s300),

                // "Edit amount or description" button
                //
                // We only allow editing the amount for LN, since we can't yet
                // accurately correlate info we put in a BIP21 URI with the
                // actual tx that comes in.
                if (isEditable && amountSatsStr == null && description == null)
                  Padding(
                    padding: const EdgeInsets.only(
                      top: Space.s100,
                      bottom: Space.s100,
                    ),
                    child: Row(
                      mainAxisSize: MainAxisSize.max,
                      mainAxisAlignment: MainAxisAlignment.start,
                      children: [
                        Expanded(
                          child: TextButton.icon(
                            onPressed: this.onTapEdit,
                            style: const ButtonStyle(
                              padding: WidgetStatePropertyAll(EdgeInsets.zero),
                              visualDensity: VisualDensity(
                                horizontal: -3.0,
                                vertical: -3.0,
                              ),
                            ),
                            label: const Row(
                              mainAxisSize: MainAxisSize.max,
                              mainAxisAlignment: MainAxisAlignment.start,
                              children: [
                                Text(
                                  "Edit amount or description",
                                  style: TextStyle(
                                    fontSize: Fonts.size200,
                                    color: LxColors.fgSecondary,
                                    fontVariations: [Fonts.weightNormal],
                                    letterSpacing: -0.25,
                                  ),
                                ),
                              ],
                            ),
                            icon: const Icon(
                              LxIcons.edit,
                              size: Fonts.size300,
                              color: LxColors.fgSecondary,
                              opticalSize: LxIcons.opszDense,
                              weight: LxIcons.weightNormal,
                            ),
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
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      // Amount and/or description
                      Expanded(
                        child: Column(
                          mainAxisAlignment: MainAxisAlignment.start,
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            // Amount (sats)
                            if (amountSatsStr != null)
                              Padding(
                                padding: const EdgeInsets.only(
                                  bottom: Space.s100,
                                ),
                                child: Text.rich(
                                  TextSpan(
                                    children: [
                                      TextSpan(text: amountSatsStr),
                                      const TextSpan(
                                        text: " sats",
                                        style: TextStyle(
                                          color: LxColors.grey550,
                                        ),
                                      ),
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
                            ValueListenableBuilder(
                              valueListenable: this.fiatRate,
                              builder: (context, fiatRate, child) {
                                if (amountSats == null) {
                                  return const SizedBox.shrink();
                                }

                                final String? amountFiatStr;
                                if (fiatRate != null) {
                                  final amountFiat =
                                      fiatRate.rate *
                                      currency_format.satsToBtc(amountSats);
                                  amountFiatStr = currency_format.formatFiat(
                                    amountFiat,
                                    fiatRate.fiat,
                                  );
                                } else {
                                  amountFiatStr = null;
                                }

                                const fontSize = Fonts.size400;
                                const style = TextStyle(
                                  color: LxColors.fgTertiary,
                                  fontSize: fontSize,
                                  letterSpacing: -0.25,
                                  height: 1.0,
                                );

                                return (amountFiatStr != null)
                                    ? Text("≈ $amountFiatStr", style: style)
                                    : const FilledTextPlaceholder(
                                        width: Space.s900,
                                        color: LxColors.background,
                                        style: style,
                                      );
                              },
                            ),

                            if (amountSatsStr != null && description != null)
                              const SizedBox(height: Space.s300),

                            // Description
                            if (description != null)
                              Text(
                                description,
                                style: const TextStyle(
                                  color: LxColors.foreground,
                                  fontSize: Fonts.size200,
                                  height: 1.25,
                                  letterSpacing: -0.25,
                                ),
                                maxLines: 2,
                                overflow: TextOverflow.ellipsis,
                              ),

                            const SizedBox(height: Space.s300),
                          ],
                        ),
                      ),

                      // edit icon
                      Transform.translate(
                        // TODO(phlip9): this should be baseline aligned?
                        offset: (amountSatsStr != null)
                            ? const Offset(Space.s200, -Space.s200)
                            : const Offset(Space.s200, -Space.s300),
                        // offset: const Offset(Space.s200, 0.0),
                        child: TextButton.icon(
                          onPressed: this.onTapEdit,
                          label: const Text(
                            "Edit",
                            style: TextStyle(
                              fontSize: Fonts.size200,
                              color: LxColors.fgSecondary,
                              letterSpacing: -0.25,
                            ),
                          ),
                          icon: const Icon(
                            LxIcons.edit,
                            size: Fonts.size300,
                            color: LxColors.fgSecondary,
                          ),
                        ),
                      ),
                    ],
                  ),
              ],
            ),
          ),
          const SizedBox(height: Space.s400),

          // Under-card section

          // Warning/info block
          if (warningStr != null)
            Padding(
              padding: const EdgeInsets.only(
                left: Space.s450,
                right: Space.s200,
              ),
              child: Row(
                children: [
                  Expanded(
                    child: Text.rich(
                      TextSpan(
                        children: [
                          TextSpan(text: warningStr),

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
                        ],
                      ),
                      style: const TextStyle(
                        color: LxColors.grey550,
                        fontSize: Fonts.size100,
                        // letterSpacing: -0.2,
                      ),
                    ),
                  ),
                ],
              ),
            ),

          // Push elements outside page to bottom
          const Expanded(child: Center()),

          // Bottom action buttons
          Padding(
            padding: const EdgeInsets.only(top: Space.s300),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                // Copy code
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: Space.s200),
                  child: FilledButton(
                    onPressed: () => this.onTapCopy(context),
                    child: const Icon(LxIcons.copy),
                  ),
                ),

                // Share payment URI (w/ share code fallback)
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: Space.s200),
                  child: Builder(
                    // Use an extra Builder layer so the `sharePositionOrigin`
                    // is around just this button.
                    builder: (context) => FilledButton(
                      onPressed: () => this.onTapShare(context),
                      child: const Icon(LxIcons.share),
                    ),
                  ),
                ),

                // Refresh
                if (this.refreshPaymentOffer != null)
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: Space.s200),
                    child: FilledButton(
                      onPressed: this.refreshPaymentOffer,
                      child: const Icon(LxIcons.refresh),
                    ),
                  ),
              ],
            ),
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
  Widget build(BuildContext context) => Container(
    decoration: BoxDecoration(
      color: LxColors.grey1000,
      borderRadius: BorderRadius.circular(LxRadius.r300),
    ),
    padding: const EdgeInsets.fromLTRB(
      Space.s450,
      Space.s200,
      Space.s450,
      Space.s200,
    ),
    clipBehavior: Clip.antiAlias,
    child: child,
  );
}

/// The button at the top of the PaymentOfferPage that copies the code to the
/// clipboard. Displays a loading placeholder if the code is not yet available.
class CopyCodeButtonOrPlaceholder extends StatelessWidget {
  const CopyCodeButtonOrPlaceholder({
    super.key,
    this.code,
    required this.onTapCopy,
  });

  final String? code;
  final VoidContextCallback onTapCopy;

  @override
  Widget build(BuildContext context) {
    const double buttonWidth = Space.s950;
    const double buttonHeight = Space.s600;
    const double buttonPadHoriz = Space.s300;

    const double fontSize = Fonts.size100;
    const Color fontColor = LxColors.grey550;

    final code = this.code;

    // raw code string + copy button
    if (code != null) {
      // align text with QR code
      return Transform.translate(
        offset: const Offset(-buttonPadHoriz, Space.s0),
        child: TextButton.icon(
          onPressed: () => this.onTapCopy(context),
          icon: Text(
            address_format.ellipsizeBtcAddress(code),
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(fontSize: fontSize, color: fontColor),
          ),
          label: const Icon(
            LxIcons.copy,
            opticalSize: LxIcons.opszDense,
            weight: LxIcons.weightNormal,
            size: Fonts.size300,
            color: fontColor,
          ),
          // Make button sizing more deterministic so we can
          // size the placeholder more accurately.
          style: const ButtonStyle(
            padding: WidgetStatePropertyAll(
              EdgeInsets.symmetric(
                vertical: Space.s0,
                horizontal: buttonPadHoriz,
              ),
            ),
            minimumSize: WidgetStatePropertyAll(
              Size(buttonWidth, buttonHeight),
            ),
            maximumSize: WidgetStatePropertyAll(Size.fromHeight(buttonHeight)),
            visualDensity: VisualDensity(horizontal: 0.0, vertical: 0.0),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
        ),
      );
    } else {
      return const SizedBox(
        width: buttonWidth,
        height: buttonHeight,
        child: Center(
          child: FilledTextPlaceholder(
            color: LxColors.background,
            style: TextStyle(fontSize: fontSize, color: fontColor),
          ),
        ),
      );
    }
  }
}

// const bottomSheetBodyPadding = Space.s600;
//
// class ReceiveSettingsBottomSheet extends StatelessWidget {
//   const ReceiveSettingsBottomSheet({super.key, required this.kind});
//
//   final PaymentOfferKind kind;
//
//   void onKindSelected(BuildContext context, PaymentOfferKind flowResult) {
//     info("ReceiveSettingsBottomSheet: selected kind: $flowResult");
//     unawaited(Navigator.of(context).maybePop(flowResult));
//   }
//
//   @override
//   Widget build(BuildContext context) {
//     return Theme(
//       data: LxTheme.light(),
//       child: Column(
//         mainAxisSize: MainAxisSize.min,
//         crossAxisAlignment: CrossAxisAlignment.start,
//         children: [
//           const SheetDragHandle(),
//           const SizedBox(height: Space.s200),
//           const Padding(
//             padding: EdgeInsets.symmetric(
//                 horizontal: bottomSheetBodyPadding, vertical: Space.s300),
//             child: HeadingText(text: "Receive settings"),
//           ),
//
//           // Lightning
//           if (this.kind.isLightning())
//             PaymentOfferKindRadio(
//               kind: PaymentOfferKind.lightningInvoice,
//               selected: this.kind,
//               title: const Text("Lightning invoice"),
//               subtitle: const Text(
//                   "Widely supported. Invoices can only be paid once!"),
//               onChanged: (kind) => this.onKindSelected(context, kind),
//             ),
//           if (this.kind.isLightning())
//             PaymentOfferKindRadio(
//               kind: PaymentOfferKind.lightningOffer,
//               selected: this.kind,
//               title: const Text("Lightning offer"),
//               subtitle: const Text(
//                   "New. Offers can be paid many times. Paste one on your twitter!"),
//               // TODO_(phlip9): uncomment when BOLT12 offers are supported.
//               // onChanged: (kind) => this.onKindSelected(context, kind),
//               onChanged: null,
//             ),
//
//           // BTC
//           if (!this.kind.isLightning())
//             PaymentOfferKindRadio(
//               kind: PaymentOfferKind.btcAddress,
//               selected: this.kind,
//               title: const Text("Bitcoin SegWit address"),
//               subtitle: const Text("Recommended. Supported by most wallets."),
//               onChanged: (kind) => this.onKindSelected(context, kind),
//             ),
//           if (!this.kind.isLightning())
//             PaymentOfferKindRadio(
//               kind: PaymentOfferKind.btcTaproot,
//               selected: this.kind,
//               title: const Text("Bitcoin Taproot address"),
//               subtitle: const Text(
//                   "Newer format. Reduced fees and increased privacy."),
//               // TODO_(phlip9): uncomment when taproot addresses are supported.
//               // onChanged: (kind) => this.onKindSelected(context, kind),
//               onChanged: null,
//             ),
//           const SizedBox(height: Space.s600),
//         ],
//       ),
//     );
//   }
// }
//
// class PaymentOfferKindRadio extends StatelessWidget {
//   const PaymentOfferKindRadio({
//     super.key,
//     required this.kind,
//     required this.selected,
//     required this.title,
//     required this.subtitle,
//     required this.onChanged,
//   });
//
//   final PaymentOfferKind kind;
//   final PaymentOfferKind selected;
//
//   final Widget title;
//   final Widget subtitle;
//
//   final void Function(PaymentOfferKind)? onChanged;
//
//   @override
//   Widget build(BuildContext context) {
//     final onChanged = this.onChanged;
//
//     return RadioListTile<PaymentOfferKind>(
//       toggleable: false,
//       controlAffinity: ListTileControlAffinity.trailing,
//       contentPadding:
//           const EdgeInsets.symmetric(horizontal: bottomSheetBodyPadding),
//       value: this.kind,
//       groupValue: this.selected,
//       onChanged: (onChanged != null) ? (kind) => onChanged(kind!) : null,
//       title: this.title,
//       subtitle: this.subtitle,
//     );
//   }
// }

/// A page for the user to set a desired amount and optional description on
/// their payment offer.
class ReceivePaymentEditInvoicePage extends StatefulWidget {
  const ReceivePaymentEditInvoicePage({super.key, required this.prev});

  final LnInvoiceInputs prev;

  @override
  State<ReceivePaymentEditInvoicePage> createState() =>
      _ReceivePaymentEditInvoicePageState();
}

class _ReceivePaymentEditInvoicePageState
    extends State<ReceivePaymentEditInvoicePage> {
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

    final flowResult = LnInvoiceInputs(
      amountSats: amountSats,
      description: description,
    );
    unawaited(Navigator.of(this.context).maybePop(flowResult));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Set amount and description"),
          const SubheadingText(
            text: "Both amount and description are optional",
          ),
          const SizedBox(height: Space.s700),

          // <amount> sats
          PaymentAmountInput(
            fieldKey: this.amountFieldKey,
            intInputFormatter: this.intInputFormatter,
            allowEmpty: true,
            allowZero: true,
            initialValue: this.widget.prev.amountSats,
          ),

          const SizedBox(height: Space.s700),

          PaymentNoteInput(
            fieldKey: this.descriptionFieldKey,
            onSubmit: this.onConfirm,
            initialNote: this.widget.prev.description,
            hintText: "Optional description (visible to sender)",
          ),

          const SizedBox(height: Space.s400),
        ],
        bottom: LxFilledButton(
          label: const Text("Confirm"),
          icon: const Icon(LxIcons.next),
          onTap: this.onConfirm,
        ),
      ),
    );
  }
}
