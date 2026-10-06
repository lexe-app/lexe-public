import 'package:app_rs_dart/app_rs_dart.dart' as app_rs_dart;
import 'package:app_rs_dart/ffi/api.dart'
    show CancelPaymentRequest, PaymentSyncSummary;
import 'package:app_rs_dart/ffi/types.dart' show PaymentStatus;
import 'package:app_rs_dart/ffi/types.ext.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:lexeapp/design_mode/mocks.dart' as mocks;
import 'package:lexeapp/service/payment_sync.dart' show PaymentSyncService;

class _MockAppHandle extends mocks.MockAppHandle {
  _MockAppHandle()
    : super(
        balance: mocks.balanceDefault,
        payments: [mocks.dummyLnInvoiceInboundPendingToComplete],
        channels: const [],
      );

  @override
  Future<PaymentSyncSummary> syncPayments() async {
    return PaymentSyncSummary(
      latestUpdatedIndex: this.latestUpdatedIndex(),
      numNew: 0,
      numUpdated: 0,
    );
  }

  @override
  Future<void> cancelPayment({required CancelPaymentRequest req}) async {
    final payment = this.payments.single;
    final finalizedAt = payment.updatedAt + 1000;
    this.payments = [
      payment.copyWith(
        status: PaymentStatus.failed,
        statusStr: "canceled",
        updatedAt: finalizedAt,
        finalizedAt: finalizedAt,
      ),
    ];
  }
}

void main() {
  setUpAll(() async {
    await app_rs_dart.init();
  });

  test("mock payment updated indexes round-trip through Rust", () {
    for (final payment in [
      ...mocks.defaultDummyPayments,
      ...mocks.appStoreWalletPayments(),
      mocks.dummyOnchainInboundCompleted02,
      mocks.dummyWaivedChannelFee01,
    ]) {
      final actual = payment.updatedIndex().field0;

      final idStr = payment.index.field0.substring(
        payment.index.field0.indexOf('-') + 1,
      );
      final updatedAtStr = payment.updatedAt.toString().padLeft(19, '0');

      expect(actual, "u$updatedAtStr-$idStr");
    }
  });

  test("notifies after SDK cancellation already synced the cache", () async {
    final app = _MockAppHandle();
    final service = PaymentSyncService(app: app);
    addTearDown(service.dispose);
    int updates = 0;
    service.updated.addListener(() => updates += 1);

    await service.sync();
    expect(updates, 1);
    expect(app.getNumPendingPayments(), 1);

    await app.cancelPayment(
      req: CancelPaymentRequest(index: app.payments.single.index),
    );
    expect(app.getNumPendingPayments(), 0);
    expect(updates, 1);

    await service.sync();
    expect(updates, 2);
    expect(app.payments.single.statusStr, "canceled");

    await service.sync();
    expect(updates, 2);
  });

  test("notifies only when the cursor advances", () async {
    final app = _MockAppHandle();
    final service = PaymentSyncService(app: app);
    addTearDown(service.dispose);
    int updates = 0;
    service.updated.addListener(() => updates += 1);

    final payment = app.payments.single;
    for (final (updatedAt, expectedUpdates) in [
      (null, 0),
      (payment.updatedAt, 1),
      (payment.updatedAt, 1),
      (payment.updatedAt + 1000, 2),
      (payment.updatedAt, 2),
      (null, 2),
      (payment.updatedAt, 3),
    ]) {
      app.payments = updatedAt == null
          ? []
          : [payment.copyWith(updatedAt: updatedAt)];
      await service.sync();
      expect(updates, expectedUpdates);
    }
  });
}
