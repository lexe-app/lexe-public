// Unit tests for send state logic, using MockAppHandleConfigurable to avoid FFI.

import 'dart:typed_data' show Uint8List;

import 'package:app_rs_dart/app_rs_dart.dart' as app_rs_dart;
import 'package:app_rs_dart/ffi/api.dart'
    show
        Balance,
        FeeEstimate,
        FiatRate,
        PayInvoiceResponse,
        PayOfferResponse,
        PayOnchainResponse,
        PreflightPayInvoiceResponse,
        PreflightPayOfferResponse,
        PreflightPayOnchainResponse;
import 'package:app_rs_dart/ffi/types.dart'
    show
        ClientPaymentId,
        ConfirmationPriority,
        Invoice,
        Network,
        Offer,
        Onchain,
        PaymentCreatedIndex,
        PaymentMethod;
import 'package:app_rs_dart/frb.dart' show AnyhowException;
import 'package:app_rs_dart/lib.dart' show U8Array32;
import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:flutter_test/flutter_test.dart';
import 'package:lexeapp/route/send/state.dart'
    show
        PreflightedPayment_Invoice,
        PreflightedPayment_Offer,
        PreflightedPayment_Onchain,
        SendFlowResult,
        SendState_NeedAmount,
        SendState_Preflighted;
import 'package:lexeapp/route/uri/state.dart' show NeedUriState;

import 'mocks/mock_app_handle.dart';

// Test constants for invoice and offer strings.
const testInvoice =
    'lnbc1p5qzaehdqqpp5n0j7fcaqx4kvffapmnj6fteeu2ykkl4hkqr7cm9gctuyxnep'
    '5caqcqpcsp5slzxgxrsu3jq8xq7rp2gx3ge0thlt3446jpp8kqs87pve60679ls9qy'
    'ysgqxqrrssnp4q0vzagw8x7r9eyalw35t0u6syql8rtqf9tejep0z6xrwkqrua5adv'
    'rzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wzmk4uqqj5s'
    'qqyqqqqqqqqqqhwqqfq89vuhjlg2tt56sv9pdt8t5cvdgfaaf6nxqtt0av74ragpql'
    '7l2d42euknlw06fcgp8xhe93xe7c802z3hrnysfsjgavmwfts7zdvj2cqka3672';
const testOffer =
    'lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q';

/// Create a deterministic ClientPaymentId for tests.
ClientPaymentId testCid() {
  final cidBytes = List.generate(32, (idx) => idx);
  return ClientPaymentId(id: U8Array32(Uint8List.fromList(cidBytes)));
}

/// Create a test balance.
Balance testBalance({
  int totalSats = 100000,
  int onchainSats = 50000,
  int lightningSats = 50000,
}) => Balance(
  totalSats: totalSats,
  onchainSats: onchainSats,
  lightningSats: lightningSats,
  lightningUsableSats: lightningSats,
  lightningMaxSendableSats: lightningSats,
);

