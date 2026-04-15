import 'package:flutter_test/flutter_test.dart';
import 'package:lexeapp/design_mode/mocks.dart' as mocks;
import 'package:lexeapp/mock_time.dart' show MockTime;
import 'package:lexeapp/service/provision.dart' show ProvisionService;

class _MockAppHandle extends mocks.MockAppHandle {
  _MockAppHandle()
    : super(
        balance: mocks.balanceDefault,
        payments: const [],
        channels: const [],
      );

  int numProvisions = 0;

  @override
  Future<void> provision() {
    this.numProvisions += 1;
    return Future.delayed(const Duration(milliseconds: 10));
  }
}

void main() {
  test("provision and waitUntilProvisioned", () {
    MockTime().run((time) async {
      final mockApp = _MockAppHandle();
      final provisionService = ProvisionService(app: mockApp);

      void assertProvisioned(int numProvisions) {
        final isProvisioned = numProvisions > 0;
        expect(mockApp.numProvisions, numProvisions);
        expect(provisionService.isProvisioned.value, isProvisioned);
        expect(provisionService.isProvisioning.value, isProvisioned);
      }

      // start provisioning
      assertProvisioned(0);
      final fut1 = provisionService.waitUntilProvisioned();

      // advance time, but not enough for provision to complete
      time.advanceMs(5);
      assertProvisioned(0);

      // the "do provision" method should immediately exit, since someone else
      // is provisioning
      await provisionService.provision();
      assertProvisioned(0);

      // advance time enough for provision to complete
      time.advanceMs(5);
      await fut1;
      assertProvisioned(1);

      // should complete w/o any time passing and should not provision twice
      await provisionService.waitUntilProvisioned();
      await provisionService.provision();
      assertProvisioned(1);
    });
  });
}
