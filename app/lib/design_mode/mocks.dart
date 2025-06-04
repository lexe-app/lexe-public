/// Mocks for various app services. These are used when the app is run in design
/// mode.
library;

import 'dart:async';

import 'package:app_rs_dart/ffi/api.dart'
    show
        Balance,
        CloseChannelRequest,
        CreateClientRequest,
        CreateClientResponse,
        CreateInvoiceRequest,
        CreateInvoiceResponse,
        CreateOfferRequest,
        CreateOfferResponse,
        FeeEstimate,
        FiatRate,
        FiatRates,
        ListChannelsResponse,
        NodeInfo,
        OpenChannelRequest,
        OpenChannelResponse,
        PayInvoiceRequest,
        PayInvoiceResponse,
        PayOfferRequest,
        PayOfferResponse,
        PayOnchainRequest,
        PayOnchainResponse,
        PreflightCloseChannelResponse,
        PreflightOpenChannelRequest,
        PreflightOpenChannelResponse,
        PreflightPayInvoiceRequest,
        PreflightPayInvoiceResponse,
        PreflightPayOfferRequest,
        PreflightPayOfferResponse,
        PreflightPayOnchainRequest,
        PreflightPayOnchainResponse,
        UpdateClientRequest,
        UpdatePaymentNote;
