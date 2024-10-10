/// Mocks for various app services. These are used when the app is run in design
/// mode.
library;

import 'dart:async';

import 'package:app_rs_dart/ffi/api.dart'
    show
        Balance,
        CreateInvoiceRequest,
        CreateInvoiceResponse,
        FeeEstimate,
        FiatRate,
        FiatRates,
        ListChannelsResponse,
        NodeInfo,
        PayInvoiceRequest,
        PayInvoiceResponse,
        PayOnchainRequest,
        PayOnchainResponse,
        PreflightPayInvoiceRequest,
        PreflightPayInvoiceResponse,
        PreflightPayOnchainRequest,
        PreflightPayOnchainResponse,
        UpdatePaymentNote;
import 'package:app_rs_dart/ffi/app.dart' show App, AppHandle, SettingsDbRs;
import 'package:app_rs_dart/ffi/settings.dart' show Settings, SettingsDb;
import 'package:app_rs_dart/ffi/types.dart'
    show
        Config,
        Invoice,
        LxChannelDetails,
        Payment,
        PaymentDirection,
        PaymentIndex,
        PaymentKind,
        PaymentStatus,
        RootSeed,
        ShortPaymentAndIndex;
import 'package:app_rs_dart/ffi/types.ext.dart' show PaymentExt;
import 'package:collection/collection.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/restore.dart' show RestoreApi;
import 'package:lexeapp/route/signup.dart' show SignupApi;

// TODO(phlip9): unhack
// TODO(phlip9): add a `App::mock` constructor?
class MockApp extends App {
  // This makes a fake `RustOpaque<App>` w/ a null pointer. Super hacky, but frb
  // will at least panic if we accidentally call a native method.
  MockApp();

  @override
  void dispose() {}

  @override
  bool get isDisposed => false;
}

// TODO(phlip9): unhack
class MockAppHandle extends AppHandle {
  MockAppHandle() : super(inner: MockApp());

  // New user has no payments
  // List<Payment> payments = [];

  // Some sample payments
  List<Payment> payments = [
    dummyOnchainInboundCompleted01,
    dummyOnchainOutboundFailed01,
    dummySpontaneousOutboundPending01,
    dummyInvoiceOutboundPending01,
    dummyInvoiceInboundPending01,
    dummyInvoiceInboundPending02,
    dummyInvoiceInboundCompleted01,
    dummyInvoiceInboundFailed01,
    dummyOnchainOutboundCompleted01,
  ].sortedBy((payment) => payment.index.field0);

  // Some sample channels
  List<LxChannelDetails> channels = [
    const LxChannelDetails(
      channelId:
          "2607641588c8a779a6f7e7e2d110b0c67bc1f01b9bb9a89bbe98c144f0f4b04c",
      counterpartyNodeId:
          "03781d57bd783a2767d6cb816edd77178d61a5e2a3faf46c5958b9c249bedce274",
      channelValueSats: 123000,
      isUsable: true,
      ourBalanceSats: 55000,
      outboundCapacitySats: 52000,
      theirBalanceSats: 68000,
      inboundCapacitySats: 65000,
    ),
  ];

  @override
  SettingsDb settingsDb() => MockSettingsDb();

  @override
  Future<NodeInfo> nodeInfo({dynamic hint}) =>
      Future.delayed(const Duration(milliseconds: 1000), () {
        const lightningSats = 9836390;
        const onchainSats = 3493734;
        // const lightningSats = 0;
        // const onchainSats = 0;
        const totalSats = lightningSats + onchainSats;
        return const NodeInfo(
          nodePk:
              "03fedbc6adf1a7175389d26b2896d10ef00fa71c81ba085a7c8cd34b6a4e0f7556",
          version: "1.2.3",
          measurement:
              "1d97c2c837b09ec7b0e0b26cb6fa9a211be84c8fdb53299cc9ee8884c7a25ac1",
          balance: Balance(
            totalSats: totalSats,
            lightningSats: lightningSats,
            onchainSats: onchainSats,
          ),
        );
      });

  @override
  Future<ListChannelsResponse> listChannels({dynamic hint}) => Future.delayed(
      const Duration(milliseconds: 1000),
      () => ListChannelsResponse(channels: this.channels));

