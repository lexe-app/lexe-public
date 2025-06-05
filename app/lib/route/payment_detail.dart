import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/api.dart' show FiatRate, UpdatePaymentNote;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart'
    show Payment, PaymentDirection, PaymentIndex, PaymentKind, PaymentStatus;
import 'package:app_rs_dart/ffi/types.ext.dart';
import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';
import 'package:lexeapp/block_explorer.dart' as block_explorer;
import 'package:lexeapp/components.dart'
    show
        FilledTextPlaceholder,
        InfoCard,
        InfoRow,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        LxRefreshButton,
        PaymentNoteInput,
        ScrollableSinglePageBody,
        SheetDragHandle,
        SliverPullToRefresh;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/notifier_ext.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;
import 'package:lexeapp/url.dart' as url;

/// A bit of a hack so we can display "reasonable" Payment info immediately
/// after sending a payment, but before we've synced our local payment DB.
sealed class PaymentSource {
  static PaymentSource localDb(int vecIdx) => PaymentSourceLocalDb(vecIdx);
  static PaymentSource unsynced(Payment payment) =>
      PaymentSourceUnsynced(payment);
}

final class PaymentSourceLocalDb implements PaymentSource {
  const PaymentSourceLocalDb(this.vecIdx);
  final int vecIdx;
}

final class PaymentSourceUnsynced implements PaymentSource {
  const PaymentSourceUnsynced(this.payment);
  final Payment payment;
}

/// A page for displaying a single payment, in detail.
///
/// Ex: tapping a payment in the wallet page payments list will open this page
///     for that payment.
/// Ex: after making a payment, we will immediately open this page for the user
///     to track the settlement status.
class PaymentDetailPage extends StatefulWidget {
  const PaymentDetailPage({
    super.key,
    required this.app,
    required this.paymentIndex,
    required this.paymentSource,
    required this.paymentsUpdated,
    required this.fiatRate,
    required this.isSyncing,
    required this.triggerRefresh,
  });

  final AppHandle app;

  /// The id of the payment we want to display.
  final PaymentIndex paymentIndex;

  /// Is the payment already synced in the local db (tap payment in list) vs
  /// display one immediately after a send (not yet synced to local db).
  final PaymentSource paymentSource;

  /// We receive a notification on this [Stream]
  final Listenable paymentsUpdated;

  /// A stream of [FiatRate] (user's preferred fiat + its exchange rate). May
  /// be null if we're still fetching the rates at startup.
  final ValueListenable<FiatRate?> fiatRate;

  /// True when we are currently syncing payments from our node.
  final ValueListenable<bool> isSyncing;

  /// Call this function to (maybe) start a new refresh. Will do nothing if
  /// we're currently refreshing.
  final VoidCallback triggerRefresh;

  @override
  State<PaymentDetailPage> createState() => _PaymentDetailPageState();
}

class _PaymentDetailPageState extends State<PaymentDetailPage> {
  // When this stream ticks, all the payments' createdAt label should update.
  // This stream ticks every 30 seconds.
  final DateTimeNotifier paymentDateUpdates =
      DateTimeNotifier(period: const Duration(seconds: 30));

  /// If `unsynced`, we'll switch the source to `localDb` after it gets synced.
  late PaymentSource paymentSource = this.widget.paymentSource;

  late final ValueNotifier<Payment> payment;
  late final LxListener paymentsUpdatedListener;

  @override
  void dispose() {
    this.payment.dispose();
    this.paymentDateUpdates.dispose();
    this.paymentsUpdatedListener.dispose();
    super.dispose();
  }

  @override
  void initState() {
    super.initState();

    // Get the current payment
    this.payment = ValueNotifier(this.getPaymentInitially());

    // Start listening for payment updates
    this.paymentsUpdatedListener =
        this.widget.paymentsUpdated.listen(this.onPaymentsUpdated);

    // HACK(phlip9): mitigate race b/w triggering refresh after send
    // and opening the page + starting to listen for the payment updated event.
    unawaited(Future.delayed(const Duration(seconds: 500), () async {
      if (!this.mounted) return;
      await this.onPaymentsUpdated();
    }));
  }

  // Can't async in `initState`
  Payment getPaymentInitially() {
    switch (this.paymentSource) {
      case PaymentSourceUnsynced(:final payment):
        return payment;
      case PaymentSourceLocalDb(:final vecIdx):
        return this.getPaymentByVecIdx(vecIdx);
    }
  }

