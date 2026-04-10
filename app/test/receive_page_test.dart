import 'package:app_rs_dart/ffi/api.dart'
    show
        CreateInvoiceRequest,
        CreateInvoiceResponse,
        CreateOfferRequest,
        CreateOfferResponse;
import 'package:app_rs_dart/ffi/types.dart' show Invoice, Offer;
import 'package:flutter_test/flutter_test.dart';
import 'package:lexeapp/design_mode/mocks.dart' as mocks;
import 'package:lexeapp/result.dart' show FfiError;
import 'package:lexeapp/route/receive/page.dart' show ReceiveProvisionGate;
import 'package:lexeapp/service/provision.dart' show ProvisionService;

class _ProvisioningMockAppHandle extends mocks.MockAppHandle {
  _ProvisioningMockAppHandle()
    : super(
        balance: mocks.balanceDefault,
        payments: const [],
        channels: const [],
      );

  static const address = "bcrt1q2nfxmhd4n3c8834pj72xagvyr9gl57n5r94fsl";

  bool isProvisioned = false;
  int preProvisionFailures = 0;

  @override
  Future<void> provision() async {
    await Future<void>.delayed(const Duration(milliseconds: 10));
    this.isProvisioned = true;
  }

  @override
  Future<String> getAddress() async {
    if (!this.isProvisioned) {
      this.preProvisionFailures += 1;
      throw const FfiError("App is not provisioned").toFfi();
    }

    return address;
  }

  @override
  Future<CreateInvoiceResponse> createInvoice({
    required CreateInvoiceRequest req,
  }) async {
    if (!this.isProvisioned) {
      this.preProvisionFailures += 1;
      throw const FfiError("App is not provisioned").toFfi();
    }

    final now = DateTime.now();
    return CreateInvoiceResponse(
      invoice: Invoice(
        string: mocks.dummyInvoiceInboundPending01.invoice!.string,
        createdAt: now.millisecondsSinceEpoch,
        expiresAt: now
            .add(Duration(seconds: req.expirySecs))
            .millisecondsSinceEpoch,
        amountSats: req.amountSats,
        description: req.description,
        payeePubkey: mocks.dummyInvoiceInboundPending01.invoice!.payeePubkey,
      ),
    );
  }

  @override
  Future<CreateOfferResponse> createOffer({
    required CreateOfferRequest req,
  }) async {
    if (!this.isProvisioned) {
      this.preProvisionFailures += 1;
      throw const FfiError("App is not provisioned").toFfi();
    }

    return CreateOfferResponse(
      offer: Offer(
        string: mocks.dummyOfferInboundPayment01.offer!.string,
        expiresAt: null,
        amountSats: req.amountSats,
        description: req.description,
      ),
    );
  }
}

void main() {
  test(
    "provision gate waits for provisioning before allowing fetches",
    () async {
      final app = _ProvisioningMockAppHandle();
      final provisionService = ProvisionService(app: app);
      final provisionGate = ReceiveProvisionGate();

      final isProvisioned = await provisionGate.ensureProvisioned(
        provisionService,
      );

      expect(isProvisioned, isTrue);
      expect(app.preProvisionFailures, 0);
      expect(app.isProvisioned, isTrue);
    },
  );
}
