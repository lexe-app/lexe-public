import 'dart:async' show Timer, unawaited;

import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';
import 'package:lexeapp/bindings_generated_api.dart'
    show
        AppHandle,
        FiatRate,
        Payment,
        PaymentDirection,
        PaymentIndex,
        PaymentKind,
        PaymentStatus,
        UpdatePaymentNote;
import 'package:lexeapp/bindings_generated_api_ext.dart';
import 'package:lexeapp/components.dart'
    show
        FilledPlaceholder,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        LxRefreshButton,
        PaymentNoteInput,
        ScrollableSinglePageBody,
        SheetDragHandle,
        StateStreamBuilder,
        ValueStreamBuilder;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/stream_ext.dart';
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;
import 'package:rxdart_ext/rxdart_ext.dart';

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

  late final ValueNotifier<Payment> payment =
      ValueNotifier(this.widget.getPayment());

  @override
  void dispose() {
    this.payment.dispose();
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
      this.payment.value = this.widget.getPayment();
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
  final ValueListenable<Payment> payment;
  final StateStream<DateTime> paymentDateUpdates;
  final ValueStream<FiatRate?> fiatRate;
  final ValueListenable<bool> isRefreshing;
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
            isRefreshing: this.isRefreshing,
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

          return ScrollableSinglePageBody(
            padding: pagePaddingInsets,
            body: [
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
                    statusStr: payment.statusStr,
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
                  paymentIndex: payment.index,
                  initialNote: payment.note,
                ),
              ),
              const SizedBox(height: Space.s1000),
            ],

            // Payment details button
            // -> opens a modal bottom sheet with the complete payment info
            bottom: Padding(
              padding: const EdgeInsets.symmetric(horizontal: pagePadding),
              child: LxFilledButton(
                onTap: () => this.openBottomSheet(context),
                label: const Text("Payment Details"),
                icon: const Icon(LxIcons.expandUp),
              ),
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
  final ValueStream<FiatRate?> fiatRate;

  String paymentIdxBody() => this.payment.value.index.body();

  @override
  Widget build(BuildContext context) {
    return DraggableScrollableSheet(
      initialChildSize: 0.6,
      maxChildSize: 0.6,
      minChildSize: 0.0,
      expand: false,
      shouldCloseOnMinExtent: true,
      builder: (context, scrollController) => Padding(
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
                final directionLabel = (direction == PaymentDirection.Inbound)
                    ? "received"
                    : "sent";

                final invoice = payment.invoice;
                final payeePubkey = invoice?.payeePubkey;

                final amountSat = payment.amountSat;
                final feesSat = payment.feesSat;
                final invoiceAmountSat = invoice?.amountSats;

                final createdAt = DateTime.fromMillisecondsSinceEpoch(
                  payment.createdAt,
                  isUtc: true,
                );
                final expiresAt =
                    (invoice != null && status != PaymentStatus.Completed)
                        ? DateTime.fromMillisecondsSinceEpoch(invoice.expiresAt,
                            isUtc: true)
                        : null;
                final maybeFinalizedAt = payment.finalizedAt;
                final finalizedAt = (maybeFinalizedAt != null)
                    ? DateTime.fromMillisecondsSinceEpoch(maybeFinalizedAt,
                        isUtc: true)
                    : null;

                // Label should be kept in sync with "common::ln::payments::LxPaymentId"
                final paymentIdxLabel = switch ((kind, direction)) {
                  (PaymentKind.Invoice, _) => "Payment hash",
                  (PaymentKind.Spontaneous, _) => "Payment hash",
                  (PaymentKind.Onchain, PaymentDirection.Inbound) => "Txid",
                  (PaymentKind.Onchain, PaymentDirection.Outbound) =>
                    "Client payment id",
                };
                final paymentIdxBody = this.paymentIdxBody();

                return SliverList.list(children: [
                  const SheetDragHandle(),

                  // Sheet heading and close button
                  const Padding(
                    padding: EdgeInsets.only(
                        left: bodyPadding, top: Space.s200, bottom: Space.s400),
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
                    stream: this.fiatRate,
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
                          value: formatSatsAmountFiatBelow(
                              invoiceAmountSat, fiatRate),
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
                      PaymentDetailInfoRow(
                          label: "Invoice", value: invoice.string),
                  ]),

                  const SizedBox(height: Space.s400)
                ]);
              },
            )
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
    final isLightning = switch (this.kind) {
      PaymentKind.Invoice || PaymentKind.Spontaneous => true,
      PaymentKind.Onchain => false,
    };

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
      PaymentStatus.Completed => PaymentDetailIconBadge(
          icon: LxIcons.completedBadge,
          color: LxColors.background,
          backgroundColor: LxColors.moneyGoUp,
          child: icon,
        ),
      PaymentStatus.Pending => PaymentDetailIconBadge(
          icon: LxIcons.pendingBadge,
          color: LxColors.background,
          // Use "green" also for pending. Assume payments will generally be
          // successful. Don't scare users.
          // TODO(phlip9): use a warning yellow after several hours of pending?
          backgroundColor: LxColors.moneyGoUp,
          child: icon,
        ),
      PaymentStatus.Failed => PaymentDetailIconBadge(
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