  @override
  Future<FiatRates> fiatRates({dynamic hint}) => Future.delayed(
        const Duration(milliseconds: 2000),
        () => const FiatRates(
          timestampMs: 1679863795,
          rates: [
            FiatRate(fiat: "USD", rate: 73111.19 /* USD/BTC */),
            FiatRate(
              fiat: "EUR",
              rate: 73111.19 /* USD/BTC */ * 1.10 /* EUR/USD */,
            ),
          ],
        ),
      );

  @override
  Future<PayOnchainResponse> payOnchain({
    required PayOnchainRequest req,
    dynamic hint,
  }) =>
      Future.delayed(
        const Duration(milliseconds: 1200),
        () => const PayOnchainResponse(
          index: PaymentIndex(
              field0:
                  "0000001687385080000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68"),
          txid:
              "f5f119aca79fa3ff1c95793c87ecf7bcd84fa326dfedde3d3c2181a6c733e689",
        ),
      );

  @override
  Future<PreflightPayOnchainResponse> preflightPayOnchain(
          {required PreflightPayOnchainRequest req, dynamic hint}) =>
      Future.delayed(
        const Duration(seconds: 1),
        () => const PreflightPayOnchainResponse(
          high: FeeEstimate(amountSats: 849),
          normal: FeeEstimate(amountSats: 722),
          background: FeeEstimate(amountSats: 563),
        ),
        // () => throw FfiError("Request timed out").toFfi(),
      );

  @override
  Future<String> getAddress({dynamic hint}) => Future.delayed(
        const Duration(milliseconds: 1200),
        () => "bcrt1q2nfxmhd4n3c8834pj72xagvyr9gl57n5r94fsl",
      );

  @override
  Future<CreateInvoiceResponse> createInvoice(
      {required CreateInvoiceRequest req, dynamic hint}) {
    final now = DateTime.now();
    final createdAt = now.millisecondsSinceEpoch;
    final expiresAt =
        now.add(Duration(seconds: req.expirySecs)).millisecondsSinceEpoch;

    final dummy = dummyInvoiceInboundPending01.invoice!;

    return Future.delayed(
      const Duration(milliseconds: 1000),
      () => CreateInvoiceResponse(
        invoice: Invoice(
          string: dummy.string,
          createdAt: createdAt,
          expiresAt: expiresAt,
          amountSats: req.amountSats,
          description: req.description,
          payeePubkey: dummy.payeePubkey,
        ),
      ),
    );
  }

  @override
  Future<PayInvoiceResponse> payInvoice({
    required PayInvoiceRequest req,
    dynamic hint,
  }) =>
      Future.delayed(
        const Duration(milliseconds: 1200),
        () => const PayInvoiceResponse(
          index: PaymentIndex(
              field0:
                  "0000001686744442000-ln_6973b3c58738403ceb3fccec470365a44361f34f4c2664ccae04f0f39fe71dc0"),
        ),
      );

  @override
  Future<PreflightPayInvoiceResponse> preflightPayInvoice(
          {required PreflightPayInvoiceRequest req, dynamic hint}) =>
      Future.delayed(
        const Duration(seconds: 1),
        // () => throw FfiError("Request timed out").toFfi(),
        () => const PreflightPayInvoiceResponse(
          amountSats: 9999,
          feesSats: 123,
        ),
      );

  @override
  Future<bool> syncPayments({dynamic hint}) =>
      Future.delayed(const Duration(milliseconds: 1500), () => true);

  @override
  Future<int?> getVecIdxByPaymentIndex(
      {required PaymentIndex paymentIndex, dynamic hint}) async {
    final vecIdx =
        this.payments.indexWhere((payment) => payment.index == paymentIndex);
    if (vecIdx >= 0) {
      return vecIdx;
    } else {
      return null;
    }
  }

  @override
  Payment? getPaymentByVecIdx({required int vecIdx, dynamic hint}) =>
      this.payments[vecIdx];

  ShortPaymentAndIndex? _getByScrollIdx({
    required bool Function(Payment) filter,
    required int scrollIdx,
  }) {
    final result = this
        .payments
        .reversed // can't `reversed` after .indexed...
        .indexed
        .where((x) => filter(x.$2))
        .elementAtOrNull(scrollIdx);
    if (result == null) return null;
    return ShortPaymentAndIndex(
      vecIdx: this.payments.length - result.$1 - 1,
      payment: result.$2.intoShort(),
    );
  }

