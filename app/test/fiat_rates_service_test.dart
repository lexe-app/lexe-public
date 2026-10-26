import 'package:app_rs_dart/ffi/api.dart' show FiatRate, FiatRates;
import 'package:flutter_test/flutter_test.dart';
import 'package:lexeapp/app_data.dart' show LxAppData;
import 'package:lexeapp/design_mode/mocks.dart' as mocks;
import 'package:lexeapp/mock_time.dart' show MockTime;
import 'package:lexeapp/service/fiat_rates.dart' show FiatRateService;

class _MockAppHandle extends mocks.MockAppHandle {
  _MockAppHandle()
    : super(
        balance: mocks.balanceDefault,
        payments: const [],
        channels: const [],
      );

  /// The rates the next fetch returns.
  FiatRates rates = _usdRates(100000.0);

  @override
  Future<FiatRates> fiatRates() => Future.value(this.rates);
}

FiatRates _usdRates(double rate) => FiatRates(
  timestampMs: 0,
  rates: [FiatRate(fiat: "USD", rate: rate)],
);

void main() {
  test("FiatRateService refreshes rates periodically", () {
    MockTime().run((time) {
      final app = _MockAppHandle();
      // `MockAppDataDb` prefers USD, so `fiatRate` is the USD rate.
      final svc = FiatRateService.start(
        app: app,
        appData: LxAppData(app.appDataDb()),
      );

      time.advance(Duration.zero);
      expect(svc.fiatRates.value, _usdRates(100000.0));
      expect(svc.fiatRate.value, const FiatRate(fiat: "USD", rate: 100000.0));

      // 15 minutes
      app.rates = _usdRates(110000.0);
      time.advance(const Duration(minutes: 15));
      expect(svc.fiatRates.value, _usdRates(110000.0));
      expect(svc.fiatRate.value, const FiatRate(fiat: "USD", rate: 110000.0));

      // 30 minutes
      app.rates = _usdRates(120000.0);
      time.advance(const Duration(minutes: 15));
      expect(svc.fiatRates.value, _usdRates(120000.0));
      expect(svc.fiatRate.value, const FiatRate(fiat: "USD", rate: 120000.0));

      // 45 minutes
      app.rates = _usdRates(130000.0);
      time.advance(const Duration(minutes: 15));
      expect(svc.fiatRates.value, _usdRates(130000.0));
      expect(svc.fiatRate.value, const FiatRate(fiat: "USD", rate: 130000.0));

      svc.dispose();
    });
  });
}
