/// Mock SendPaymentService for unit tests.
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
import 'package:app_rs_dart/ffi/types.dart'
    show Invoice, LnurlPayRequest, Network, PaymentMethod;
import 'package:lexeapp/result.dart' show Err, FfiError, FfiResult;
import 'package:lexeapp/service/send_payment_service.dart'
    show SendPaymentService;

/// Mock SendPaymentService for unit tests.
///
/// Configure return values before each test by setting the appropriate
/// `*Result` field. Call tracking is available via the `calls` list.
class MockSendPaymentService implements SendPaymentService {
  // Configurable responses for send operations
  FfiResult<PaymentMethod>? resolveBestResult;
  FfiResult<PreflightPayOnchainResponse>? preflightPayOnchainResult;
  FfiResult<PreflightPayInvoiceResponse>? preflightPayInvoiceResult;
  FfiResult<PreflightPayOfferResponse>? preflightPayOfferResult;
  FfiResult<Invoice>? resolveLnurlPayRequestResult;
  FfiResult<PayOnchainResponse>? payOnchainResult;
  FfiResult<PayInvoiceResponse>? payInvoiceResult;
  FfiResult<PayOfferResponse>? payOfferResult;

  /// Tracked method calls for verification in tests.
  final List<String> calls = [];

  /// Reset all configured responses and call tracking.
  void reset() {
    this.resolveBestResult = null;
    this.preflightPayOnchainResult = null;
    this.preflightPayInvoiceResult = null;
    this.preflightPayOfferResult = null;
    this.resolveLnurlPayRequestResult = null;
    this.payOnchainResult = null;
    this.payInvoiceResult = null;
    this.payOfferResult = null;
    this.calls.clear();
  }

  @override
  Future<FfiResult<PaymentMethod>> resolveBest({
    required Network network,
    required String uriStr,
  }) async {
    this.calls.add('resolveBest($uriStr)');
    return this.resolveBestResult ??
        Err(const FfiError('resolveBest not configured'));
  }

  @override
  Future<FfiResult<PreflightPayOnchainResponse>> preflightPayOnchain({
    required PreflightPayOnchainRequest req,
  }) async {
    this.calls.add('preflightPayOnchain(${req.amountSats})');
    return this.preflightPayOnchainResult ??
        Err(const FfiError('preflightPayOnchain not configured'));
  }

  @override
  Future<FfiResult<PreflightPayInvoiceResponse>> preflightPayInvoice({
    required PreflightPayInvoiceRequest req,
  }) async {
    this.calls.add('preflightPayInvoice(${req.invoice})');
    return this.preflightPayInvoiceResult ??
        Err(const FfiError('preflightPayInvoice not configured'));
  }

  @override
  Future<FfiResult<PreflightPayOfferResponse>> preflightPayOffer({
    required PreflightPayOfferRequest req,
  }) async {
    this.calls.add('preflightPayOffer(${req.offer})');
    return this.preflightPayOfferResult ??
        Err(const FfiError('preflightPayOffer not configured'));
  }

  @override
  Future<FfiResult<Invoice>> resolveLnurlPayRequest({
    required LnurlPayRequest req,
    required int amountMsats,
    String? comment,
  }) async {
    this.calls.add('resolveLnurlPayRequest($amountMsats)');
    return this.resolveLnurlPayRequestResult ??
        Err(const FfiError('resolveLnurlPayRequest not configured'));
  }

  @override
  Future<FfiResult<PayOnchainResponse>> payOnchain({
    required PayOnchainRequest req,
  }) async {
    this.calls.add('payOnchain(${req.amountSats})');
    return this.payOnchainResult ??
        Err(const FfiError('payOnchain not configured'));
  }

  @override
  Future<FfiResult<PayInvoiceResponse>> payInvoice({
    required PayInvoiceRequest req,
  }) async {
    this.calls.add('payInvoice(${req.invoice})');
    return this.payInvoiceResult ??
        Err(const FfiError('payInvoice not configured'));
  }

  @override
  Future<FfiResult<PayOfferResponse>> payOffer({
    required PayOfferRequest req,
  }) async {
    this.calls.add('payOffer(${req.offer})');
    return this.payOfferResult ??
        Err(const FfiError('payOffer not configured'));
  }
}
