import 'package:app_rs_dart/ffi/types.dart'
    show
        Invoice,
        Offer,
        Payment,
        PaymentCreatedIndex,
        PaymentDirection,
        PaymentKind_Invoice,
        PaymentStatus;
import 'package:app_rs_dart/ffi/types.ext.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lexeapp/design_mode/mocks.dart' as mocks;

/// A [Payment] with all fields populated, used n the test below to ensure
/// [PaymentExt.copyWith] preserves every field.
const Payment paymentAllFields = Payment(
  index: PaymentCreatedIndex(field0: "0000001687140003000-ln_aaaa"),
  kind: PaymentKind_Invoice(),
  direction: PaymentDirection.inbound,
  invoice: Invoice(
    string: "lnbc...",
    description: "invoice description",
    createdAt: 1687140001000,
    expiresAt: 1687150001000,
    amountSats: 1000,
    payeePubkey: "03aaaa",
  ),
  offerId: "offer-id",
  offer: Offer(string: "lno...", payeePubkey: "03bbbb"),
  preimage: "preimage-hex",
  hash: "hash-hex",
  txid: "txid-hex",
  replacement: "replacement-txid",
  amountSats: 1000,
  feesSats: 5,
  amountMsats: 1000 * 1000,
  feesMsats: 5 * 1000,
  status: PaymentStatus.completed,
  statusStr: "completed",
  description: "top-level description",
  payerName: "satoshi@bitcoin.org",
  message: "thanks for the coffee",
  personalNote: "my own note",
  createdAt: 1687140003000,
  finalizedAt: 1687140004000,
);

void main() {
  // `copyWith()` with no args must return a [Payment] equal to the original;
  // catches forgotten fields in [PaymentExt.copyWith].
  test("Payment.copyWith() preserves all fields", () {
    expect(paymentAllFields.copyWith(), paymentAllFields);

    for (final payment in mocks.defaultDummyPayments) {
      expect(payment.copyWith(), payment);
    }
  });
}