  /// Get the Payment. If we know the payment is in our local DB, this just gets
  /// it. Otherwise, _check_ if it's in our DB and use that from now on,
  /// else fallback to `unsynced`.
  Future<Payment> getPaymentAfterUpdate() async {
    final int paymentVecIdx;
    switch (this.paymentSource) {
      case PaymentSourceLocalDb(:final vecIdx):
        paymentVecIdx = vecIdx;
      case PaymentSourceUnsynced(:final payment):
        final maybeVecIdx = await this
            .widget
            .app
            .getVecIdxByPaymentIndex(paymentIndex: payment.index);

        // Still not synced yet, keep displaying the unsynced payment
        if (maybeVecIdx == null) {
          return payment;
        }

        // Payment is synced, can get by local db idx now
        this.paymentSource = PaymentSourceLocalDb(maybeVecIdx);
        paymentVecIdx = maybeVecIdx;
    }

    return this.getPaymentByVecIdx(paymentVecIdx);
  }

  /// [AppHandle.getPaymentByVecIdx] but we expect the payment to be in the
  /// local db. Throws otherwise.
  Payment getPaymentByVecIdx(final int vecIdx) {
    // O/w get the payment from the local DB.
    final payment = this.widget.app.getPaymentByVecIdx(vecIdx: vecIdx);
    if (payment == null) {
      throw StateError(
          "PaymentDb is in an invalid state: missing payment @ vec_idx: "
          "$vecIdx, payment_index: ${this.widget.paymentIndex}");
    }
    return payment;
  }

  /// After we sync some new payments, fetch the payment from the local db.
  Future<void> onPaymentsUpdated() async {
    final payment = await this.getPaymentAfterUpdate();
    if (!this.mounted) return;
    this.payment.value = payment;
  }

  @override
  Widget build(BuildContext context) {
    return PaymentDetailPageInner(
      app: this.widget.app,
      payment: this.payment,
      paymentDateUpdates: this.paymentDateUpdates,
      fiatRate: this.widget.fiatRate,
      isSyncing: this.widget.isSyncing,
      triggerRefresh: this.widget.triggerRefresh,
    );
  }
}

const double pagePadding = Space.s400;
const double bodyPadding = Space.s300;

class PaymentDetailPageInner extends StatelessWidget {
  const PaymentDetailPageInner({
    super.key,
    required this.app,
    required this.payment,
    required this.paymentDateUpdates,
    required this.fiatRate,
    required this.triggerRefresh,
    required this.isSyncing,
  });

  final AppHandle app;
  final ValueListenable<Payment> payment;
  final ValueListenable<DateTime> paymentDateUpdates;
  final ValueListenable<FiatRate?> fiatRate;
  final ValueListenable<bool> isSyncing;
  final VoidCallback triggerRefresh;

  // HACK: parsing the serialized form like this is ugly af.
  String paymentIdxBody() => this.payment.value.index.body();

  void openBottomSheet(BuildContext context) {
    unawaited(showModalBottomSheet(
      backgroundColor: LxColors.background,
      elevation: 0.0,
      clipBehavior: Clip.hardEdge,
      enableDrag: true,
      isDismissible: true,
      isScrollControlled: true,
      context: context,
      builder: (context) => PaymentDetailBottomSheet(
        payment: this.payment,
        fiatRate: this.fiatRate,
      ),
    ));
  }

