/// Abstract interface for payment operations for ease of testing.
library;

import 'package:app_rs_dart/ffi/api.dart'
    show
        CreateInvoiceRequest,
        CreateInvoiceResponse,
        CreateOfferRequest,
        CreateOfferResponse,
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
import 'package:lexeapp/result.dart' show FfiResult;

/// Abstract interface for payment operations.
abstract class PaymentService {
  /// Resolve a payment URI to the best payment method.
  Future<FfiResult<PaymentMethod>> resolveBest({
    required Network network,
    required String uriStr,
  });

  /// Preflight an onchain payment.
  Future<FfiResult<PreflightPayOnchainResponse>> preflightPayOnchain({
    required PreflightPayOnchainRequest req,
  });

  /// Preflight a BOLT11 invoice payment.
  Future<FfiResult<PreflightPayInvoiceResponse>> preflightPayInvoice({
    required PreflightPayInvoiceRequest req,
  });

  /// Preflight a BOLT12 offer payment.
  Future<FfiResult<PreflightPayOfferResponse>> preflightPayOffer({
    required PreflightPayOfferRequest req,
  });

  /// Resolve an LNURL pay request to an invoice.
  Future<FfiResult<Invoice>> resolveLnurlPayRequest({
    required LnurlPayRequest req,
    required int amountMsats,
  });

  /// Pay onchain.
  Future<FfiResult<PayOnchainResponse>> payOnchain({
    required PayOnchainRequest req,
  });

  /// Pay a BOLT11 invoice.
  Future<FfiResult<PayInvoiceResponse>> payInvoice({
    required PayInvoiceRequest req,
  });

  /// Pay a BOLT12 offer.
  Future<FfiResult<PayOfferResponse>> payOffer({required PayOfferRequest req});

  /// Create a BOLT11 invoice.
  Future<FfiResult<CreateInvoiceResponse>> createInvoice({
    required CreateInvoiceRequest req,
  });

  /// Create a BOLT12 offer.
  Future<FfiResult<CreateOfferResponse>> createOffer({
    required CreateOfferRequest req,
  });

  /// Get a new onchain address.
  Future<FfiResult<String>> getAddress();
}