import 'package:app_rs_dart/ffi/app.dart' show App, AppHandle, SettingsDbRs;
import 'package:app_rs_dart/ffi/settings.dart' show Settings, SettingsDb;
import 'package:app_rs_dart/ffi/types.dart'
    show
        AppUserInfo,
        Config,
        Invoice,
        LxChannelDetails,
        Offer,
        Payment,
        PaymentDirection,
        PaymentIndex,
        PaymentKind,
        PaymentStatus,
        RevocableClient,
        RootSeed,
        Scope,
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
  MockAppHandle(
      {required this.balance, required this.payments, required this.channels})
      : assert(payments.isSortedBy((payment) => payment.index.field0)),
        assert(
            balance.totalSats == balance.lightningSats + balance.onchainSats),
        super(inner: MockApp());

  // Wallet balance
  Balance balance;

  // Some sample payments
  List<Payment> payments;

  // Some sample channels
  List<LxChannelDetails> channels;

  @override
  SettingsDb settingsDb() => MockSettingsDb();

  @override
  AppUserInfo userInfo() => const AppUserInfo(
        userPk:
            "52b999003525a3d905f9916eff26cee6625a3976fc25270ce5b3e79aa3c16f45",
        nodePk:
            "024de9a91aaf32588a7b0bb97ba7fad3db22fcfe62a52bc2b2d389c5fa9d946e1b",
        nodePkProof:
            "024de9a91aaf32588a7b0bb97ba7fad3db22fcfe62a52bc2b2d389c5fa9d946e1b46304402206f762d23d206f3af2ffa452a71a11bca3df68838408851ab77931d7eb7fa1ef6022057141408428d6885d00ca6ca50e6d702aeab227c1550135be5fce4af4e726736",
      );

  @override
  Future<NodeInfo> nodeInfo() =>
      Future.delayed(const Duration(milliseconds: 1000), () {
        return NodeInfo(
          nodePk:
              "024de9a91aaf32588a7b0bb97ba7fad3db22fcfe62a52bc2b2d389c5fa9d946e1b",
          version: "1.2.3",
          measurement:
              "1d97c2c837b09ec7b0e0b26cb6fa9a211be84c8fdb53299cc9ee8884c7a25ac1",
          balance: this.balance,
        );
      });

  @override
  Future<ListChannelsResponse> listChannels() => Future.delayed(
      const Duration(milliseconds: 1000),
      () => ListChannelsResponse(channels: this.channels));

  @override
  Future<PreflightOpenChannelResponse> preflightOpenChannel(
          {required PreflightOpenChannelRequest req}) =>
      Future.delayed(const Duration(milliseconds: 1000),
          () => const PreflightOpenChannelResponse(feeEstimateSats: 123));

  @override
  Future<OpenChannelResponse> openChannel({required OpenChannelRequest req}) =>
      Future.delayed(const Duration(milliseconds: 1000),
          () => OpenChannelResponse(channelId: this.channels[1].channelId));

  @override
  Future<void> closeChannel({required CloseChannelRequest req}) =>
      Future.delayed(const Duration(milliseconds: 1000), () {});

  @override
  Future<PreflightCloseChannelResponse> preflightCloseChannel(
          {required CloseChannelRequest req}) =>
      Future.delayed(const Duration(milliseconds: 1000),
          () => const PreflightCloseChannelResponse(feeEstimateSats: 1100));

  @override
  Future<FiatRates> fiatRates() => Future.delayed(
        const Duration(milliseconds: 2000),
        () => const FiatRates(
          timestampMs: 1732136733,
          rates: [
            FiatRate(fiat: "USD", rate: 94385.79 /* USD/BTC */),
            FiatRate(
              fiat: "EUR",
              rate: 94385.79 /* USD/BTC */ * 1.10 /* EUR/USD */,
            ),
          ],
        ),
      );

  @override
  Future<PayOnchainResponse> payOnchain({
    required PayOnchainRequest req,
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
          {required PreflightPayOnchainRequest req}) =>
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
  Future<String> getAddress() => Future.delayed(
        const Duration(milliseconds: 1200),
        () => "bcrt1q2nfxmhd4n3c8834pj72xagvyr9gl57n5r94fsl",
      );

  @override
  Future<CreateInvoiceResponse> createInvoice(
      {required CreateInvoiceRequest req}) {
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
          {required PreflightPayInvoiceRequest req}) =>
      Future.delayed(
        const Duration(seconds: 1),
        // () => throw FfiError("Request timed out").toFfi(),
        () => PreflightPayInvoiceResponse(
          amountSats: req.fallbackAmountSats ?? 9999,
          feesSats: 123,
        ),
      );

  @override
  Future<CreateOfferResponse> createOffer({required CreateOfferRequest req}) {
    final expirySecs = req.expirySecs;
    final expiresAt = (expirySecs != null)
        ? DateTime.now()
            .add(Duration(seconds: expirySecs))
            .millisecondsSinceEpoch
        : null;

    return Future.delayed(
      const Duration(milliseconds: 1000),
      () => CreateOfferResponse(
        offer: Offer(
          string:
              "lno1zrxq8pjw7qjlm68mtp7e3yvxee4y5xrgjhhyf2fxhlphpckrvevh50u0qdp2nyl5lh362fu4r6ycw59tul97ptq57j9mhusk4dyqed0nytnzyqsz0qduahca4eryls267a72a4rtcnk4p6ululyvg7a7pdczg8ha8e6qqval7cremj65ut2k087xdhay6qvv0dtljppyd80zyj68f748jt569nutyznpf9qms39a06ecl0tw9w6ky9xpqd4k7hl4phttq9lkdrhjffv08tc04yxf4pfexypwt0e8zlmdeuf4qqqsdt4qevd84nlmks62nzzz9swwpu",
          expiresAt: expiresAt,
          amountSats: req.amountSats,
          description: req.description,
        ),
      ),
    );
  }

  @override
  Future<PayOfferResponse> payOffer({
    required PayOfferRequest req,
  }) =>
      Future.delayed(
        const Duration(milliseconds: 1200),
        () => PayOfferResponse(index: dummyOfferOutboundPayment01.index),
      );

  @override
  Future<PreflightPayOfferResponse> preflightPayOffer(
          {required PreflightPayOfferRequest req}) =>
      Future.delayed(
        const Duration(seconds: 1),
        // () => throw FfiError("Request timed out").toFfi(),
        () => PreflightPayOfferResponse(
          amountSats: req.fallbackAmountSats ?? 9999,
          feesSats: 123,
        ),
      );

  @override
  Future<bool> syncPayments() =>
      Future.delayed(const Duration(milliseconds: 1500), () => true);

  @override
  Future<int?> getVecIdxByPaymentIndex(
      {required PaymentIndex paymentIndex}) async {
    final vecIdx =
        this.payments.indexWhere((payment) => payment.index == paymentIndex);
    if (vecIdx >= 0) {
      return vecIdx;
    } else {
      return null;
    }
  }

  @override
  Payment? getPaymentByVecIdx({required int vecIdx}) => this.payments[vecIdx];

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
  ShortPaymentAndIndex? getShortPaymentByScrollIdx({required int scrollIdx}) =>
      this._getByScrollIdx(filter: (_) => true, scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getPendingShortPaymentByScrollIdx(
          {required int scrollIdx}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isPending(), scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getPendingNotJunkShortPaymentByScrollIdx(
          {required int scrollIdx}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isPendingNotJunk(),
          scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getFinalizedShortPaymentByScrollIdx(
          {required int scrollIdx}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isFinalized(), scrollIdx: scrollIdx);

  @override
  ShortPaymentAndIndex? getFinalizedNotJunkShortPaymentByScrollIdx(
          {required int scrollIdx}) =>
      this._getByScrollIdx(
          filter: (payment) => payment.isFinalizedNotJunk(),
          scrollIdx: scrollIdx);

  @override
  int getNumPayments() => this.payments.length;

  @override
  int getNumPendingPayments() =>
      this.payments.where((payment) => payment.isPending()).length;

  @override
  int getNumPendingNotJunkPayments() =>
      this.payments.where((payment) => payment.isPendingNotJunk()).length;

  @override
  int getNumFinalizedPayments() =>
      this.payments.where((payment) => payment.isFinalized()).length;

  @override
  int getNumFinalizedNotJunkPayments() =>
      this.payments.where((payment) => payment.isFinalizedNotJunk()).length;

  @override
  Future<void> updatePaymentNote({required UpdatePaymentNote req}) =>
      Future.delayed(const Duration(milliseconds: 1000), () => ());

  @override
  Future<CreateClientResponse> createClient(
          {required CreateClientRequest req}) =>
      Future.delayed(
        const Duration(milliseconds: 1000),
        () => const CreateClientResponse(
          client: RevocableClient(
            pubkey:
                "7088af1fc12ab04ad6dd165bc3a3c5eb3062b411a2f55a166b0e400b390fe4db",
            createdAt: 1747011336000,
            scope: Scope.nodeConnect,
          ),
          credentials:
              "eyJsZXhlX2F1dGhfdG9rZW4iOiI5ZFRDVXZDOHk3cWNOeVVicXluejNud0lRUUhiUXFQVktlTWhYVWoxQWZyLXZnajlFMjE3XzJ0Q1MxSVFNN0xGcWZCVUM4RWM5ZmNiLWRRaUNSeTZvdDJGTi1rUjYwZWRSRkpVenRBYTJSeGFvMVEwQlMxczZ2RThncmdmaE1ZSUFKRExNV2dBQUFBQVNFNHphQUFBQUFCcGFXbHBhV2xwYVdscGFXbHBhV2xwYVdscGFXbHBhV2xwYVdscGFXbHBhUUUiLCJjbGllbnRfcGsiOiI3MDg4YWYxZmMxMmFiMDRhZDZkZDE2NWJjM2EzYzVlYjMwNjJiNDExYTJmNTVhMTY2YjBlNDAwYjM5MGZlNGRiIiwiY2xpZW50X2tleV9kZXIiOiIzMDUzMDIwMTAxMzAwNTA2MDMyYjY1NzAwNDIyMDQyMDBmNTgwZDM0NjFjNGVhMGIzNmI4MzZkNDUxYzFjMTk5ZWUzZTA2NDZhZDBkNjQyMzUzNzk3MzlkNjg2OTkyODlhMTIzMDMyMTAwNzA4OGFmMWZjMTJhYjA0YWQ2ZGQxNjViYzNhM2M1ZWIzMDYyYjQxMWEyZjU1YTE2NmIwZTQwMGIzOTBmZTRkYiIsImNsaWVudF9jZXJ0X2RlciI6IjMwODIwMTgzMzA4MjAxMzVhMDAzMDIwMTAyMDIxNDQwYmVkYzU2ZDAzZDZiNTJmMjg0MmQ2NGRmOTBkMDJkNmRhMzZhNWIzMDA1MDYwMzJiNjU3MDMwNTYzMTBiMzAwOTA2MDM1NTA0MDYwYzAyNTU1MzMxMGIzMDA5MDYwMzU1MDQwODBjMDI0MzQxMzExMTMwMGYwNjAzNTUwNDBhMGMwODZjNjU3ODY1MmQ2MTcwNzAzMTI3MzAyNTA2MDM1NTA0MDMwYzFlNGM2NTc4NjUyMDcyNjU3NjZmNjM2MTYyNmM2NTIwNjk3MzczNzU2OTZlNjcyMDQzNDEyMDYzNjU3Mjc0MzAyMDE3MGQzNzM1MzAzMTMwMzEzMDMwMzAzMDMwMzA1YTE4MGYzNDMwMzkzNjMwMzEzMDMxMzAzMDMwMzAzMDMwNWEzMDUyMzEwYjMwMDkwNjAzNTUwNDA2MGMwMjU1NTMzMTBiMzAwOTA2MDM1NTA0MDgwYzAyNDM0MTMxMTEzMDBmMDYwMzU1MDQwYTBjMDg2YzY1Nzg2NTJkNjE3MDcwMzEyMzMwMjEwNjAzNTUwNDAzMGMxYTRjNjU3ODY1MjA3MjY1NzY2ZjYzNjE2MjZjNjUyMDYzNmM2OTY1NmU3NDIwNjM2NTcyNzQzMDJhMzAwNTA2MDMyYjY1NzAwMzIxMDA3MDg4YWYxZmMxMmFiMDRhZDZkZDE2NWJjM2EzYzVlYjMwNjJiNDExYTJmNTVhMTY2YjBlNDAwYjM5MGZlNGRiYTMxNzMwMTUzMDEzMDYwMzU1MWQxMTA0MGMzMDBhODIwODZjNjU3ODY1MmU2MTcwNzAzMDA1MDYwMzJiNjU3MDAzNDEwMDdiMTdiYzk1MzgyNjdiMzU0ZjA3MjZkODljYjFlYzMxMGIxMDJlNDIyYWI5Njk2Yjg3ZDlhZTcwMGNlZjJlODNjMTM2NmQwYWQxOTAzNWQ5ZTNlZDA0Y2Y1ZjdmMDVkZWY2OGE3MWRlMjEyYjg5ODM0NDc3OTQyYWU3NjNhMjBmIiwiY2FfY2VydF9kZXIiOiIzMDgyMDFiYTMwODIwMTZjYTAwMzAyMDEwMjAyMTQwY2RjYzJkMGVhM2MzMjI4MDc1OWNkNGFiN2E1MzBmNDNiODAwMjA5MzAwNTA2MDMyYjY1NzAzMDU2MzEwYjMwMDkwNjAzNTUwNDA2MGMwMjU1NTMzMTBiMzAwOTA2MDM1NTA0MDgwYzAyNDM0MTMxMTEzMDBmMDYwMzU1MDQwYTBjMDg2YzY1Nzg2NTJkNjE3MDcwMzEyNzMwMjUwNjAzNTUwNDAzMGMxZTRjNjU3ODY1MjA3MjY1NzY2ZjYzNjE2MjZjNjUyMDY5NzM3Mzc1Njk2ZTY3MjA0MzQxMjA2MzY1NzI3NDMwMjAxNzBkMzczNTMwMzEzMDMxMzAzMDMwMzAzMDMwNWExODBmMzQzMDM5MzYzMDMxMzAzMTMwMzAzMDMwMzAzMDVhMzA1NjMxMGIzMDA5MDYwMzU1MDQwNjBjMDI1NTUzMzEwYjMwMDkwNjAzNTUwNDA4MGMwMjQzNDEzMTExMzAwZjA2MDM1NTA0MGEwYzA4NmM2NTc4NjUyZDYxNzA3MDMxMjczMDI1MDYwMzU1MDQwMzBjMWU0YzY1Nzg2NTIwNzI2NTc2NmY2MzYxNjI2YzY1MjA2OTczNzM3NTY5NmU2NzIwNDM0MTIwNjM2NTcyNzQzMDJhMzAwNTA2MDMyYjY1NzAwMzIxMDBjOTM2ZDhlNzhiMDc4ZDkyODQ5YWRjMzYyZGVjYzNlYTMxNzA1ZTQ0ODZiZDAwZDgxYmU2NGFmZDYzZTg4NzU5YTM0YTMwNDgzMDEzMDYwMzU1MWQxMTA0MGMzMDBhODIwODZjNjU3ODY1MmU2MTcwNzAzMDFkMDYwMzU1MWQwZTA0MTYwNDE0MGNkY2MyZDBlYTNjMzIyODA3NTljZDRhYjdhNTMwZjQzYjgwMDIwOTMwMTIwNjAzNTUxZDEzMDEwMWZmMDQwODMwMDYwMTAxZmYwMjAxMDAzMDA1MDYwMzJiNjU3MDAzNDEwMGVhNzJlOTY3MGY5OTFjODdlZDVlMmMxZGY3YzgyZWNlN2VjMjAxMGM1NzY1NGJmYWU4Y2Q1ZmE5NzMzNmYyNTViMzRkN2FjNzQzOTk5NThkYWQwY2U0NDU2ZDcwYjAzOWMyYzIzMmU0YjVhZDljZTNjYzRhMWZhMTljNzA1MTA0In0=",
        ),
      );

  @override
  Future<List<RevocableClient>> listClients() => Future.delayed(
        const Duration(milliseconds: 1000),
        () => <RevocableClient>[
          const RevocableClient(
            createdAt: 1747095626000,
            label: "SDK client 1",
            pubkey:
                "d6f34a82bf64b68a28b2f0934d715271580afebc29a4b37bfd355999705f43ff",
            scope: Scope.nodeConnect,
          ),
          const RevocableClient(
            createdAt: 1746095626000,
            label: null,
            pubkey:
                "90cdb0fa319acd1b5cbf79c027fbadeafbaed593c3b0d81c88e0746fe0dc2016",
            scope: Scope.nodeConnect,
          ),
        ],
      );

  @override
  Future<void> updateClient({required UpdateClientRequest req}) =>
      Future.delayed(
        const Duration(milliseconds: 1000),
        () => {},
      );
}

/// An [AppHandle] that usually errors first.
class MockAppHandleErr extends MockAppHandle {
  MockAppHandleErr({
    required super.balance,
    required super.payments,
    required super.channels,
  });

  @override
  Future<CreateInvoiceResponse> createInvoice(
      {required CreateInvoiceRequest req}) {
    return Future.delayed(
      const Duration(milliseconds: 1000),
      () => throw const FfiError("[106=Command] Failed to register new payment")
          .toFfi(),
    );
  }

  @override
  Future<PayInvoiceResponse> payInvoice({required PayInvoiceRequest req}) =>
      Future.delayed(
          const Duration(milliseconds: 1000),
          () => throw const FfiError(
                  "[106=Command] Already tried to pay this invoice: Error handling new payment: Payment already exists: finalized")
              .toFfi());

  @override
  Future<PreflightPayOnchainResponse> preflightPayOnchain(
          {required PreflightPayOnchainRequest req}) =>
      Future.delayed(
          const Duration(milliseconds: 1000),
          () => throw const FfiError(
                  "[106=Command] Failed to build onchain send tx: Insufficient funds: 433 sat available of 16546 sat needed")
              .toFfi());

  @override
  Future<PreflightOpenChannelResponse> preflightOpenChannel(
          {required PreflightOpenChannelRequest req}) =>
      Future.delayed(
        const Duration(milliseconds: 1000),
        () => throw const FfiError(
                "[106=Command] Channel value is below limit 5000 sats")
            .toFfi(),
      );

  @override
  Future<OpenChannelResponse> openChannel({required OpenChannelRequest req}) =>
      Future.delayed(
        const Duration(milliseconds: 1000),
        () => throw const FfiError(
                "[106=Command] Waiting for channel close event: deadline has elapsed")
            .toFfi(),
      );

  @override
  Future<PreflightCloseChannelResponse> preflightCloseChannel(
          {required CloseChannelRequest req}) =>
      Future.delayed(
          const Duration(milliseconds: 1000),
          () => throw const FfiError("[106=Command] No channel with this id")
              .toFfi());

  @override
  Future<void> closeChannel({required CloseChannelRequest req}) =>
      Future.delayed(
        const Duration(milliseconds: 1000),
        () => throw const FfiError(
                "[106=Command] Waiting for channel close event: deadline has elapsed")
            .toFfi(),
      );

  @override
  Future<CreateClientResponse> createClient(
          {required CreateClientRequest req}) =>
      Future.delayed(
        const Duration(milliseconds: 1000),
        () => throw const FfiError(
                "[106=Command] Failed to create client: label is too long")
            .toFfi(),
      );

  @override
  Future<List<RevocableClient>> listClients() => Future.delayed(
        const Duration(milliseconds: 1000),
        () => throw const FfiError("[106=Command] Failed to list clients")
            .toFfi(),
      );

  @override
  Future<void> updateClient({required UpdateClientRequest req}) =>
      Future.delayed(
        const Duration(milliseconds: 1000),
        () => throw const FfiError("[106=Command] Failed to update client")
            .toFfi(),
      );
}

/// `AppHandle` used for screenshots.
///
/// * Mocked API requests should resolve immediately
/// * TODO(phlip9): easily configure language and localization
class MockAppHandleScreenshots extends MockAppHandle {
  MockAppHandleScreenshots()
      : super(
          payments: [
            dummyOnchainInboundCompleted02,
            dummyInvoiceOutboundCompleted01,
            dummyInvoiceInboundCompleted02,
          ],
          channels: [],
          balance: balanceDefault,
        );

  @override
  Future<bool> syncPayments() => Future.value(false);

  @override
  Future<FiatRates> fiatRates() => Future.value(const FiatRates(
        timestampMs: 1732136733,
        rates: [
          FiatRate(fiat: "USD", rate: 96626.76 /* USD/BTC */),
          FiatRate(
            fiat: "EUR",
            rate: 96626.76 /* USD/BTC */ * 0.9559 /* EUR/USD */,
          ),
        ],
      ));

  @override
  Future<NodeInfo> nodeInfo() => Future.value(const NodeInfo(
        nodePk:
            "024de9a91aaf32588a7b0bb97ba7fad3db22fcfe62a52bc2b2d389c5fa9d946e1b",
        version: "0.6.15",
        measurement:
            "1d97c2c837b09ec7b0e0b26cb6fa9a211be84c8fdb53299cc9ee8884c7a25ac1",
        balance: Balance(
          totalSats: 233671,
          lightningSats: 154226,
          lightningMaxSendableSats: 154226 - 4500,
          onchainSats: 233671 - 154226,
        ),
      ));

  @override
  Future<CreateInvoiceResponse> createInvoice(
      {required CreateInvoiceRequest req}) {
    final now = DateTime.now();
    final createdAt = now.millisecondsSinceEpoch;
    final expiresAt =
        now.add(Duration(seconds: req.expirySecs)).millisecondsSinceEpoch;

    final dummy = dummyInvoiceInboundPending01.invoice!;

    return Future.value(
      CreateInvoiceResponse(
        invoice: Invoice(
          string: dummy.string,
          createdAt: createdAt,
          expiresAt: expiresAt,
          amountSats: 4670,
          description: "pour-over coffee",
          payeePubkey: dummy.payeePubkey,
        ),
      ),
    );
  }

  @override
  Future<String> getAddress() =>
      Future.value("bcrt1q2nfxmhd4n3c8834pj72xagvyr9gl57n5r94fsl");
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
    required String? signupCode,
    required String? partner,
  }) =>
      Future.delayed(
        const Duration(milliseconds: 2000),
        () => Ok(this.app),
      );
}

class MockSignupApiErr implements SignupApi {
  const MockSignupApiErr();

  @override
  Future<FfiResult<AppHandle>> signup({
    required Config config,
    required String googleAuthCode,
    required String password,
    required String? signupCode,
    required String? partner,
  }) =>
      Future.delayed(
        const Duration(milliseconds: 1000),
        () => const Err(FfiError("[Connect=10] Could not connect")),
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

//
// Dummy balance data
//

const Balance balanceDefault = Balance(
  lightningSats: 198466,
  lightningMaxSendableSats: 198466 - 2300,
  onchainSats: 21214,
  totalSats: 198466 + 21214,
);

const Balance balanceZero = Balance(
  totalSats: 0,
  lightningSats: 0,
  lightningMaxSendableSats: 0,
  onchainSats: 0,
);

const Balance balanceOnchainOnly = Balance(
  totalSats: 123000,
  lightningSats: 0,
  lightningMaxSendableSats: 0,
  onchainSats: 123000,
);

//
// Dummy payments data
//

const Payment dummyOnchainInboundPending01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001687309696000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68"),
  kind: PaymentKind.onchain,
  direction: PaymentDirection.inbound,
  txid: "238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68",
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
  txid: "551df4ef3b67b3f2ca53f3e668eb73c2a9b3a77dea84b340fd2407ec5542aa66",
  amountSat: 20000,
  feesSat: 0,
  status: PaymentStatus.completed,
  statusStr: "fully confirmed (6+ confirmations)",
  note: "Brunch w/ friends",
  createdAt: 1670090492000,
  finalizedAt: 1670090502000,
  replacement: null,
);

const Payment dummyOnchainInboundCompleted02 = Payment(
  index: PaymentIndex(
      field0:
          "0000001739386001000-bc_70596383fb7dd5c578a5ef348ec77c5979a65ecb4b10bae0ce60e814c35f04f1"),
  kind: PaymentKind.onchain,
  direction: PaymentDirection.inbound,
  txid: "70596383fb7dd5c578a5ef348ec77c5979a65ecb4b10bae0ce60e814c35f04f1",
  amountSat: 208505,
  feesSat: 0,
  status: PaymentStatus.completed,
  statusStr: "fully confirmed (6+ confirmations)",
  note: "Exchange → Lexe wallet",
  createdAt: 1739386001000,
  finalizedAt: 1739386501000,
  replacement: null,
);

const Payment dummyOnchainOutboundCompleted01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001687385080000-bc_238eb9f1b1db5e39877da642126783e2d6a043e047bbbe8872df3e7fdc3dca68"),
  kind: PaymentKind.onchain,
  direction: PaymentDirection.outbound,
  txid: "0a882813f2bb937a45f18568ff0d09d83d437558e85e369629226d0f7405c52e",
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
  txid: "e9e3db092c9b4f94d1c603dd503a0f9bb95f9369897a25aafef2960053f8ccab",
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

const Payment dummyInvoiceInboundCompleted02 = Payment(
  index: PaymentIndex(
      field0:
          "0000001739490952000-ln_4ca99b7534df3a98afb69757b770faffead8b0794e5d618fbbf9b4cfd1f157cf"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.inbound,
  invoice: Invoice(
    string:
        "lnbcrt154660n1pn6ap5xdqgf36kucmgpp5fj5ekaf5muaf3takjatmwu86ll4d3vrefewkrramlx6vl5032l8ssp50sn9getawgwsuzmlll5rfk0cqydw4hhdgct47k424f7r9s4pya9s9qyysgqcqpcxq8pn6aph2dzwjlq2vjtmducjrdgjpk6pvr23c7a3s4qrh4770a7qj00pph3vpurg0av8ps689pxt8exufuf45vd8mladjsky2rxtdqtwdpmdj38qp7k5cz7",
    createdAt: 1739490950000,
    expiresAt: 1739491050000,
    amountSats: 32466,
    description: "Lunch at Celia's",
    payeePubkey:
        "036d5a2631b3f1c25ef9a004973762b3c1af5fb892ad14b166e9573b93b83088926667d1c431271a8f06adf5510ac79763f0dfbf66904a449fd55aff60639905",
  ),
  amountSat: 32166,
  feesSat: 300,
  status: PaymentStatus.completed,
  statusStr: "completed",
  note: "Lunch at Celia's",
  createdAt: 1739490952000,
  finalizedAt: 1739490955000,
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

const Payment dummyInvoiceOutboundCompleted01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001739487454000-ln_432ec4be62f494b0498c76145fd31b302d0be4ac8cffe7c4102ad1f1c056bec9"),
  kind: PaymentKind.invoice,
  direction: PaymentDirection.outbound,
  invoice: Invoice(
    string:
        "lnbcrt70000n1qqp4zkldqgf36kucmgpp5gvhvf0nz7j2tqjvvwc29l5cmxqkshe9v3nl703qs9tglrszkhmyssp5v8v37rw8qkn34sxlgqh9yfcss8ru73834k4kl0xc9tcn59l2fuas9qyysgqcqpcxq9p4zhfzh9jcl8c5z8f4v3dp30hxrvy4mnjwhnk749fazx6tu28kyz0hgvq98jaallfkq6yscsrfyerv5y2c5z2c65dxuk3xlhnvchj66tlwesqrwkvga",
    createdAt: 1739487454000,
    expiresAt: 1739497454000,
    amountSats: 7000,
    description: "stacker.news",
    payeePubkey:
        "036d5a2631b3f1c25ef9a004973762b3c1af5fb892ad14b166e9573b93b83088926667d1c431271a8f06adf5510ac79763f0dfbf66904a449fd55aff60639905",
  ),
  amountSat: 7000,
  feesSat: 0,
  status: PaymentStatus.completed,
  statusStr: "completed",
  note: "stacker.news",
  createdAt: 1739487454000,
  finalizedAt: 1739487458000,
);

const Payment dummyOfferOutboundPayment01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001748993362000-fs_1a0863b5785d35c34e008e8bd879e78a93b795640c2ad9e941a7d12f44356804"),
  kind: PaymentKind.offer,
  direction: PaymentDirection.outbound,
  offer: Offer(
    string:
        "lno1qgsqvgnwgcg35z6ee2h3yczraddm72xrfua9uve2rlrm9deu7xyfzrc2p4zx7mnpw35k7m3q2pskwegwq35rl86qzr7sz0sztfk2ex9hfmq35agpv450kw90sx3ewxhzmcq5324qrl89gv02s54q862yje5mzjagzvvqs5ptwk9x5txt0rgecmsll7qyy2lurdjpcqerqvp0pvxu088jng3v560f94t4ajw6jltszfgh8flzm33w3gpqa6ajuwcqx0wqwsv40gp7rs2e2ywggmx5kjj4xdeq6ph62u7z7j2p8cvntcgyqxwywv86uyuu59033z6tzgsr8gme5g5q9gahnxul2fg44zen05t7w7mr23jqwr2t4hnvqmgpkzydskfzu66cqqec0uw2q0wmqknc2v6t53rpgkv5v9nu05k2w5k4a3kf942q9jgp0gqrrqwyc58k443qt9gfd3mzfmt452dksqc9d7cdls8v7dwlma2yq9275y6lrk4ctdeh0gwjkrtx9j9ncaxnryqzex9cvtpm8nvckhdhr889m4xhx04f5dqvl3d2mq0aex6ynnq4rlz7dsjqtqnrllw3vykzhtw3yrmsdp5kc6tsgpkx27r99eshquqkyypwq633sgq2xqayayzn3t76e49av3ecvdgtnvlst33ctpyg4mu5eps",
    description: "Donation Page",
    expiresAt: null,
    amountSats: null,
    payee: "philip@lexe.app",
    payeePubkey: null,
  ),
  amountSat: 33000,
  feesSat: 122,
  status: PaymentStatus.completed,
  statusStr: "completed",
  note: "Donation",
  createdAt: 1748993362000,
  finalizedAt: 1748993364000,
);

const Payment dummyOfferInboundPayment01 = Payment(
  index: PaymentIndex(
      field0:
          "0000001748999074000-fr_016041408597e243d2b7a8fddf2304288f4763809e340b94df32c12deb894927"),
  kind: PaymentKind.offer,
  direction: PaymentDirection.inbound,
  // NOTE: inbound offer payments currently don't have the `offer` field set
  offer: null,
  amountSat: 59690,
  feesSat: 0,
  status: PaymentStatus.completed,
  statusStr: "completed",
  note: null,
  createdAt: 1748999074000,
  finalizedAt: 1748999075000,
);

// Default set of sample payments
List<Payment> defaultDummyPayments = [
  dummyOnchainInboundCompleted01,
  dummyOnchainOutboundFailed01,
  dummySpontaneousOutboundPending01,
  dummyInvoiceOutboundPending01,
  dummyInvoiceInboundPending01,
  dummyInvoiceInboundPending02,
  dummyInvoiceInboundCompleted01,
  dummyInvoiceInboundFailed01,
  dummyOnchainOutboundCompleted01,
  dummyOfferOutboundPayment01,
  dummyOfferInboundPayment01,
].sortedBy((payment) => payment.index.field0);

//
// Dummy channel data
//

const LxChannelDetails dummyChannelUsable01 = LxChannelDetails(
  channelId: "eb3a2ec97707e9218491a48db1b768de3d1170f84dc8ea539a385ce5a1b68527",
  counterpartyNodeId:
      "0314a77523d1dcbc5db56081edcbc24ab820b35e343a6c6769176de707c178d457",
  channelValueSats: 300231 + 477788,
  isUsable: true,
  ourBalanceSats: 300231,
  outboundCapacitySats: 300231 - 1000,
  nextOutboundHtlcLimitSats: 300231 - 1000 - 1198,
  theirBalanceSats: 477788,
  inboundCapacitySats: 477788 - 1000,
);

const LxChannelDetails dummyChannelUsable02 = LxChannelDetails(
  channelId: "4f21130b6117cfba0b771e12e98298b2027b39e41d681726384277c935d30d7a",
  counterpartyNodeId:
      "0314a77523d1dcbc5db56081edcbc24ab820b35e343a6c6769176de707c178d457",
  channelValueSats: 24350 + 1100,
  isUsable: true,
  ourBalanceSats: 24350,
  outboundCapacitySats: 24350 - 1000,
  nextOutboundHtlcLimitSats: 24350 - 1000 - 1198,
  theirBalanceSats: 1100,
  inboundCapacitySats: 1100 - 1000,
);

const LxChannelDetails dummyChannelUnusable01 = LxChannelDetails(
  channelId: "2607641588c8a779a6f7e7e2d110b0c67bc1f01b9bb9a89bbe98c144f0f4b04c",
  counterpartyNodeId:
      "0314a77523d1dcbc5db56081edcbc24ab820b35e343a6c6769176de707c178d457",
  channelValueSats: 776231 + 226787,
  isUsable: false,
  ourBalanceSats: 776231,
  outboundCapacitySats: 776231 - 1000,
  nextOutboundHtlcLimitSats: 776231 - 1000 - 1198,
  theirBalanceSats: 226787,
  inboundCapacitySats: 226787 - 1000,
);

const LxChannelDetails dummyChannelUnusable02 = LxChannelDetails(
  channelId: "2ec634f7ae13ae3509e1044d7be014d320897d3b663e7b8e2a7d27b37ba13127",
  counterpartyNodeId:
      "0314a77523d1dcbc5db56081edcbc24ab820b35e343a6c6769176de707c178d457",
  channelValueSats: 254116 + 43844,
  isUsable: false,
  ourBalanceSats: 254116,
  outboundCapacitySats: 254116 - 1000,
  nextOutboundHtlcLimitSats: 254116 - 1000 - 1198,
  theirBalanceSats: 43844,
  inboundCapacitySats: 43844 - 1000,
);

// Default set of sample channels
const List<LxChannelDetails> defaultDummyChannels = [
  dummyChannelUnusable01,
  dummyChannelUsable01,
  dummyChannelUsable02,
  dummyChannelUnusable02,
];