  @override
  Widget build(BuildContext context) {
    const pagePaddingInsets = EdgeInsets.symmetric(horizontal: pagePadding);

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(isLeading: true),
        actions: [
          LxRefreshButton(
            isRefreshing: this.isSyncing,
            triggerRefresh: this.triggerRefresh,
          ),
          const SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ValueListenableBuilder(
        valueListenable: this.payment,
        builder: (context, payment, _child) {
          final kind = payment.kind;
          final status = payment.status;
          final direction = payment.direction;
          final createdAt = DateTime.fromMillisecondsSinceEpoch(
              payment.createdAt,
              isUtc: true);
          final maybeAmountSat = payment.amountSat;
          final txid = payment.txid;

          return ScrollableSinglePageBody(
            padding: pagePaddingInsets,
            bodySlivers: [
              // Pull-to-refresh, but only when payment is pending.
              SliverPullToRefresh(
                onRefresh: (status == PaymentStatus.pending)
                    ? this.triggerRefresh
                    : null,
              ),

              // Payment detail body
              SliverList.list(children: [
                const SizedBox(height: Space.s500),

                // Big LN/BTC icon + status badge
                Align(
                  alignment: Alignment.topCenter,
                  child: PaymentDetailIcon(
                    kind: kind,
                    status: status,
                  ),
                ),

                const SizedBox(height: Space.s500),

                // Direction + short time
                ValueListenableBuilder(
                  valueListenable: this.paymentDateUpdates,
                  builder: (_, now, child) => PaymentDetailDirectionTime(
                    status: status,
                    direction: direction,
                    createdAt: createdAt,
                    now: now,
                  ),
                ),
                const SizedBox(height: Space.s200),

                // TODO(phlip9): LN invoice "expires in X min" goes here?
                // If pending or failed, show a card with more info on the
                // current status.
                if (status != PaymentStatus.completed)
                  Padding(
                    // padding: const EdgeInsets.only(top: Space.s200, bottom: Space.s200),
                    padding: const EdgeInsets.symmetric(
                        vertical: Space.s200, horizontal: Space.s600),
                    child: PaymentDetailStatusCard(
                      status: status,
                      statusStr: payment.statusStr,
                    ),
                  ),
                const SizedBox(height: Space.s600),

                // Amount sent/received in BTC and fiat.
                if (maybeAmountSat != null)
                  ValueListenableBuilder(
                    valueListenable: this.fiatRate,
                    builder: (_context, fiatRate, child) =>
                        PaymentDetailPrimaryAmount(
                      status: status,
                      direction: direction,
                      amountSat: maybeAmountSat,
                      fiatRate: fiatRate,
                    ),
                  ),
                const SizedBox(height: Space.s600),

                // The payment's note field
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: bodyPadding),
                  child: PaymentDetailNoteInput(
                    app: this.app,
                    paymentIndex: payment.index,
                    initialNote: payment.note,
                  ),
                ),
                const SizedBox(height: Space.s600),
              ]),
            ],

            // Payment details button
            // -> opens a modal bottom sheet with the complete payment info
            bottomPadding: const EdgeInsets.symmetric(
              horizontal: pagePadding,
              vertical: Space.s600,
            ),
            bottom: Column(
              mainAxisSize: MainAxisSize.min,
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                // View in block explorer
                if (txid != null)
                  Padding(
                    padding: const EdgeInsets.only(bottom: Space.s200),
                    child: LxFilledButton(
                      onTap: () => url.open(block_explorer.txid(txid)),
                      label: const Text("View in block explorer"),
                      icon: const Icon(LxIcons.openLink),
                    ),
                  ),

                // Open payment details bottom sheet
                LxFilledButton(
                  onTap: () => this.openBottomSheet(context),
                  label: const Text("Payment details"),
                  icon: const Icon(LxIcons.expandUp),
                ),
              ],
            ),
          );
        },
      ),
    );
  }
}

String formatSatsAmountFiatBelow(int amountSats, FiatRate? fiatRate) {
  final amountSatsStr =
      currency_format.formatSatsAmount(amountSats, satsSuffix: true);
  if (fiatRate != null) {
    final fiatAmount = currency_format.satsToBtc(amountSats) * fiatRate.rate;
    final fiatAmountStr = currency_format.formatFiat(fiatAmount, fiatRate.fiat);
    return "$amountSatsStr\n≈ $fiatAmountStr (now)";
  } else {
    return "$amountSatsStr\n";
  }
}

/// The complete payment details sheet. Opens when the "Payment details" button
/// is pressed. This sheet should contain all the structured payment info that
/// a user normally shouldn't need to be aware of, but might be useful while
/// debugging or auditing.
class PaymentDetailBottomSheet extends StatelessWidget {
  const PaymentDetailBottomSheet({
    super.key,
    required this.payment,
    required this.fiatRate,
  });

  final ValueListenable<Payment> payment;
  final ValueListenable<FiatRate?> fiatRate;

  String paymentIdxBody() => this.payment.value.index.body();

