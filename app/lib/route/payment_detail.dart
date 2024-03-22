import 'dart:async' show Timer, unawaited;

import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';
import 'package:rxdart_ext/rxdart_ext.dart';

import '../bindings_generated_api.dart'
    show
        AppHandle,
        FiatRate,
        Payment,
        PaymentDirection,
        PaymentKind,
        PaymentStatus,
        UpdatePaymentNote;
import '../components.dart'
    show
        FilledPlaceholder,
        HeadingText,
        LxCloseButton,
        LxRefreshButton,
        PaymentNoteInput,
        ScrollableSinglePageBody,
        StateStreamBuilder,
        SubheadingText,
        ValueStreamBuilder;
import '../currency_format.dart' as currency_format;
import '../date_format.dart' as date_format;
import '../logger.dart';
import '../result.dart';
import '../stream_ext.dart';
import '../style.dart' show Fonts, LxColors, LxRadius, Space;

/// A page for displaying a single payment, in detail.
///
/// Ex: tapping a payment in the wallet page payments list will open this page
/// for that payment.
class PaymentDetailPage extends StatefulWidget {
  const PaymentDetailPage({
    super.key,
    required this.app,
    required this.paymentVecIdx,
    required this.paymentsUpdated,
    required this.fiatRate,
    required this.isRefreshing,
    required this.triggerRefresh,
  });

  final AppHandle app;

  /// The index of this payment in the [app_rs::payments::PaymentDb].
  final int paymentVecIdx;

  /// We receive a notification on this [Stream]
  final Stream<Null> paymentsUpdated;

  /// A stream of [FiatRate] (user's preferred fiat + its exchange rate). May
  /// be null if we're still fetching the rates at startup.
  final ValueStream<FiatRate?> fiatRate;

  /// True when we are currently refreshing (includes syncing payments from our
  /// node).
  final ValueListenable<bool> isRefreshing;

  /// Call this function to (maybe) start a new refresh. Will do nothing if
  /// we're currently refreshing.
  final VoidCallback triggerRefresh;

  Payment getPayment() {
    final vecIdx = this.paymentVecIdx;
    final payment = this.app.getPaymentByVecIdx(vecIdx: vecIdx);

    if (payment == null) {
      throw StateError(
          "PaymentDb is in an invalid state: missing payment @ vec_idx: $vecIdx");
    }

    return payment;
  }

  @override
  State<PaymentDetailPage> createState() => _PaymentDetailPageState();
}

class _PaymentDetailPageState extends State<PaymentDetailPage> {
  // When this stream ticks, all the payments' createdAt label should update.
  // This stream ticks every 30 seconds.
  final StateSubject<DateTime> paymentDateUpdates =
      StateSubject(DateTime.now());
  Timer? paymentDateUpdatesTimer;

  late Payment payment = this.widget.getPayment();

  @override
  void dispose() {
    this.paymentDateUpdatesTimer?.cancel();
    this.paymentDateUpdates.close();

    super.dispose();
  }