  @override
  ShortPaymentAndIndex? getShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      this._getByScrollIdx(filter: (_) => true, scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getPendingShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isPending(), scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getPendingNotJunkShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isPendingNotJunk(),
          scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getFinalizedShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isFinalized(), scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getFinalizedNotJunkShortPaymentByScrollIdx(
          {required int scrollIdx, dynamic hint}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isFinalizedNotJunk(),
          scrollIdx: scrollIdx);

  @override
  int getNumPayments({dynamic hint}) => this.payments.length;

  @override
  int getNumPendingPayments({dynamic hint}) =>
      this.payments.where((payment) => payment.isPending()).length;

  @override
  int getNumPendingNotJunkPayments({dynamic hint}) =>
      this.payments.where((payment) => payment.isPendingNotJunk()).length;

  @override
  int getNumFinalizedPayments({dynamic hint}) =>
      this.payments.where((payment) => payment.isFinalized()).length;

  @override
  int getNumFinalizedNotJunkPayments({dynamic hint}) =>
      this.payments.where((payment) => payment.isFinalizedNotJunk()).length;

  @override
  Future<void> updatePaymentNote(
          {required UpdatePaymentNote req, dynamic hint}) =>
      Future.delayed(const Duration(milliseconds: 1000), () => ());
}

/// An [AppHandle] that usually errors first.
class MockAppHandleErroring extends MockAppHandle {
  MockAppHandleErroring();

  @override
  Future<CreateInvoiceResponse> createInvoice(
      {required CreateInvoiceRequest req, dynamic hint}) {
    return Future.delayed(
      const Duration(milliseconds: 1000),
      () => throw const FfiError(
              "[106=Command] Error while executing command: Failed to register new payment")
          .toFfi(),
    );
  }
}

class MockSettingsDb extends SettingsDb {
  MockSettingsDb() : super(inner: MockSettingsDbRs());

  @override
  Settings read() => const Settings();

  @override
  void reset() {}

  @override
  void update({required Settings update}) {}
}

// A fake `RustOpaque<SettingsDbRs>`
class MockSettingsDbRs extends SettingsDbRs {
  MockSettingsDbRs();

  @override
  void dispose() {}

  @override
  bool get isDisposed => false;
}

class MockSignupApi implements SignupApi {
  const MockSignupApi({required this.app});

  final AppHandle app;

  @override
  Future<FfiResult<AppHandle>> signup({
    required Config config,
    required String googleAuthCode,
    required String password,
  }) =>
      Future.delayed(
        const Duration(milliseconds: 2000),
        () => Ok(this.app),
        // () => const Err(FfiError("[Connect=10] Could not connect")),
      );
}

class MockRestoreApi implements RestoreApi {
  const MockRestoreApi({required this.app});

  final AppHandle app;

  @override
  Future<FfiResult<AppHandle>> restore({
    required Config config,
    required String googleAuthCode,
    required RootSeed rootSeed,
  }) =>
      Future.delayed(
        const Duration(milliseconds: 2000),
        () => Ok(this.app),
      );
}

// Dummy payments data

const Payment dummyOnchainInboundPending01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001687309696000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68"),
  kind: PaymentKind.onchain,
  direction: PaymentDirection.inbound,
  amountSat: 1469,
  feesSat: 0,
  status: PaymentStatus.pending,
  statusStr: "partially confirmed (1-5 confirmations)",
  note: null,
  createdAt: 1687309696000,
  finalizedAt: null,
  replacement: null,
);

const Payment dummyOnchainInboundCompleted01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001670090492000-bc_551df4ef3b67b3f2ca53f3e668eb73c2a9b3a77dea84b340fd2407ec5542aa66"),
  kind: PaymentKind.onchain,
  direction: PaymentDirection.inbound,
  amountSat: 20000,
  feesSat: 0,
  status: PaymentStatus.completed,
  statusStr: "fully confirmed (6+ confirmations)",
  note: "Brunch w/ friends",
  createdAt: 1670090492000,
  finalizedAt: 1670090502000,
  replacement: null,
);