  @override
  Widget build(BuildContext context) {
    return DraggableScrollableSheet(
      initialChildSize: 0.6,
      maxChildSize: 0.6,
      minChildSize: 0.0,
      expand: false,
      shouldCloseOnMinExtent: true,
      // by default, SnackBar's are covered by the bottomSheet, so wrap
      // everything here in a Scaffold so SnackBar's actually get displayed.
      builder: (context, scrollController) => Scaffold(
        backgroundColor: LxColors.clearW0,
        body: Padding(
          padding: const EdgeInsets.symmetric(horizontal: pagePadding),
          child: CustomScrollView(
            controller: scrollController,
            slivers: [
              ValueListenableBuilder(
                valueListenable: this.payment,
                builder: (context, payment, _child) {
                  final kind = payment.kind;
                  final status = payment.status;
                  final direction = payment.direction;
                  final directionLabel = (direction == PaymentDirection.inbound)
                      ? "received"
                      : "sent";

                  final invoice = payment.invoice;
                  final payeePubkey = invoice?.payeePubkey;

                  final offer = payment.offer;
                  final offerExpiresAt = offer?.expiresAt;
                  final offerAmountSat = offer?.amountSats;

                  final txid = payment.txid;
                  final replacement = payment.replacement;

                  final amountSat = payment.amountSat;
                  final feesSat = payment.feesSat;
                  final invoiceAmountSat = invoice?.amountSats;

                  final createdAt = DateTime.fromMillisecondsSinceEpoch(
                    payment.createdAt,
                    isUtc: true,
                  );
                  final expiresAt = (invoice != null &&
                          status != PaymentStatus.completed)
                      ? DateTime.fromMillisecondsSinceEpoch(invoice.expiresAt,
                          isUtc: true)
                      : (offerExpiresAt != null &&
                              status != PaymentStatus.completed)
                          ? DateTime.fromMillisecondsSinceEpoch(offerExpiresAt,
                              isUtc: true)
                          : null;
                  final maybeFinalizedAt = payment.finalizedAt;
                  final finalizedAt = (maybeFinalizedAt != null)
                      ? DateTime.fromMillisecondsSinceEpoch(maybeFinalizedAt,
                          isUtc: true)
                      : null;

                  // Label should be kept in sync with "lexe_api::types::payments::LxPaymentId"
                  final InfoRow? paymentIdxRow = switch ((kind, direction)) {
                    // Onchain receive -> we'll use the txid field
                    (PaymentKind.onchain, PaymentDirection.inbound) => null,
                    (PaymentKind.onchain, PaymentDirection.outbound) => InfoRow(
                        label: "Client payment id",
                        value: this.paymentIdxBody()),
                    (PaymentKind.invoice, _) => InfoRow(
                        label: "Payment hash", value: this.paymentIdxBody()),
                    (PaymentKind.spontaneous, _) => InfoRow(
                        label: "Payment hash", value: this.paymentIdxBody()),
                    (PaymentKind.offer, PaymentDirection.inbound) => InfoRow(
                        label: "Offer claim id", value: this.paymentIdxBody()),
                    (PaymentKind.offer, PaymentDirection.outbound) => InfoRow(
                        label: "Client payment id",
                        value: this.paymentIdxBody()),
                  };

                  // Show on-chain txid's with link to mempool.space
                  final InfoRow? txidRow = (txid != null)
                      ? InfoRow(
                          label: "Txid",
                          value: txid,
                          linkTarget: block_explorer.txid(txid),
                        )
                      : null;
                  final InfoRow? replacementRow = (replacement != null)
                      ? InfoRow(
                          label: "Replacement txid",
                          value: replacement,
                          linkTarget: block_explorer.txid(replacement),
                        )
                      : null;

                  return SliverList.list(children: [
                    const SheetDragHandle(),

                    // Sheet heading and close button
                    const Padding(
                      padding: EdgeInsets.only(
                          left: bodyPadding,
                          top: Space.s200,
                          bottom: Space.s400),
                      child: Row(
                        mainAxisAlignment: MainAxisAlignment.spaceBetween,
                        crossAxisAlignment: CrossAxisAlignment.center,
                        children: [
                          Text(
                            "Payment details",
                            style: TextStyle(
                              fontSize: Fonts.size600,
                              fontVariations: [Fonts.weightMedium],
                              letterSpacing: -0.5,
                              height: 1.0,
                            ),
                          ),
                          LxCloseButton(kind: LxCloseButtonKind.closeFromTop),
                        ],
                      ),
                    ),

                    // Payment date info
                    PaymentDetailInfoCard(children: [
                      InfoRow(
                        label: "Created at",
                        value: date_format.formatDateFull(createdAt),
                      ),
                      if (expiresAt != null)
                        InfoRow(
                          label: "Expires at",
                          value: date_format.formatDateFull(expiresAt),
                        ),
                      if (finalizedAt != null)
                        InfoRow(
                          label: "Finalized at",
                          value: date_format.formatDateFull(finalizedAt),
                        ),
                    ]),

                    // Full payment amount + fees info
                    // TODO(phlip9): deemphasize fiat amount below
                    ValueListenableBuilder(
                      valueListenable: this.fiatRate,
                      builder: (_context, fiatRate, child) =>
                          PaymentDetailInfoCard(children: [
                        if (amountSat != null)
                          InfoRow(
                            label: "Amount $directionLabel",
                            value:
                                formatSatsAmountFiatBelow(amountSat, fiatRate),
                          ),

                        if (invoiceAmountSat != null)
                          InfoRow(
                            label: "Invoiced amount",
                            value: formatSatsAmountFiatBelow(
                                invoiceAmountSat, fiatRate),
                          ),

                        if (offerAmountSat != null)
                          InfoRow(
                            label: "Offer amount",
                            value: formatSatsAmountFiatBelow(
                                offerAmountSat, fiatRate),
                          ),

                        // TODO(phlip9): breakdown fees
                        InfoRow(
                          label: "Fees",
                          value: formatSatsAmountFiatBelow(feesSat, fiatRate),
                        ),
                      ]),
                    ),

                    // Low-level stuff
                    PaymentDetailInfoCard(children: [
                      // oneof: LN payment hash, Lx ClientPaymentId
                      if (paymentIdxRow != null) paymentIdxRow,

                      // Txid
                      if (txidRow != null) txidRow,
                      // Replacement Txid
                      if (replacementRow != null) replacementRow,

                      // LN payee pubkey
                      if (payeePubkey != null)
                        InfoRow(label: "Payee public key", value: payeePubkey),

                      // the full invoice
                      if (invoice != null)
                        InfoRow(label: "Invoice", value: invoice.string),

                      // the full offer
                      if (offer != null)
                        InfoRow(label: "Offer", value: offer.string),
                    ]),

                    const SizedBox(height: Space.s400)
                  ]);
                },
              )
            ],
          ),
        ),
      ),
    );
  }
}