void main() {
  late MockAppHandleConfigurable mockApp;
  late ValueNotifier<FiatRate?> fiatRate;

  setUpAll(() async {
    await app_rs_dart.init();
  });

  setUp(() {
    mockApp = MockAppHandleConfigurable();
    fiatRate = ValueNotifier(null);
  });

  tearDown(() {
    mockApp.reset();
    fiatRate.dispose();
  });

  group('SendState_NeedUri', () {
    test('resolveAndMaybePreflight succeeds with onchain address', () async {
      const address = 'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4';
      const onchain = Onchain(address: address);
      const paymentMethod = PaymentMethod.onchain(onchain);

      final state = NeedUriState(
        app: mockApp,
        configNetwork: Network.mainnet,
        balance: testBalance(),
        cid: testCid(),
        fiatRate: fiatRate,
      );

      final result = await state.enterSendFlow(paymentMethod);

      expect(result.isOk, true);
      final newState = result.ok!;
      expect(newState, isA<SendState_NeedAmount>());
    });

    test(
      'resolveAndMaybePreflight preflights immediately for onchain with amount',
      () async {
        const address = 'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4';
        const onchain = Onchain(address: address, amountSats: 5000);
        const paymentMethod = PaymentMethod.onchain(onchain);

        mockApp.mock(
          preflightPayOnchain,
          (_) async => const PreflightPayOnchainResponse(
            high: FeeEstimate(amountSats: 500),
            normal: FeeEstimate(amountSats: 300),
            background: FeeEstimate(amountSats: 100),
          ),
        );

        final state = NeedUriState(
          app: mockApp,
          configNetwork: Network.mainnet,
          balance: testBalance(),
          cid: testCid(),
          fiatRate: fiatRate,
        );

        final result = await state.enterSendFlow(paymentMethod);

        expect(result.isOk, true);
        expect(result.ok, isA<SendState_Preflighted>());
      },
    );

    test('resolveAndMaybePreflight returns error for invalid URI', () async {
      mockApp.mock(
        resolveBest,
        (_) async => throw AnyhowException('Invalid address format'),
      );

      final state = NeedUriState(
        app: mockApp,
        configNetwork: Network.mainnet,
        balance: testBalance(),
        cid: testCid(),
        fiatRate: fiatRate,
      );

      final result = await state.resolve('invalid');

      expect(result.isErr, true);
      expect(result.err, 'Invalid address format');
    });

    test(
      'resolveAndMaybePreflight handles preflight error gracefully',
      () async {
        const address = 'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4';
        const onchain = Onchain(address: address, amountSats: 5000);
        const paymentMethod = PaymentMethod.onchain(onchain);
        mockApp.mock(
          preflightPayOnchain,
          (_) async => throw AnyhowException('Insufficient balance'),
        );

        final state = NeedUriState(
          app: mockApp,
          configNetwork: Network.mainnet,
          balance: testBalance(),
          cid: testCid(),
          fiatRate: fiatRate,
        );

        final result = await state.enterSendFlow(paymentMethod);

        expect(result.isErr, true);
        expect(result.err, 'Insufficient balance');
      },
    );
  });

  group('SendState_NeedAmount', () {
    test('canPreflightImmediately returns amount for onchain with amount', () {
      const onchain = Onchain(
        address: 'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4',
        amountSats: 1000,
      );
      final state = SendState_NeedAmount(
        app: mockApp,
        configNetwork: Network.mainnet,
        balance: testBalance(),
        cid: testCid(),
        fiatRate: fiatRate,
        paymentMethod: const PaymentMethod.onchain(onchain),
      );

      expect(state.canPreflightImmediately(), 1000);
    });

    test('canPreflightImmediately returns null for onchain without amount', () {
      const onchain = Onchain(
        address: 'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4',
      );
      final state = SendState_NeedAmount(
        app: mockApp,
        configNetwork: Network.mainnet,
        balance: testBalance(),
        cid: testCid(),
        fiatRate: fiatRate,
        paymentMethod: const PaymentMethod.onchain(onchain),
      );

      expect(state.canPreflightImmediately(), null);
    });

    test('canPreflightImmediately returns amount for invoice with amount', () {
      const invoice = Invoice(
        string: testInvoice,
        amountSats: 1000,
        createdAt: 1700000000000,
        expiresAt: 1700003600000,
        payeePubkey: 'abc123',
      );
      final state = SendState_NeedAmount(
        app: mockApp,
        configNetwork: Network.mainnet,
        balance: testBalance(),
        cid: testCid(),
        fiatRate: fiatRate,
        paymentMethod: const PaymentMethod.invoice(invoice),
      );

      expect(state.canPreflightImmediately(), 1000);
    });

    test('preflight succeeds for onchain payment', () async {
      const onchain = Onchain(
        address: 'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4',
      );
      mockApp.mock(
        preflightPayOnchain,
        (_) async => const PreflightPayOnchainResponse(
          high: FeeEstimate(amountSats: 500),
          normal: FeeEstimate(amountSats: 300),
          background: FeeEstimate(amountSats: 100),
        ),
      );

      final state = SendState_NeedAmount(
        app: mockApp,
        configNetwork: Network.mainnet,
        balance: testBalance(),
        cid: testCid(),
        fiatRate: fiatRate,
        paymentMethod: const PaymentMethod.onchain(onchain),
      );

      final result = await state.preflight(10000);

      expect(result.isOk, true);
      expect(result.ok, isA<SendState_Preflighted>());
    });

    test('preflight succeeds for invoice payment', () async {
      const invoice = Invoice(
        string: testInvoice,
        createdAt: 1700000000000,
        expiresAt: 1700003600000,
        payeePubkey: 'abc123',
      );
      mockApp.mock(
        preflightPayInvoice,
        (_) async =>
            const PreflightPayInvoiceResponse(amountSats: 5000, feesSats: 10),
      );

      final state = SendState_NeedAmount(
        app: mockApp,
        configNetwork: Network.mainnet,
        balance: testBalance(),
        cid: testCid(),
        fiatRate: fiatRate,
        paymentMethod: const PaymentMethod.invoice(invoice),
      );

      final result = await state.preflight(5000);

      expect(result.isOk, true);
      expect(result.ok, isA<SendState_Preflighted>());
    });

    test('preflight succeeds for offer payment', () async {
      const offer = Offer(string: testOffer);
      mockApp.mock(
        preflightPayOffer,
        (_) async =>
            const PreflightPayOfferResponse(amountSats: 3000, feesSats: 5),
      );

      final state = SendState_NeedAmount(
        app: mockApp,
        configNetwork: Network.mainnet,
        balance: testBalance(),
        cid: testCid(),
        fiatRate: fiatRate,
        paymentMethod: const PaymentMethod.offer(offer),
      );

      final result = await state.preflight(3000, message: 'payer note');

      expect(result.isOk, true);
      expect(result.ok, isA<SendState_Preflighted>());
      final preflighted =
          result.ok!.preflightedPayment as PreflightedPayment_Offer;
      expect(preflighted.message, 'payer note');
    });

    test('preflight returns error on failure', () async {
      const onchain = Onchain(
        address: 'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4',
      );
      mockApp.mock(
        preflightPayOnchain,
        (_) async => throw AnyhowException('Network error'),
      );

      final state = SendState_NeedAmount(
        app: mockApp,
        configNetwork: Network.mainnet,
        balance: testBalance(),
        cid: testCid(),
        fiatRate: fiatRate,
        paymentMethod: const PaymentMethod.onchain(onchain),
      );

      final result = await state.preflight(10000);

      expect(result.isErr, true);
      expect(result.err?.message, 'Network error');
    });
  });

  group('SendState_Preflighted', () {
    test('pay succeeds for onchain', () async {
      mockApp.mock(
        payOnchain,
        (_) async => PayOnchainResponse(
          index: PaymentCreatedIndex(field0: 'test-index'),
          txid: 'abc123',
        ),
      );

      final state = _createPreflightedOnchain(mockApp, fiatRate);

      final result = await state.pay('Test note', ConfirmationPriority.normal);

      expect(result.isOk, true);
      expect(result.ok, isA<SendFlowResult>());
    });

    test('pay succeeds for invoice', () async {
      mockApp.mock(
        payInvoice,
        (_) async => PayInvoiceResponse(
          index: PaymentCreatedIndex(field0: 'test-index'),
        ),
      );

      final state = _createPreflightedInvoice(mockApp, fiatRate);

      final result = await state.pay('Invoice payment', null);

      expect(result.isOk, true);
      expect(result.ok, isA<SendFlowResult>());
    });

    test('pay succeeds for offer', () async {
      mockApp.mock(
        payOffer,
        (_) async =>
            PayOfferResponse(index: PaymentCreatedIndex(field0: 'test-index')),
      );

      final state = _createPreflightedOffer(
        mockApp,
        fiatRate,
        message: 'payer note',
      );

      final result = await state.pay('Offer payment', null);

      expect(result.isOk, true);
      expect(result.ok, isA<SendFlowResult>());
    });

    test('pay returns error on failure', () async {
      mockApp.mock(
        payOnchain,
        (_) async => throw AnyhowException('Transaction failed'),
      );

      final state = _createPreflightedOnchain(mockApp, fiatRate);

      final result = await state.pay('Test note', ConfirmationPriority.normal);

      expect(result.isErr, true);
      expect(result.err?.message, 'Transaction failed');
    });
  });

  group('State transitions', () {
    test(
      'full flow: NeedUri -> NeedAmount -> Preflighted -> SendFlowResult',
      () async {
        const address = 'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4';
        const onchain = Onchain(address: address);
        const paymentMethod = PaymentMethod.onchain(onchain);
        mockApp.mock(
          preflightPayOnchain,
          (_) async => const PreflightPayOnchainResponse(
            high: FeeEstimate(amountSats: 500),
            normal: FeeEstimate(amountSats: 300),
            background: FeeEstimate(amountSats: 100),
          ),
        );
        mockApp.mock(
          payOnchain,
          (req) async => PayOnchainResponse(
            index: PaymentCreatedIndex(field0: 'test-index'),
            txid: 'abc123',
          ),
        );

        // Step 1: Start with NeedUri
        final needUri = NeedUriState(
          app: mockApp,
          configNetwork: Network.mainnet,
          balance: testBalance(),
          cid: testCid(),
          fiatRate: fiatRate,
        );

        // Step 2: Resolve URI -> NeedAmount
        final resolveResult = await needUri.enterSendFlow(paymentMethod);
        expect(resolveResult.isOk, true);
        final needAmount = resolveResult.ok as SendState_NeedAmount;

        // Step 3: Preflight -> Preflighted
        final preflightResult = await needAmount.preflight(10000);
        expect(preflightResult.isOk, true);
        final preflighted = preflightResult.ok!;

        // Step 4: Pay -> SendFlowResult
        final payResult = await preflighted.pay(
          'Test payment',
          ConfirmationPriority.normal,
        );
        expect(payResult.isOk, true);
        expect(payResult.ok, isA<SendFlowResult>());
      },
    );
  });
}

