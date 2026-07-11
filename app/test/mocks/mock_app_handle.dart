/// Mock AppHandle for unit tests.
library;

import 'package:app_rs_dart/ffi/api.dart'
    show
        PayInvoicePreflightRequest,
        PayInvoicePreflightResponse,
        PayInvoiceRequest,
        PayInvoiceResponse,
        PayOfferPreflightRequest,
        PayOfferPreflightResponse,
        PayOfferRequest,
        PayOfferResponse,
        PayOnchainPreflightRequest,
        PayOnchainPreflightResponse,
        PayOnchainRequest,
        PayOnchainResponse;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart'
    show ClaimMethod, Invoice, LnurlPayRequest, Network, PaymentMethod;

/// Represents an [AppHandle] method which can be mocked, carrying its request
/// type [Req] and response type [Resp] so that stubbing is type-checked.
///
/// See the const list of supported stubs below.
final class Stub<Req, Resp> {
  const Stub._(this.name, this._invocationToArgs);

  /// Function name
  final Symbol name;

  /// Converts [Invocation] from [noSuchMethod] into typed args for the mock fn.
  /// This allows for statically typed args when writing the mock function.
  final dynamic Function(Invocation invocation) _invocationToArgs;
}

// Since the stub arguments need to be converted from Invocation,
// we need to define a function for each unique argument structure.

/// `{req: Foo}` -> `(req)`
Object? _reqArg(Invocation invocation) => invocation.namedArguments[#req];

/// `{network: Network, uriStr: String}` -> `(network, uriStr)`.
Object? _resolveBestArgs(Invocation invocation) => (
  invocation.namedArguments[#network] as Network,
  invocation.namedArguments[#uriStr] as String,
);

/// `{req: LnurlPayRequest, amountMsats: int, comment: String?}`
/// -> `(req, amountMsats, comment)`.
Object? _resolveLnurlPayRequestArgs(Invocation invocation) => (
  invocation.namedArguments[#req] as LnurlPayRequest,
  invocation.namedArguments[#amountMsats] as int,
  invocation.namedArguments[#comment] as String?,
);

// If an AppHandle method needs to be mocked, add it here alphabetically.
// dart format off
const payInvoice = Stub<PayInvoiceRequest, PayInvoiceResponse>._(#payInvoice, _reqArg);
const payOffer = Stub<PayOfferRequest, PayOfferResponse>._(#payOffer, _reqArg);
const payOnchain = Stub<PayOnchainRequest, PayOnchainResponse>._(#payOnchain, _reqArg);
const payInvoicePreflight = Stub<PayInvoicePreflightRequest, PayInvoicePreflightResponse>._(#payInvoicePreflight, _reqArg);
const payOfferPreflight = Stub<PayOfferPreflightRequest, PayOfferPreflightResponse>._(#payOfferPreflight, _reqArg);
const payOnchainPreflight = Stub<PayOnchainPreflightRequest, PayOnchainPreflightResponse>._(#payOnchainPreflight, _reqArg);
const resolveBest = Stub<(Network, String), (PaymentMethod?, ClaimMethod?)>._(#resolveBest, _resolveBestArgs);
const resolveLnurlPayRequest = Stub<(LnurlPayRequest, int, String?), Invoice>._(#resolveLnurlPayRequest, _resolveLnurlPayRequestArgs);
// dart format on

/// Mock [AppHandle] for unit tests.
///
/// Configure responses before each test by invoking [mock] with one of the
/// const [Stub]s above and a typed responder.
class MockAppHandleConfigurable implements AppHandle {
  MockAppHandleConfigurable();

  /// Configured responders, keyed by method name.
  final Map<Symbol, Future<dynamic> Function(Invocation)> _responses = {};

  /// Reset all configured responses.
  void reset() {
    this._responses.clear();
  }

  /// Configure a [responseFn] for the given AppHandle method [stub].
  ///
  /// The responder receives the call's typed request and returns the method's
  /// response. Throw from the responder to exercise the FFI error path.
  void mock<Req, Resp>(
    Stub<Req, Resp> stub,
    Future<Resp> Function(Req req) responseFn,
  ) {
    this._responses[stub.name] = (invocation) =>
        responseFn(stub._invocationToArgs(invocation) as Req);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) {
    // FRB generates functions with named arguments by default
    assert(
      invocation.positionalArguments.isEmpty,
      '${invocation.memberName} was called with positional arguments',
    );

    final responseFn = this._responses[invocation.memberName];
    return responseFn != null
        ? responseFn(invocation)
        : super.noSuchMethod(invocation);
  }
}