class PaymentDetailIcon extends StatelessWidget {
  const PaymentDetailIcon({
    super.key,
    required this.kind,
    required this.status,
  });

  final PaymentKind kind;
  final PaymentStatus status;

  @override
  Widget build(BuildContext context) {
    final isLightning = this.kind.isLightning();
    const size = Space.s700;
    const color = LxColors.fgSecondary;

    final icon = DecoratedBox(
      decoration: const BoxDecoration(
        color: LxColors.grey825,
        borderRadius: BorderRadius.all(Radius.circular(Space.s800 / 2)),
      ),
      child: SizedBox.square(
        dimension: Space.s800,
        child: (isLightning)
            ? const Icon(
                LxIcons.lightning,
                size: size,
                color: color,
                fill: 1.0,
                weight: LxIcons.weightExtraLight,
              )
            : const Icon(
                LxIcons.bitcoin,
                size: size,
                color: color,
              ),
      ),
    );

    return switch (this.status) {
      PaymentStatus.completed => PaymentDetailIconBadge(
          icon: LxIcons.completedBadge,
          color: LxColors.background,
          backgroundColor: LxColors.moneyGoUp,
          child: icon,
        ),
      PaymentStatus.pending => PaymentDetailIconBadge(
          icon: LxIcons.pendingBadge,
          color: LxColors.background,
          // Use "green" also for pending. Assume payments will generally be
          // successful. Don't scare users.
          // TODO(phlip9): use a warning yellow after several hours of pending?
          backgroundColor: LxColors.moneyGoUp,
          child: icon,
        ),
      PaymentStatus.failed => PaymentDetailIconBadge(
          icon: LxIcons.failedBadge,
          color: LxColors.background,
          backgroundColor: LxColors.errorText,
          child: icon,
        ),
    };
  }
}

class PaymentDetailIconBadge extends StatelessWidget {
  const PaymentDetailIconBadge({
    super.key,
    required this.icon,
    required this.color,
    required this.backgroundColor,
    required this.child,
  });

