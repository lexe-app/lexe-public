import 'package:app_rs_dart/ffi/api.dart' show FiatRate;
import 'package:app_rs_dart/ffi/types.dart' show Payment;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lexeapp/design_mode/mocks.dart' as mocks;
import 'package:lexeapp/route/payment_detail.dart' show PaymentDetailPageInner;

Widget buildPaymentDetail(Payment payment) {
  final app = mocks.MockAppHandle(
    balance: mocks.balanceDefault,
    payments: [payment],
    channels: const [],
  );

  return MaterialApp(
    home: PaymentDetailPageInner(
      app: app,
      payment: ValueNotifier(payment),
      paymentDateUpdates: ValueNotifier(
        DateTime.fromMillisecondsSinceEpoch(payment.createdAt, isUtc: true),
      ),
      fiatRate: ValueNotifier(const FiatRate(fiat: "USD", rate: 100000.0)),
      isSyncing: ValueNotifier(false),
      triggerRefresh: () {},
    ),
  );
}

Payment _paymentWithPayerFields(
  Payment base, {
  String? payerName,
  String? payerNote,
}) {
  return Payment(
    index: base.index,
    kind: base.kind,
    direction: base.direction,
    invoice: base.invoice,
    offerId: base.offerId,
    offer: base.offer,
    txid: base.txid,
    replacement: base.replacement,
    amountSat: base.amountSat,
    feesSat: base.feesSat,
    status: base.status,
    statusStr: base.statusStr,
    description: base.description,
    note: base.note,
    payerName: payerName ?? base.payerName,
    payerNote: payerNote ?? base.payerNote,
    createdAt: base.createdAt,
    finalizedAt: base.finalizedAt,
  );
}

void main() {
  testWidgets("outbound offer shows payer note as message to recipient", (
    tester,
  ) async {
    await tester.pumpWidget(
      buildPaymentDetail(mocks.dummyOfferOutboundPayment01),
    );

    expect(find.text("Message to recipient"), findsOneWidget);
    expect(find.text("Payer note"), findsNothing);
    expect(find.text("From"), findsNothing);
    expect(find.text("Thanks for building this project."), findsOneWidget);
  });

  testWidgets("inbound offer keeps payer info labels", (tester) async {
    await tester.pumpWidget(
      buildPaymentDetail(mocks.dummyOfferInboundPayment01),
    );

    expect(find.text("From"), findsOneWidget);
    expect(find.text("Payer note"), findsOneWidget);
    expect(find.text("Message to recipient"), findsNothing);
    expect(find.text("satoshi@bitcoin.org"), findsOneWidget);
    expect(find.textContaining("Thanks for the coffee!"), findsOneWidget);
  });

  testWidgets("outbound invoice shows payer note without offer kind", (
    tester,
  ) async {
    final payment = _paymentWithPayerFields(
      mocks.dummyInvoiceOutboundCompleted01,
      payerNote: "Thanks for the lunch!",
    );

    await tester.pumpWidget(buildPaymentDetail(payment));

    expect(find.text("Message to recipient"), findsOneWidget);
    expect(find.text("Payer note"), findsNothing);
    expect(find.text("Thanks for the lunch!"), findsOneWidget);
  });

  testWidgets("inbound invoice shows payer identity without offer kind", (
    tester,
  ) async {
    final payment = _paymentWithPayerFields(
      mocks.dummyInvoiceInboundCompleted01,
      payerName: "satoshi@bitcoin.org",
      payerNote: "Thanks for the coffee!",
    );

    await tester.pumpWidget(buildPaymentDetail(payment));

    expect(find.text("From"), findsOneWidget);
    expect(find.text("Payer note"), findsOneWidget);
    expect(find.text("Message to recipient"), findsNothing);
    expect(find.text("satoshi@bitcoin.org"), findsOneWidget);
    expect(find.text("Thanks for the coffee!"), findsOneWidget);
  });
}