const Payment dummyOnchainOutboundCompleted01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001687385080000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68"),
  kind: PaymentKind.onchain,
  direction: PaymentDirection.outbound,
  amountSat: 77000,
  feesSat: 2881,
  status: PaymentStatus.completed,
  statusStr: "fully confirmed (6+ confirmations)",
  note: "Funding exchange",
  createdAt: 1687385080000,
  finalizedAt: 1687385380000,
);

const Payment dummyOnchainOutboundFailed01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001671818392000-bc_46e52089b60b00de067c84ce58d34a75ffd71a106f720855bc099f20da11700c"),
  kind: PaymentKind.onchain,
  direction: PaymentDirection.outbound,
  amountSat: 95000000,
  feesSat: 5433,
  status: PaymentStatus.failed,
  statusStr: "dropped from mempool",
  note: "Sweep from Muun",
  createdAt: 1671818392000,
  finalizedAt: 1671918392000,
  replacement: null,
);

const Payment dummySpontaneousOutboundPending01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001686938392000-ln_6973b3c58738403ceb3fccec470365a44361f34f4c2664ccae04f0f39fe71dc0"),
  kind: PaymentKind.spontaneous,
  direction: PaymentDirection.outbound,
  amountSat: 123000,
  feesSat: 615,
  status: PaymentStatus.pending,
  statusStr: "pending",
  note: "🍑🍑🍑🍆🍆🍆😂😂😂",
  createdAt: 1686938392000,
);

const Payment dummyInvoiceOutboundPending01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001686744442000-ln_6973b3c58738403ceb3fccec470365a44361f34f4c2664ccae04f0f39fe71dc0"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.outbound,
  invoice: Invoice(
    string:
        "lnbcrt4693500n1pjgld4pxq8pjglhd3pp5h038tqal0m3xjwrmht2gcj8u4cgwg9fh6d0ynv2ds8x8xph5sm9ssp5d4jx76ttd4ek76tnv3hkv6tpdfekgenvdfkx76t2wdskg6nxda5s9qrsgqdp4wdhk6efqdehhgefqw35x2grfdemx76trv5sxxun9v96x7u3qwdjhgcqpcnp4qgywe59xssrqj004k24477svqtgynw4am39hz06hk4dlu4l0ssk8w2rpkgvpsusjrwde5qym0t9g42px0dahyh7jz9lvn5umk9gzqxtc8r0rdplu9psdewwqnw6t7uvdqtvn6heqfgxvn9a76kkl760cy4rqpewlfe6",
    description: "wuhhh",
    createdAt: 1686743442000,
    expiresAt: 1686745442000,
    amountSats: 55000,
    payeePubkey:
        "03fedbc6adf1a7175389d26b2896d10ef00fa71c81ba085a7c8cd34b6a4e0f7556",
  ),
  amountSat: 55000,
  feesSat: 150,
  status: PaymentStatus.pending,
  statusStr: "pending",
  note: null,
  createdAt: 1686744442000,
);

const Payment dummyInvoiceInboundPending01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001687140003000-ln_bbe27583bf7ee269387bbad48c48fcae10e41537d35e49b14d81cc7306f486cb"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.inbound,
  invoice: Invoice(
    string:
        "lnbcrt4693500n1pjgld4pxq8pjglhd3pp5h038tqal0m3xjwrmht2gcj8u4cgwg9fh6d0ynv2ds8x8xph5sm9ssp5d4jx76ttd4ek76tnv3hkv6tpdfekgenvdfkx76t2wdskg6nxda5s9qrsgqdp4wdhk6efqdehhgefqw35x2grfdemx76trv5sxxun9v96x7u3qwdjhgcqpcnp4qgywe59xssrqj004k24477svqtgynw4am39hz06hk4dlu4l0ssk8w2rpkgvpsusjrwde5qym0t9g42px0dahyh7jz9lvn5umk9gzqxtc8r0rdplu9psdewwqnw6t7uvdqtvn6heqfgxvn9a76kkl760cy4rqpewlfe6",
    description: "some note the invoice creator set",
    createdAt: 1687140001000,
    expiresAt: 1687150001000,
    amountSats: 469350,
    payeePubkey:
        "772c84ef57fe5bb5573f714bdcbdba49d0020c7a5fabb2f53d090684a6d0ec082ee2f633d8398b2dd0bade4b2fd2fc78ec881b1296e4834b48c0e73c9edbc774",
  ),
  amountSat: 469350,
  feesSat: 2350,
  status: PaymentStatus.pending,
  statusStr: "claiming",
  note:
      "My super long note that really is too long it just keeps going and going",
  createdAt: 1687140003000,
);