  final IconData icon;
  final Color color;
  final Color backgroundColor;
  final Widget child;

  @override
  Widget build(BuildContext context) => Badge(
        label: Icon(
          this.icon,
          size: Fonts.size400,
          color: this.color,
        ),
        backgroundColor: this.backgroundColor,
        largeSize: Space.s500,
        child: this.child,
      );
}

class PaymentDetailDirectionTime extends StatelessWidget {
  const PaymentDetailDirectionTime({
    super.key,
    required this.status,
    required this.direction,
    required this.createdAt,
    required this.now,
  });

  final PaymentStatus status;
  final PaymentDirection direction;
  final DateTime createdAt;
  final DateTime now;

  @override
  Widget build(BuildContext context) {
    final directionLabel = switch ((status, direction)) {
      ((PaymentStatus.pending, PaymentDirection.inbound)) => "Receiving",
      ((PaymentStatus.pending, PaymentDirection.outbound)) => "Sending",
      ((PaymentStatus.completed, PaymentDirection.inbound)) => "Received",
      ((PaymentStatus.completed, PaymentDirection.outbound)) => "Sent",
      ((PaymentStatus.failed, PaymentDirection.inbound)) => "Failed to receive",
      ((PaymentStatus.failed, PaymentDirection.outbound)) => "Failed to send",
    };

    final createdAtStr = date_format.formatDate(then: createdAt, now: now);

    return Text.rich(
      TextSpan(
        children: <TextSpan>[
          TextSpan(
            text: directionLabel,
            style: const TextStyle(fontVariations: [Fonts.weightSemiBold]),
          ),
          const TextSpan(text: " · "),
          TextSpan(
              text: createdAtStr,
              style: const TextStyle(color: LxColors.fgSecondary)),
        ],
        style: Fonts.fontBody.copyWith(
          // letterSpacing: -0.5,
          fontSize: Fonts.size300,
          fontVariations: [Fonts.weightMedium],
        ),
      ),
      textAlign: TextAlign.center,
    );
  }
}

class PaymentDetailStatusCard extends StatelessWidget {
  const PaymentDetailStatusCard(
      {super.key, required this.status, required this.statusStr})
      : assert(status != PaymentStatus.completed);

  final PaymentStatus status;
  final String statusStr;