/// Create a preflighted onchain state for testing pay().
SendState_Preflighted _createPreflightedOnchain(
  MockAppHandleConfigurable mockApp,
  ValueNotifier<FiatRate?> fiatRate,
) {
  return SendState_Preflighted(
    app: mockApp,
    configNetwork: Network.mainnet,
    balance: testBalance(),
    cid: testCid(),
    fiatRate: fiatRate,
    preflightedPayment: const PreflightedPayment_Onchain(
      onchain: Onchain(address: 'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4'),
      amountSats: 10000,
      preflight: PreflightPayOnchainResponse(
        high: FeeEstimate(amountSats: 500),
        normal: FeeEstimate(amountSats: 300),
        background: FeeEstimate(amountSats: 100),
      ),
    ),
  );
}

/// Create a preflighted invoice state for testing pay().
SendState_Preflighted _createPreflightedInvoice(
  MockAppHandleConfigurable mockApp,
  ValueNotifier<FiatRate?> fiatRate,
) {
  return SendState_Preflighted(
    app: mockApp,
    configNetwork: Network.mainnet,
    balance: testBalance(),
    cid: testCid(),
    fiatRate: fiatRate,
    preflightedPayment: const PreflightedPayment_Invoice(
      invoice: Invoice(
        string: testInvoice,
        amountSats: 1000,
        createdAt: 1700000000000,
        expiresAt: 1700003600000,
        payeePubkey: 'abc123',
      ),
      amountSats: 1000,
      preflight: PreflightPayInvoiceResponse(amountSats: 1000, feesSats: 10),
    ),
  );
}

/// Create a preflighted offer state for testing pay().
SendState_Preflighted _createPreflightedOffer(
  MockAppHandleConfigurable mockApp,
  ValueNotifier<FiatRate?> fiatRate, {
  required String? message,
}) {
  return SendState_Preflighted(
    app: mockApp,
    configNetwork: Network.mainnet,
    balance: testBalance(),
    cid: testCid(),
    fiatRate: fiatRate,
    preflightedPayment: PreflightedPayment_Offer(
      offer: Offer(string: testOffer),
      amountSats: 3000,
      preflight: PreflightPayOfferResponse(amountSats: 3000, feesSats: 5),
      message: message,
    ),
  );
}