  @override
  void initState() {
    super.initState();

    // Update the relative dates on a timer.
    this.paymentDateUpdatesTimer =
        Timer.periodic(const Duration(seconds: 30), (timer) {
      this.paymentDateUpdates.addIfNotClosed(DateTime.now());
    });

    // After we sync some new payments, fetch the payment from the local db.
    this.widget.paymentsUpdated.listen((_) {
      if (!this.mounted) return;

      final newPayment = this.widget.getPayment();

      if (this.payment != newPayment) {
        info("PaymentDetailPage: payment updated");
        this.setState(() {
          this.payment = newPayment;
        });
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return PaymentDetailPageInner(
      app: this.widget.app,
      payment: this.payment,
      paymentDateUpdates: this.paymentDateUpdates,
      fiatRate: this.widget.fiatRate,
      isRefreshing: this.widget.isRefreshing,
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
    required this.isRefreshing,
  });

  final AppHandle app;
  final Payment payment;
  final StateStream<DateTime> paymentDateUpdates;
  final ValueStream<FiatRate?> fiatRate;
  final ValueListenable<bool> isRefreshing;
  final VoidCallback triggerRefresh;

  // HACK: parsing the serialized form like this is ugly af.
  String paymentIdxBody() {
    final paymentIdx = this.payment.index;
    final splitIdx = paymentIdx.lastIndexOf('_');
    if (splitIdx < 0) {
      return paymentIdx;
    } else {
      return paymentIdx.substring(splitIdx + 1);
    }
  }

  @override
  Widget build(BuildContext context) {
    final kind = this.payment.kind;
    final status = this.payment.status;
    final direction = this.payment.direction;
    final directionLabel =
        (direction == PaymentDirection.Inbound) ? "received" : "sent";

    final invoice = this.payment.invoice;
    final payeePubkey = invoice?.payeePubkey;

    final amountSat = this.payment.amountSat;
    final feesSat = this.payment.feesSat;
    final invoiceAmountSat = invoice?.amountSats;

    final createdAt = DateTime.fromMillisecondsSinceEpoch(
      this.payment.createdAt,
      isUtc: true,
    );
    final expiresAt = (invoice != null && status != PaymentStatus.Completed)
        ? DateTime.fromMillisecondsSinceEpoch(invoice.expiresAt, isUtc: true)
        : null;
    final maybeFinalizedAt = this.payment.finalizedAt;
    final finalizedAt = (maybeFinalizedAt != null)
        ? DateTime.fromMillisecondsSinceEpoch(maybeFinalizedAt, isUtc: true)
        : null;

    final maybeAmountSat = this.payment.amountSat;

    // Label should be kept in sync with "common::ln::payments::LxPaymentId"
    final paymentIdxLabel = switch ((kind, direction)) {
      (PaymentKind.Invoice, _) => "Payment hash",
      (PaymentKind.Spontaneous, _) => "Payment hash",
      (PaymentKind.Onchain, PaymentDirection.Inbound) => "Txid",
      (PaymentKind.Onchain, PaymentDirection.Outbound) => "Client payment id",
    };
    final paymentIdxBody = this.paymentIdxBody();

    const pagePaddingInsets = EdgeInsets.symmetric(horizontal: pagePadding);

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(),
        actions: [
          LxRefreshButton(
            isRefreshing: this.isRefreshing,
            triggerRefresh: this.triggerRefresh,
          ),
          const SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(padding: pagePaddingInsets, body: [
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
        StateStreamBuilder(
          stream: this.paymentDateUpdates,
          builder: (_, now) => PaymentDetailDirectionTime(
            status: status,
            direction: direction,
            createdAt: createdAt,
            now: now,
          ),
        ),
        const SizedBox(height: Space.s400),

        // TODO(phlip9): LN invoice "expires in X min" goes here?
        // If pending or failed, show a card with more info on the current
        // status.
        if (status != PaymentStatus.Completed)
          Padding(
            // padding: const EdgeInsets.only(top: Space.s200, bottom: Space.s200),
            padding: const EdgeInsets.symmetric(
                vertical: Space.s200, horizontal: Space.s600),
            child: PaymentDetailStatusCard(
              status: status,
              statusStr: this.payment.statusStr,
            ),
          ),

        const SizedBox(height: Space.s700),

        // Amount sent/received in BTC and fiat.
        if (maybeAmountSat != null)
          ValueStreamBuilder(
            stream: this.fiatRate,
            builder: (_context, fiatRate) => PaymentDetailPrimaryAmount(
              status: status,
              direction: direction,
              amountSat: maybeAmountSat,
              fiatRate: fiatRate,
            ),
          ),
        const SizedBox(height: Space.s400),

        // The payment's note field
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: bodyPadding),
          child: PaymentDetailNoteInput(
            app: this.app,
            paymentIndex: this.payment.index,
            initialNote: this.payment.note,
          ),
        ),
        const SizedBox(height: Space.s1000),
        //
        // // Payment date info
        // PaymentDetailInfoCard(header: "Payment details", children: [
        //   PaymentDetailInfoRow(
        //     label: "Created at",
        //     value: date_format.formatDateFull(createdAt),
        //   ),
        //   if (expiresAt != null)
        //     PaymentDetailInfoRow(
        //       label: "Expires at",
        //       value: date_format.formatDateFull(expiresAt),
        //     ),
        //   if (finalizedAt != null)
        //     PaymentDetailInfoRow(
        //       label: "Finalized at",
        //       value: date_format.formatDateFull(finalizedAt),
        //     ),
        // ]),
        //
        // // Full payment amount + fees info
        // // TODO(phlip9): deemphasize fiat amount below
        // ValueStreamBuilder(
        //   stream: this.fiatRate,
        //   builder: (_context, fiatRate) => PaymentDetailInfoCard(children: [
        //     if (amountSat != null)
        //       PaymentDetailInfoRow(
        //         label: "Amount $directionLabel",
        //         value: formatSatsAmountFiatBelow(amountSat, fiatRate),
        //       ),
        //
        //     if (invoiceAmountSat != null)
        //       PaymentDetailInfoRow(
        //         label: "Invoiced amount",
        //         value: formatSatsAmountFiatBelow(invoiceAmountSat, fiatRate),
        //       ),
        //
        //     // TODO(phlip9): breakdown fees
        //     PaymentDetailInfoRow(
        //       label: "Fees",
        //       value: formatSatsAmountFiatBelow(feesSat, fiatRate),
        //     ),
        //   ]),
        // ),
        //
        // // Low-level stuff
        // PaymentDetailInfoCard(children: [
        //   // oneof: BTC txid, LN payment hash, Lx ClientPaymentId
        //   PaymentDetailInfoRow(label: paymentIdxLabel, value: paymentIdxBody),
        //
        //   if (payeePubkey != null)
        //     PaymentDetailInfoRow(label: "Payee public key", value: payeePubkey),
        //
        //   // the full invoice
        //   if (invoice != null)
        //     PaymentDetailInfoRow(label: "Invoice", value: invoice.string),
        // ]),

        const SizedBox(height: Space.s400),
      ]),
      bottomSheet: PaymentDetailBottomSheet(
        payment: this.payment,
        fiatRate: this.fiatRate,
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

class PaymentDetailBottomSheet extends StatefulWidget {
  const PaymentDetailBottomSheet(
      {super.key, required this.payment, required this.fiatRate});

  final Payment payment;
  final ValueStream<FiatRate?> fiatRate;

  @override
  State<PaymentDetailBottomSheet> createState() =>
      _PaymentDetailBottomSheetState();
}

class _PaymentDetailBottomSheetState extends State<PaymentDetailBottomSheet>
    with TickerProviderStateMixin {
  final sheetKey = GlobalKey();
  final controller = DraggableScrollableController();

  late final Payment payment = this.widget.payment;

  bool didInitialScroll = false;

  // AnimationController? controller;

  @override
  void initState() {
    super.initState();

    unawaited(this.controller.animateTo(0.2,
        duration: const Duration(milliseconds: 150), curve: Curves.bounceIn));
  }

  @override
  void dispose() {
    controller.dispose();
    super.dispose();
  }

  // HACK: parsing the serialized form like this is ugly af.
  String paymentIdxBody() {
    final paymentIdx = this.payment.index;
    final splitIdx = paymentIdx.lastIndexOf('_');
    if (splitIdx < 0) {
      return paymentIdx;
    } else {
      return paymentIdx.substring(splitIdx + 1);
    }
  }

  @override
  Widget build(BuildContext context) {
    final kind = this.payment.kind;
    final status = this.payment.status;
    final direction = this.payment.direction;
    final directionLabel =
        (direction == PaymentDirection.Inbound) ? "received" : "sent";

    final invoice = this.payment.invoice;
    final payeePubkey = invoice?.payeePubkey;

    final amountSat = this.payment.amountSat;
    final feesSat = this.payment.feesSat;
    final invoiceAmountSat = invoice?.amountSats;

    final createdAt = DateTime.fromMillisecondsSinceEpoch(
      this.payment.createdAt,
      isUtc: true,
    );
    final expiresAt = (invoice != null && status != PaymentStatus.Completed)
        ? DateTime.fromMillisecondsSinceEpoch(invoice.expiresAt, isUtc: true)
        : null;
    final maybeFinalizedAt = this.payment.finalizedAt;
    final finalizedAt = (maybeFinalizedAt != null)
        ? DateTime.fromMillisecondsSinceEpoch(maybeFinalizedAt, isUtc: true)
        : null;

    // Label should be kept in sync with "common::ln::payments::LxPaymentId"
    final paymentIdxLabel = switch ((kind, direction)) {
      (PaymentKind.Invoice, _) => "Payment hash",
      (PaymentKind.Spontaneous, _) => "Payment hash",
      (PaymentKind.Onchain, PaymentDirection.Inbound) => "Txid",
      (PaymentKind.Onchain, PaymentDirection.Outbound) => "Client payment id",
    };
    final paymentIdxBody = this.paymentIdxBody();

    if (!this.didInitialScroll) {
      this.didInitialScroll = true;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!this.mounted) return;
        unawaited(this.controller.animateTo(
              0.15,
              duration: const Duration(milliseconds: 500),
              curve: Curves.easeOutQuint,
            ));
      });
    }

    return DraggableScrollableSheet(
      key: this.sheetKey,
      initialChildSize: 0.12,
      minChildSize: 0.12,
      maxChildSize: 0.6,
      expand: false,
      snap: true,
      // snapSizes: const [0.5],
      controller: this.controller,
      shouldCloseOnMinExtent: false,

      builder: (context, scrollController) => Container(
        decoration: BoxDecoration(
          color: LxColors.grey1000,
          borderRadius: BorderRadius.circular(LxRadius.r400),
        ),
        padding: const EdgeInsets.only(
            left: pagePadding, right: pagePadding, top: Space.s100),
        clipBehavior: Clip.antiAlias,
        child: CustomScrollView(
          primary: false,
          controller: scrollController,
          physics: const BouncingScrollPhysics(
              parent: AlwaysScrollableScrollPhysics()),
          slivers: [
            SliverList.list(children: [
              Center(
                child: Container(
                  margin: const EdgeInsets.only(top: Space.s200),
                  width: Space.s800,
                  height: 4,
                  alignment: Alignment.center,
                  decoration: BoxDecoration(
                    color: LxColors.grey800,
                    borderRadius: BorderRadius.circular(2),
                  ),
                ),
              ),

              const Center(child: HeadingText(text: "Payment details")),
              const SizedBox(height: Space.s400),

              // Payment date info
              PaymentDetailInfoCard(children: [
                PaymentDetailInfoRow(
                  label: "Created at",
                  value: date_format.formatDateFull(createdAt),
                ),
                if (expiresAt != null)
                  PaymentDetailInfoRow(
                    label: "Expires at",
                    value: date_format.formatDateFull(expiresAt),
                  ),
                if (finalizedAt != null)
                  PaymentDetailInfoRow(
                    label: "Finalized at",
                    value: date_format.formatDateFull(finalizedAt),
                  ),
              ]),

              // Full payment amount + fees info
              // TODO(phlip9): deemphasize fiat amount below
              ValueStreamBuilder(
                stream: this.widget.fiatRate,
                builder: (_context, fiatRate) =>
                    PaymentDetailInfoCard(children: [
                  if (amountSat != null)
                    PaymentDetailInfoRow(
                      label: "Amount $directionLabel",
                      value: formatSatsAmountFiatBelow(amountSat, fiatRate),
                    ),

                  if (invoiceAmountSat != null)
                    PaymentDetailInfoRow(
                      label: "Invoiced amount",
                      value:
                          formatSatsAmountFiatBelow(invoiceAmountSat, fiatRate),
                    ),

                  // TODO(phlip9): breakdown fees
                  PaymentDetailInfoRow(
                    label: "Fees",
                    value: formatSatsAmountFiatBelow(feesSat, fiatRate),
                  ),
                ]),
              ),

              // Low-level stuff
              PaymentDetailInfoCard(children: [
                // oneof: BTC txid, LN payment hash, Lx ClientPaymentId
                PaymentDetailInfoRow(
                    label: paymentIdxLabel, value: paymentIdxBody),

                if (payeePubkey != null)
                  PaymentDetailInfoRow(
                      label: "Payee public key", value: payeePubkey),

                // the full invoice
                if (invoice != null)
                  PaymentDetailInfoRow(label: "Invoice", value: invoice.string),
              ]),

              const SizedBox(height: Space.s400)
            ]),
          ],
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
    final iconData = switch (this.kind) {
      PaymentKind.Onchain => Icons.currency_bitcoin_rounded,
      PaymentKind.Invoice || PaymentKind.Spontaneous => Icons.bolt_rounded,
    };

    final icon = DecoratedBox(
      decoration: const BoxDecoration(
        color: LxColors.grey825,
        borderRadius: BorderRadius.all(Radius.circular(Space.s800 / 2)),
      ),
      child: SizedBox.square(
        dimension: Space.s800,
        child: Icon(
          iconData,
          size: Space.s700,
          color: LxColors.fgSecondary,
        ),
      ),
    );

    return switch (this.status) {
      PaymentStatus.Completed => PaymentDetailIconBadge(
          icon: Icons.check_rounded,
          color: LxColors.background,
          backgroundColor: LxColors.moneyGoUp,
          child: icon,
        ),
      PaymentStatus.Pending => PaymentDetailIconBadge(
          icon: Icons.sync_rounded,
          color: LxColors.background,
          // Use "green" also for pending. Assume payments will generally be
          // successful. Don't scare users.
          // TODO(phlip9): use a warning yellow after several hours of pending?
          backgroundColor: LxColors.moneyGoUp,
          child: icon,
        ),
      PaymentStatus.Failed => PaymentDetailIconBadge(
          icon: Icons.close_rounded,
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
      ((PaymentStatus.Pending, PaymentDirection.Inbound)) => "Receiving",
      ((PaymentStatus.Pending, PaymentDirection.Outbound)) => "Sending",
      ((PaymentStatus.Completed, PaymentDirection.Inbound)) => "Received",
      ((PaymentStatus.Completed, PaymentDirection.Outbound)) => "Sent",
      ((PaymentStatus.Failed, PaymentDirection.Inbound)) => "Failed to receive",
      ((PaymentStatus.Failed, PaymentDirection.Outbound)) => "Failed to send",
    };

    final createdAtStr = date_format.formatDate(then: createdAt, now: now);

    return RichText(
      text: TextSpan(
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
      : assert(status != PaymentStatus.Completed);

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
                (this.status == PaymentStatus.Pending) ? "pending" : "failed",
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
      ((PaymentStatus.Failed, _)) => LxColors.fgTertiary,
      ((_, PaymentDirection.Inbound)) => LxColors.moneyGoUp,
      ((_, PaymentDirection.Outbound)) => LxColors.fgSecondary,
    };

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
                  style: Fonts.fontUI.copyWith(
                    letterSpacing: -0.5,
                    fontSize: Fonts.size500,
                    fontVariations: [Fonts.weightNormal],
                    fontFeatures: [Fonts.featSlashedZero],
                    color: LxColors.fgTertiary,
                  ),
                  textAlign: TextAlign.center,
                )
              : const FilledPlaceholder(
                  width: Space.s1000,
                  height: Fonts.size500,
                  forText: true,
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
  final String paymentIndex;
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

class PaymentDetailInfoCard extends StatelessWidget {
  const PaymentDetailInfoCard({super.key, required this.children, this.header});

  final String? header;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final section = Card(
      color: LxColors.grey1000,
      elevation: 0.0,
      margin: const EdgeInsets.all(0),
      child: Padding(
        padding: const EdgeInsets.symmetric(
            horizontal: bodyPadding, vertical: Space.s300 / 2),
        child: Column(
          children: this.children,
        ),
      ),
    );

    const intraCardSpace = Space.s200;

    final header = this.header;
    if (header != null) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: intraCardSpace),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding:
                  const EdgeInsets.only(left: bodyPadding, bottom: Space.s200),
              child: Text(
                header,
                style: const TextStyle(
                  color: LxColors.fgTertiary,
                  fontSize: Fonts.size200,
                ),
              ),
            ),
            section,
          ],
        ),
      );
    } else {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: intraCardSpace),
        child: section,
      );
    }
  }
}

class PaymentDetailInfoRow extends StatelessWidget {
  const PaymentDetailInfoRow(
      {super.key, required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) => Padding(
        padding: const EdgeInsets.symmetric(vertical: Space.s300 / 2),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            ConstrainedBox(
              constraints: const BoxConstraints.tightFor(width: Space.s900),
              child: Text(
                this.label,
                style: const TextStyle(
                  color: LxColors.grey550,
                  fontSize: Fonts.size200,
                  height: 1.3,
                ),
              ),
            ),
            const SizedBox(width: Space.s400),
            Expanded(
              // TODO(phlip9): just copy to clipboard on tap or hold?
              child: SelectableText(
                this.value,
                style: const TextStyle(
                  color: LxColors.fgSecondary,
                  fontSize: Fonts.size200,
                  height: 1.3,
                  fontFeatures: [Fonts.featDisambugation],
                ),
              ),
            ),
          ],
        ),
      );
}