// Junk payment
const Payment dummyInvoiceInboundPending02 = Payment(
  index: PaymentIndex(
      field0:
          "0000001714432815000-ln_c6e5e46c59267114f91d64df0e069b0dae176f9a134656820bba1e6164318980"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.inbound,
  invoice: Invoice(
    string:
        "lnbcrt1pnrq2e0xq8pnrqvaepp5cmj7gmzeyec3f7gavn0sup5mpkhpwmu6zdr9dqsthg0xzep33xqqsp5dfhkjumxv3hkj6npwdhkgenfdfshxmmfv3nx5mmfwdskg6nxda5s9qrsgqdqqcqpcnp4qwla7nx7p5e5nau5k2hh2gxf736rhw0naslthr3jmyu5jqk8gjx7v62qr2p6rh6v38kclflj2yk5x90jsshpe77tjzngc4enn2muxwhu54haacvyef60y5xz2xslezykrvfqlj9yfe4d0tdjrdtx44jusr8sqtehvp3",
    description: null,
    createdAt: 1714432815000,
    expiresAt: 1714435001000,
    amountSats: null,
    payeePubkey:
        "e68d44c7024939d9328ebb3eecf3b93b74f4c92075afb294f749330dde4cdfbfe5a75ff4cbb752a40e1c4947255d2a9c0ae88c826b5f47d6d660ce9b7c6ebca1",
  ),
  amountSat: null,
  feesSat: 0,
  status: PaymentStatus.pending,
  statusStr: "claiming",
  note: null,
  createdAt: 1714432815000,
);

const Payment dummyInvoiceInboundCompleted01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001687100002000-ln_801ffce9fbe74fecc7ec6fa72716d7de6167cc5607635062b24797b54f9ba4be"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.inbound,
  invoice: Invoice(
    string:
        "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
    createdAt: 1687100000000,
    expiresAt: 1687110000000,
    amountSats: 223466,
    description: "Direct deposit",
    payeePubkey:
        "28157d6ca3555a0a3275817d0832c535955b28b20a55f9596f6873434feebfd797d4b245397fab8f8f94dcdd32aac475d64893aa042f18b8d725e116082ae909",
  ),
  amountSat: 223466,
  feesSat: 0,
  status: PaymentStatus.completed,
  statusStr: "completed",
  note: null,
  createdAt: 1687100002000,
  finalizedAt: 1687100005000,
);

// Junk payment (failed)
const Payment dummyInvoiceInboundFailed01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001700222815000-ln_034a21eee2bea4288ec9582b10a4abd6bfdca83855b25257279e67dd02f77d43"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.inbound,
  invoice: Invoice(
    string:
        "lnbcrt1pj4w46lxq8pj4whlfpp5qd9zrmhzh6jz3rkftq43pf9t66lae2pc2ke9y4e8nena6qhh04pssp5v9k8xerxdfhkj6n0d9ekg6nxda5hxer2vekxk6npd3skk6nnve5s9qrsgqdqqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jsc5p3dhdl25x88ndth9qzc4ms2wm5xwa9xfw56dapyaj5n84vv7djsgul2gyjdvk9xzu2pjqv59lfssmft95x43gqqqq5g05r93epkpqpq8a02n",
    description: null,
    createdAt: 1700222815000,
    expiresAt: 1700225001000,
    amountSats: null,
    payeePubkey:
        "28157d6ca3555a0a3275817d0832c535955b28b20a55f9596f6873434feebfd797d4b245397fab8f8f94dcdd32aac475d64893aa042f18b8d725e116082ae909",
  ),
  amountSat: null,
  feesSat: 0,
  status: PaymentStatus.failed,
  statusStr: "expired",
  note: null,
  createdAt: 1700222815000,
);