  @override
  Widget build(BuildContext context) {
    return Card(
      color: LxColors.grey1000,
      elevation: 0.0,
      margin: const EdgeInsets.all(0),
      child: Padding(
        padding: const EdgeInsets.all(Space.s400),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            Expanded(
              flex: 2,
              child: Text(
                (this.status == PaymentStatus.pending) ? "pending" : "failed",
                style: Fonts.fontBody.copyWith(
                  fontSize: Fonts.size300,
                  color: LxColors.foreground,
                  fontVariations: [Fonts.weightSemiBold],
                  height: 1.0,
                ),
                textAlign: TextAlign.center,
              ),
            ),
            const SizedBox(width: Space.s400),
            Expanded(
              flex: 4,
              child: Text(
                this.statusStr,
                style: Fonts.fontBody.copyWith(
                  letterSpacing: -0.25,
                  fontSize: Fonts.size200,
                  color: LxColors.fgSecondary,
                  fontVariations: [Fonts.weightNormal],
                  height: 1.3,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class PaymentDetailPrimaryAmount extends StatelessWidget {
  const PaymentDetailPrimaryAmount({
    super.key,
    required this.status,
    required this.direction,
    required this.amountSat,
    this.fiatRate,
  });

  final PaymentStatus status;
  final PaymentDirection direction;
  final int amountSat;
  final FiatRate? fiatRate;

  String? maybeAmountFiatStr() {
    final fiatRate = this.fiatRate;
    if (fiatRate == null) {
      return null;
    }

    final amountBtc = currency_format.satsToBtc(this.amountSat);
    final amountFiat = amountBtc * fiatRate.rate;
    return currency_format.formatFiat(amountFiat, fiatRate.fiat);
  }

  @override
  Widget build(BuildContext context) {
    final amountSatsStr = currency_format.formatSatsAmount(this.amountSat,
        direction: this.direction, satsSuffix: true);

    final maybeAmountFiatStr = this.maybeAmountFiatStr();

    final amountColor = switch ((this.status, this.direction)) {
      ((PaymentStatus.failed, _)) => LxColors.fgTertiary,
      ((_, PaymentDirection.inbound)) => LxColors.moneyGoUp,
      ((_, PaymentDirection.outbound)) => LxColors.fgSecondary,
    };

    final fiatStyle = Fonts.fontUI.copyWith(
      letterSpacing: -0.5,
      fontSize: Fonts.size500,
      fontVariations: [Fonts.weightNormal],
      fontFeatures: [Fonts.featSlashedZero],
      color: LxColors.fgTertiary,
    );

    return Column(
      mainAxisAlignment: MainAxisAlignment.start,
      children: [
        Text(
          amountSatsStr,
          style: Fonts.fontUI.copyWith(
            letterSpacing: -0.5,
            fontSize: Fonts.size800,
            fontVariations: [Fonts.weightNormal],
            fontFeatures: [Fonts.featSlashedZero],
            color: amountColor,
          ),
          textAlign: TextAlign.center,
        ),
        Padding(
          padding: const EdgeInsets.only(top: Space.s300),
          child: (maybeAmountFiatStr != null)
              ? Text(
                  "≈ $maybeAmountFiatStr",
                  style: fiatStyle,
                  textAlign: TextAlign.center,
                )
              : FilledTextPlaceholder(
                  width: Space.s1000,
                  style: fiatStyle,
                ),
        ),
      ],
    );
  }
}

class PaymentDetailNoteInput extends StatefulWidget {
  const PaymentDetailNoteInput({
    super.key,
    required this.app,
    required this.paymentIndex,
    required this.initialNote,
  });

  final AppHandle app;
  final PaymentIndex paymentIndex;
  final String? initialNote;

  @override
  State<PaymentDetailNoteInput> createState() => _PaymentDetailNoteInputState();
}

class _PaymentDetailNoteInputState extends State<PaymentDetailNoteInput> {
  final GlobalKey<FormFieldState<String>> fieldKey = GlobalKey();

  final ValueNotifier<String?> submitError = ValueNotifier(null);
  final ValueNotifier<bool> isSubmitting = ValueNotifier(false);

  @override
  void dispose() {
    this.submitError.dispose();
    this.isSubmitting.dispose();
    super.dispose();
  }

  Future<void> onSubmit() async {
    if (this.isSubmitting.value) return;

    this.isSubmitting.value = true;
    this.submitError.value = null;

    final req = UpdatePaymentNote(
      index: this.widget.paymentIndex,
      note: this.fieldKey.currentState!.value,
    );
    final result = await Result.tryFfiAsync(
        () async => this.widget.app.updatePaymentNote(req: req));

    if (!this.mounted) return;

    switch (result) {
      case Ok():
        this.isSubmitting.value = false;
        this.submitError.value = null;
        return;

      case Err(:final err):
        error("PaymentDetailNoteInput: error updating note: $err");
        this.isSubmitting.value = false;
        this.submitError.value = err.message;
        return;
    }
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Header
        Row(
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            const Text("Payment note",
                style: TextStyle(
                  fontSize: Fonts.size200,
                  color: LxColors.fgTertiary,
                )),
            const SizedBox(width: Space.s400),

            // Show a small spinner while submitting.
            ValueListenableBuilder(
              valueListenable: this.isSubmitting,
              child: const SizedBox.square(
                dimension: Fonts.size200,
                child: CircularProgressIndicator(
                  strokeWidth: 1.0,
                  color: LxColors.fgTertiary,
                  strokeCap: StrokeCap.round,
                ),
              ),
              builder: (_context, submitting, child) => AnimatedOpacity(
                opacity: submitting ? 1.0 : 0.0,
                duration: const Duration(milliseconds: 150),
                child: child,
              ),
            ),
          ],
        ),
        const SizedBox(height: Space.s200),

        // note text field
        ValueListenableBuilder(
          valueListenable: this.isSubmitting,
          builder: (_context, submitting, _child) => PaymentNoteInput(
            fieldKey: this.fieldKey,
            onSubmit: this.onSubmit,
            initialNote: this.widget.initialNote,
            isEnabled: !submitting,
          ),
        ),
      ],
    );
  }
}

/// [InfoCard] but uses the shared [bodyPadding] constant on this page.
class PaymentDetailInfoCard extends InfoCard {
  const PaymentDetailInfoCard({
    super.key,
    required super.children,
    super.header,
  }) : super(bodyPadding: bodyPadding);
}
