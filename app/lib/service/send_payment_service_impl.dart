/// Implementation of [SendPaymentService] that delegates to [AppHandle].
library;

import 'package:app_rs_dart/ffi/api.dart'
    show
        PayInvoiceRequest,
        PayInvoiceResponse,
        PayOfferRequest,
        PayOfferResponse,
        PayOnchainRequest,
        PayOnchainResponse,
        PreflightPayInvoiceRequest,
        PreflightPayInvoiceResponse,
        PreflightPayOfferRequest,
        PreflightPayOfferResponse,
        PreflightPayOnchainRequest,
        PreflightPayOnchainResponse;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart'
    show Invoice, LnurlPayRequest, Network, PaymentMethod;
import 'package:lexeapp/result.dart' show FfiResult, Result;
import 'package:lexeapp/service/send_payment_service.dart'
    show SendPaymentService;

/// Implementation of [SendPaymentService] that delegates to [AppHandle] (Rust FFI).
class SendPaymentServiceImpl implements SendPaymentService {
  const SendPaymentServiceImpl(this._app);

  final AppHandle _app;

  @override
  Future<FfiResult<PaymentMethod>> resolveBest({
    required Network network,
    required String uriStr,
  }) => Result.tryFfiAsync(
    () => this._app.resolveBest(network: network, uriStr: uriStr),
  );

  @override
  Future<FfiResult<PreflightPayOnchainResponse>> preflightPayOnchain({
    required PreflightPayOnchainRequest req,
  }) => Result.tryFfiAsync(() => this._app.preflightPayOnchain(req: req));

  @override
  Future<FfiResult<PreflightPayInvoiceResponse>> preflightPayInvoice({
    required PreflightPayInvoiceRequest req,
  }) => Result.tryFfiAsync(() => this._app.preflightPayInvoice(req: req));

  @override
  Future<FfiResult<PreflightPayOfferResponse>> preflightPayOffer({
    required PreflightPayOfferRequest req,
  }) => Result.tryFfiAsync(() => this._app.preflightPayOffer(req: req));

  @override
  Future<FfiResult<Invoice>> resolveLnurlPayRequest({
    required LnurlPayRequest req,
    required int amountMsats,
  }) => Result.tryFfiAsync(
    () => this._app.resolveLnurlPayRequest(req: req, amountMsats: amountMsats),
  );

  @override
  Future<FfiResult<PayOnchainResponse>> payOnchain({
    required PayOnchainRequest req,
  }) => Result.tryFfiAsync(() => this._app.payOnchain(req: req));

  @override
  Future<FfiResult<PayInvoiceResponse>> payInvoice({
    required PayInvoiceRequest req,
  }) => Result.tryFfiAsync(() => this._app.payInvoice(req: req));

  @override
  Future<FfiResult<PayOfferResponse>> payOffer({
    required PayOfferRequest req,
  }) => Result.tryFfiAsync(() => this._app.payOffer(req: req));
}
