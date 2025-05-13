// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'api.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

T _$identity<T>(T value) => value;

final _privateConstructorUsedError = UnsupportedError(
    'It seems like you constructed your class using `MyClass._()`. This constructor is only meant to be used by freezed and you are not supposed to need it nor use it.\nPlease check the documentation here for more information: https://github.com/rrousselGit/freezed#adding-getters-and-methods-to-our-models');

/// @nodoc
mixin _$Balance {
  int get totalSats => throw _privateConstructorUsedError;
  int get onchainSats => throw _privateConstructorUsedError;
  int get lightningSats => throw _privateConstructorUsedError;
  int get lightningMaxSendableSats => throw _privateConstructorUsedError;
}

/// @nodoc

class _$BalanceImpl implements _Balance {
  const _$BalanceImpl(
      {required this.totalSats,
      required this.onchainSats,
      required this.lightningSats,
      required this.lightningMaxSendableSats});

  @override
  final int totalSats;
  @override
  final int onchainSats;
  @override
  final int lightningSats;
  @override
  final int lightningMaxSendableSats;

  @override
  String toString() {
    return 'Balance(totalSats: $totalSats, onchainSats: $onchainSats, lightningSats: $lightningSats, lightningMaxSendableSats: $lightningMaxSendableSats)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$BalanceImpl &&
            (identical(other.totalSats, totalSats) ||
                other.totalSats == totalSats) &&
            (identical(other.onchainSats, onchainSats) ||
                other.onchainSats == onchainSats) &&
            (identical(other.lightningSats, lightningSats) ||
                other.lightningSats == lightningSats) &&
            (identical(
                    other.lightningMaxSendableSats, lightningMaxSendableSats) ||
                other.lightningMaxSendableSats == lightningMaxSendableSats));
  }

  @override
  int get hashCode => Object.hash(runtimeType, totalSats, onchainSats,
      lightningSats, lightningMaxSendableSats);
}

abstract class _Balance implements Balance {
  const factory _Balance(
      {required final int totalSats,
      required final int onchainSats,
      required final int lightningSats,
      required final int lightningMaxSendableSats}) = _$BalanceImpl;

  @override
  int get totalSats;
  @override
  int get onchainSats;
  @override
  int get lightningSats;
  @override
  int get lightningMaxSendableSats;
}

/// @nodoc
mixin _$CloseChannelRequest {
  String get channelId => throw _privateConstructorUsedError;
}

/// @nodoc

class _$CloseChannelRequestImpl implements _CloseChannelRequest {
  const _$CloseChannelRequestImpl({required this.channelId});

  @override
  final String channelId;

  @override
  String toString() {
    return 'CloseChannelRequest(channelId: $channelId)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$CloseChannelRequestImpl &&
            (identical(other.channelId, channelId) ||
                other.channelId == channelId));
  }

  @override
  int get hashCode => Object.hash(runtimeType, channelId);
}

abstract class _CloseChannelRequest implements CloseChannelRequest {
  const factory _CloseChannelRequest({required final String channelId}) =
      _$CloseChannelRequestImpl;

  @override
  String get channelId;
}

/// @nodoc
mixin _$CreateClientRequest {
  String? get label => throw _privateConstructorUsedError;
  Scope get scope => throw _privateConstructorUsedError;
}

/// @nodoc

class _$CreateClientRequestImpl implements _CreateClientRequest {
  const _$CreateClientRequestImpl({this.label, required this.scope});

  @override
  final String? label;
  @override
  final Scope scope;

  @override
  String toString() {
    return 'CreateClientRequest(label: $label, scope: $scope)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$CreateClientRequestImpl &&
            (identical(other.label, label) || other.label == label) &&
            (identical(other.scope, scope) || other.scope == scope));
  }

  @override
  int get hashCode => Object.hash(runtimeType, label, scope);
}

abstract class _CreateClientRequest implements CreateClientRequest {
  const factory _CreateClientRequest(
      {final String? label,
      required final Scope scope}) = _$CreateClientRequestImpl;

  @override
  String? get label;
  @override
  Scope get scope;
}

/// @nodoc
mixin _$CreateClientResponse {
  RevocableClient get client => throw _privateConstructorUsedError;
  String get credentials => throw _privateConstructorUsedError;
}

/// @nodoc

class _$CreateClientResponseImpl implements _CreateClientResponse {
  const _$CreateClientResponseImpl(
      {required this.client, required this.credentials});

  @override
  final RevocableClient client;
  @override
  final String credentials;

  @override
  String toString() {
    return 'CreateClientResponse(client: $client, credentials: $credentials)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$CreateClientResponseImpl &&
            (identical(other.client, client) || other.client == client) &&
            (identical(other.credentials, credentials) ||
                other.credentials == credentials));
  }

  @override
  int get hashCode => Object.hash(runtimeType, client, credentials);
}

abstract class _CreateClientResponse implements CreateClientResponse {
  const factory _CreateClientResponse(
      {required final RevocableClient client,
      required final String credentials}) = _$CreateClientResponseImpl;

  @override
  RevocableClient get client;
  @override
  String get credentials;
}

/// @nodoc
mixin _$CreateInvoiceRequest {
  int get expirySecs => throw _privateConstructorUsedError;
  int? get amountSats => throw _privateConstructorUsedError;
  String? get description => throw _privateConstructorUsedError;
}

/// @nodoc

class _$CreateInvoiceRequestImpl implements _CreateInvoiceRequest {
  const _$CreateInvoiceRequestImpl(
      {required this.expirySecs, this.amountSats, this.description});

  @override
  final int expirySecs;
  @override
  final int? amountSats;
  @override
  final String? description;

  @override
  String toString() {
    return 'CreateInvoiceRequest(expirySecs: $expirySecs, amountSats: $amountSats, description: $description)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$CreateInvoiceRequestImpl &&
            (identical(other.expirySecs, expirySecs) ||
                other.expirySecs == expirySecs) &&
            (identical(other.amountSats, amountSats) ||
                other.amountSats == amountSats) &&
            (identical(other.description, description) ||
                other.description == description));
  }

  @override
  int get hashCode =>
      Object.hash(runtimeType, expirySecs, amountSats, description);
}

abstract class _CreateInvoiceRequest implements CreateInvoiceRequest {
  const factory _CreateInvoiceRequest(
      {required final int expirySecs,
      final int? amountSats,
      final String? description}) = _$CreateInvoiceRequestImpl;

  @override
  int get expirySecs;
  @override
  int? get amountSats;
  @override
  String? get description;
}

/// @nodoc
mixin _$CreateInvoiceResponse {
  Invoice get invoice => throw _privateConstructorUsedError;
}

/// @nodoc

class _$CreateInvoiceResponseImpl implements _CreateInvoiceResponse {
  const _$CreateInvoiceResponseImpl({required this.invoice});

  @override
  final Invoice invoice;

  @override
  String toString() {
    return 'CreateInvoiceResponse(invoice: $invoice)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$CreateInvoiceResponseImpl &&
            (identical(other.invoice, invoice) || other.invoice == invoice));
  }

  @override
  int get hashCode => Object.hash(runtimeType, invoice);
}

abstract class _CreateInvoiceResponse implements CreateInvoiceResponse {
  const factory _CreateInvoiceResponse({required final Invoice invoice}) =
      _$CreateInvoiceResponseImpl;

  @override
  Invoice get invoice;
}

/// @nodoc
mixin _$CreateOfferRequest {
  int? get expirySecs => throw _privateConstructorUsedError;
  int? get amountSats => throw _privateConstructorUsedError;
  String? get description => throw _privateConstructorUsedError;
}

/// @nodoc

class _$CreateOfferRequestImpl implements _CreateOfferRequest {
  const _$CreateOfferRequestImpl(
      {this.expirySecs, this.amountSats, this.description});

  @override
  final int? expirySecs;
  @override
  final int? amountSats;
  @override
  final String? description;

  @override
  String toString() {
    return 'CreateOfferRequest(expirySecs: $expirySecs, amountSats: $amountSats, description: $description)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$CreateOfferRequestImpl &&
            (identical(other.expirySecs, expirySecs) ||
                other.expirySecs == expirySecs) &&
            (identical(other.amountSats, amountSats) ||
                other.amountSats == amountSats) &&
            (identical(other.description, description) ||
                other.description == description));
  }

  @override
  int get hashCode =>
      Object.hash(runtimeType, expirySecs, amountSats, description);
}

abstract class _CreateOfferRequest implements CreateOfferRequest {
  const factory _CreateOfferRequest(
      {final int? expirySecs,
      final int? amountSats,
      final String? description}) = _$CreateOfferRequestImpl;

  @override
  int? get expirySecs;
  @override
  int? get amountSats;
  @override
  String? get description;
}

/// @nodoc
mixin _$CreateOfferResponse {
  Offer get offer => throw _privateConstructorUsedError;
}

/// @nodoc

class _$CreateOfferResponseImpl implements _CreateOfferResponse {
  const _$CreateOfferResponseImpl({required this.offer});

  @override
  final Offer offer;

  @override
  String toString() {
    return 'CreateOfferResponse(offer: $offer)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$CreateOfferResponseImpl &&
            (identical(other.offer, offer) || other.offer == offer));
  }

  @override
  int get hashCode => Object.hash(runtimeType, offer);
}

abstract class _CreateOfferResponse implements CreateOfferResponse {
  const factory _CreateOfferResponse({required final Offer offer}) =
      _$CreateOfferResponseImpl;

  @override
  Offer get offer;
}

/// @nodoc
mixin _$FeeEstimate {
  int get amountSats => throw _privateConstructorUsedError;
}

/// @nodoc

class _$FeeEstimateImpl implements _FeeEstimate {
  const _$FeeEstimateImpl({required this.amountSats});

  @override
  final int amountSats;

  @override
  String toString() {
    return 'FeeEstimate(amountSats: $amountSats)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$FeeEstimateImpl &&
            (identical(other.amountSats, amountSats) ||
                other.amountSats == amountSats));
  }

  @override
  int get hashCode => Object.hash(runtimeType, amountSats);
}

abstract class _FeeEstimate implements FeeEstimate {
  const factory _FeeEstimate({required final int amountSats}) =
      _$FeeEstimateImpl;

  @override
  int get amountSats;
}

/// @nodoc
mixin _$FiatRate {
  String get fiat => throw _privateConstructorUsedError;
  double get rate => throw _privateConstructorUsedError;
}

/// @nodoc

class _$FiatRateImpl implements _FiatRate {
  const _$FiatRateImpl({required this.fiat, required this.rate});

  @override
  final String fiat;
  @override
  final double rate;

  @override
  String toString() {
    return 'FiatRate(fiat: $fiat, rate: $rate)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$FiatRateImpl &&
            (identical(other.fiat, fiat) || other.fiat == fiat) &&
            (identical(other.rate, rate) || other.rate == rate));
  }

  @override
  int get hashCode => Object.hash(runtimeType, fiat, rate);
}

abstract class _FiatRate implements FiatRate {
  const factory _FiatRate(
      {required final String fiat,
      required final double rate}) = _$FiatRateImpl;

  @override
  String get fiat;
  @override
  double get rate;
}

/// @nodoc
mixin _$FiatRates {
  int get timestampMs => throw _privateConstructorUsedError;
  List<FiatRate> get rates => throw _privateConstructorUsedError;
}

/// @nodoc

class _$FiatRatesImpl implements _FiatRates {
  const _$FiatRatesImpl(
      {required this.timestampMs, required final List<FiatRate> rates})
      : _rates = rates;

  @override
  final int timestampMs;
  final List<FiatRate> _rates;
  @override
  List<FiatRate> get rates {
    if (_rates is EqualUnmodifiableListView) return _rates;
    // ignore: implicit_dynamic_type
    return EqualUnmodifiableListView(_rates);
  }

  @override
  String toString() {
    return 'FiatRates(timestampMs: $timestampMs, rates: $rates)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$FiatRatesImpl &&
            (identical(other.timestampMs, timestampMs) ||
                other.timestampMs == timestampMs) &&
            const DeepCollectionEquality().equals(other._rates, _rates));
  }

  @override
  int get hashCode => Object.hash(
      runtimeType, timestampMs, const DeepCollectionEquality().hash(_rates));
}

abstract class _FiatRates implements FiatRates {
  const factory _FiatRates(
      {required final int timestampMs,
      required final List<FiatRate> rates}) = _$FiatRatesImpl;

  @override
  int get timestampMs;
  @override
  List<FiatRate> get rates;
}

/// @nodoc
mixin _$ListChannelsResponse {
  List<LxChannelDetails> get channels => throw _privateConstructorUsedError;
}

/// @nodoc

class _$ListChannelsResponseImpl implements _ListChannelsResponse {
  const _$ListChannelsResponseImpl(
      {required final List<LxChannelDetails> channels})
      : _channels = channels;

  final List<LxChannelDetails> _channels;
  @override
  List<LxChannelDetails> get channels {
    if (_channels is EqualUnmodifiableListView) return _channels;
    // ignore: implicit_dynamic_type
    return EqualUnmodifiableListView(_channels);
  }

  @override
  String toString() {
    return 'ListChannelsResponse(channels: $channels)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$ListChannelsResponseImpl &&
            const DeepCollectionEquality().equals(other._channels, _channels));
  }

  @override
  int get hashCode =>
      Object.hash(runtimeType, const DeepCollectionEquality().hash(_channels));
}

abstract class _ListChannelsResponse implements ListChannelsResponse {
  const factory _ListChannelsResponse(
          {required final List<LxChannelDetails> channels}) =
      _$ListChannelsResponseImpl;

  @override
  List<LxChannelDetails> get channels;
}

/// @nodoc
mixin _$NodeInfo {
  String get nodePk => throw _privateConstructorUsedError;
  String get version => throw _privateConstructorUsedError;
  String get measurement => throw _privateConstructorUsedError;
  Balance get balance => throw _privateConstructorUsedError;
}

/// @nodoc

class _$NodeInfoImpl implements _NodeInfo {
  const _$NodeInfoImpl(
      {required this.nodePk,
      required this.version,
      required this.measurement,
      required this.balance});

  @override
  final String nodePk;
  @override
  final String version;
  @override
  final String measurement;
  @override
  final Balance balance;

  @override
  String toString() {
    return 'NodeInfo(nodePk: $nodePk, version: $version, measurement: $measurement, balance: $balance)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$NodeInfoImpl &&
            (identical(other.nodePk, nodePk) || other.nodePk == nodePk) &&
            (identical(other.version, version) || other.version == version) &&
            (identical(other.measurement, measurement) ||
                other.measurement == measurement) &&
            (identical(other.balance, balance) || other.balance == balance));
  }

  @override
  int get hashCode =>
      Object.hash(runtimeType, nodePk, version, measurement, balance);
}

abstract class _NodeInfo implements NodeInfo {
  const factory _NodeInfo(
      {required final String nodePk,
      required final String version,
      required final String measurement,
      required final Balance balance}) = _$NodeInfoImpl;

  @override
  String get nodePk;
  @override
  String get version;
  @override
  String get measurement;
  @override
  Balance get balance;
}

/// @nodoc
mixin _$OpenChannelRequest {
  UserChannelId get userChannelId => throw _privateConstructorUsedError;
  int get valueSats => throw _privateConstructorUsedError;
}

/// @nodoc

class _$OpenChannelRequestImpl implements _OpenChannelRequest {
  const _$OpenChannelRequestImpl(
      {required this.userChannelId, required this.valueSats});

  @override
  final UserChannelId userChannelId;
  @override
  final int valueSats;

  @override
  String toString() {
    return 'OpenChannelRequest(userChannelId: $userChannelId, valueSats: $valueSats)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$OpenChannelRequestImpl &&
            (identical(other.userChannelId, userChannelId) ||
                other.userChannelId == userChannelId) &&
            (identical(other.valueSats, valueSats) ||
                other.valueSats == valueSats));
  }

  @override
  int get hashCode => Object.hash(runtimeType, userChannelId, valueSats);
}

abstract class _OpenChannelRequest implements OpenChannelRequest {
  const factory _OpenChannelRequest(
      {required final UserChannelId userChannelId,
      required final int valueSats}) = _$OpenChannelRequestImpl;

  @override
  UserChannelId get userChannelId;
  @override
  int get valueSats;
}

/// @nodoc
mixin _$OpenChannelResponse {
  String get channelId => throw _privateConstructorUsedError;
}

/// @nodoc

class _$OpenChannelResponseImpl implements _OpenChannelResponse {
  const _$OpenChannelResponseImpl({required this.channelId});

  @override
  final String channelId;

  @override
  String toString() {
    return 'OpenChannelResponse(channelId: $channelId)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$OpenChannelResponseImpl &&
            (identical(other.channelId, channelId) ||
                other.channelId == channelId));
  }

  @override
  int get hashCode => Object.hash(runtimeType, channelId);
}

abstract class _OpenChannelResponse implements OpenChannelResponse {
  const factory _OpenChannelResponse({required final String channelId}) =
      _$OpenChannelResponseImpl;

  @override
  String get channelId;
}

/// @nodoc
mixin _$PayInvoiceRequest {
  String get invoice => throw _privateConstructorUsedError;
  int? get fallbackAmountSats => throw _privateConstructorUsedError;
  String? get note => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PayInvoiceRequestImpl implements _PayInvoiceRequest {
  const _$PayInvoiceRequestImpl(
      {required this.invoice, this.fallbackAmountSats, this.note});

  @override
  final String invoice;
  @override
  final int? fallbackAmountSats;
  @override
  final String? note;

  @override
  String toString() {
    return 'PayInvoiceRequest(invoice: $invoice, fallbackAmountSats: $fallbackAmountSats, note: $note)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PayInvoiceRequestImpl &&
            (identical(other.invoice, invoice) || other.invoice == invoice) &&
            (identical(other.fallbackAmountSats, fallbackAmountSats) ||
                other.fallbackAmountSats == fallbackAmountSats) &&
            (identical(other.note, note) || other.note == note));
  }

  @override
  int get hashCode =>
      Object.hash(runtimeType, invoice, fallbackAmountSats, note);
}

abstract class _PayInvoiceRequest implements PayInvoiceRequest {
  const factory _PayInvoiceRequest(
      {required final String invoice,
      final int? fallbackAmountSats,
      final String? note}) = _$PayInvoiceRequestImpl;

  @override
  String get invoice;
  @override
  int? get fallbackAmountSats;
  @override
  String? get note;
}

/// @nodoc
mixin _$PayInvoiceResponse {
  PaymentIndex get index => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PayInvoiceResponseImpl implements _PayInvoiceResponse {
  const _$PayInvoiceResponseImpl({required this.index});

  @override
  final PaymentIndex index;

  @override
  String toString() {
    return 'PayInvoiceResponse(index: $index)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PayInvoiceResponseImpl &&
            (identical(other.index, index) || other.index == index));
  }

  @override
  int get hashCode => Object.hash(runtimeType, index);
}

abstract class _PayInvoiceResponse implements PayInvoiceResponse {
  const factory _PayInvoiceResponse({required final PaymentIndex index}) =
      _$PayInvoiceResponseImpl;

  @override
  PaymentIndex get index;
}

/// @nodoc
mixin _$PayOnchainRequest {
  ClientPaymentId get cid => throw _privateConstructorUsedError;
  String get address => throw _privateConstructorUsedError;
  int get amountSats => throw _privateConstructorUsedError;
  ConfirmationPriority get priority => throw _privateConstructorUsedError;
  String? get note => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PayOnchainRequestImpl implements _PayOnchainRequest {
  const _$PayOnchainRequestImpl(
      {required this.cid,
      required this.address,
      required this.amountSats,
      required this.priority,
      this.note});

  @override
  final ClientPaymentId cid;
  @override
  final String address;
  @override
  final int amountSats;
  @override
  final ConfirmationPriority priority;
  @override
  final String? note;

  @override
  String toString() {
    return 'PayOnchainRequest(cid: $cid, address: $address, amountSats: $amountSats, priority: $priority, note: $note)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PayOnchainRequestImpl &&
            (identical(other.cid, cid) || other.cid == cid) &&
            (identical(other.address, address) || other.address == address) &&
            (identical(other.amountSats, amountSats) ||
                other.amountSats == amountSats) &&
            (identical(other.priority, priority) ||
                other.priority == priority) &&
            (identical(other.note, note) || other.note == note));
  }

  @override
  int get hashCode =>
      Object.hash(runtimeType, cid, address, amountSats, priority, note);
}

abstract class _PayOnchainRequest implements PayOnchainRequest {
  const factory _PayOnchainRequest(
      {required final ClientPaymentId cid,
      required final String address,
      required final int amountSats,
      required final ConfirmationPriority priority,
      final String? note}) = _$PayOnchainRequestImpl;

  @override
  ClientPaymentId get cid;
  @override
  String get address;
  @override
  int get amountSats;
  @override
  ConfirmationPriority get priority;
  @override
  String? get note;
}

/// @nodoc
mixin _$PayOnchainResponse {
  PaymentIndex get index => throw _privateConstructorUsedError;
  String get txid => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PayOnchainResponseImpl implements _PayOnchainResponse {
  const _$PayOnchainResponseImpl({required this.index, required this.txid});

  @override
  final PaymentIndex index;
  @override
  final String txid;

  @override
  String toString() {
    return 'PayOnchainResponse(index: $index, txid: $txid)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PayOnchainResponseImpl &&
            (identical(other.index, index) || other.index == index) &&
            (identical(other.txid, txid) || other.txid == txid));
  }

  @override
  int get hashCode => Object.hash(runtimeType, index, txid);
}

abstract class _PayOnchainResponse implements PayOnchainResponse {
  const factory _PayOnchainResponse(
      {required final PaymentIndex index,
      required final String txid}) = _$PayOnchainResponseImpl;

  @override
  PaymentIndex get index;
  @override
  String get txid;
}

/// @nodoc
mixin _$PreflightCloseChannelResponse {
  int get feeEstimateSats => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PreflightCloseChannelResponseImpl
    implements _PreflightCloseChannelResponse {
  const _$PreflightCloseChannelResponseImpl({required this.feeEstimateSats});

  @override
  final int feeEstimateSats;

  @override
  String toString() {
    return 'PreflightCloseChannelResponse(feeEstimateSats: $feeEstimateSats)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PreflightCloseChannelResponseImpl &&
            (identical(other.feeEstimateSats, feeEstimateSats) ||
                other.feeEstimateSats == feeEstimateSats));
  }

  @override
  int get hashCode => Object.hash(runtimeType, feeEstimateSats);
}

abstract class _PreflightCloseChannelResponse
    implements PreflightCloseChannelResponse {
  const factory _PreflightCloseChannelResponse(
          {required final int feeEstimateSats}) =
      _$PreflightCloseChannelResponseImpl;

  @override
  int get feeEstimateSats;
}

/// @nodoc
mixin _$PreflightOpenChannelRequest {
  int get valueSats => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PreflightOpenChannelRequestImpl
    implements _PreflightOpenChannelRequest {
  const _$PreflightOpenChannelRequestImpl({required this.valueSats});

  @override
  final int valueSats;

  @override
  String toString() {
    return 'PreflightOpenChannelRequest(valueSats: $valueSats)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PreflightOpenChannelRequestImpl &&
            (identical(other.valueSats, valueSats) ||
                other.valueSats == valueSats));
  }

  @override
  int get hashCode => Object.hash(runtimeType, valueSats);
}

abstract class _PreflightOpenChannelRequest
    implements PreflightOpenChannelRequest {
  const factory _PreflightOpenChannelRequest({required final int valueSats}) =
      _$PreflightOpenChannelRequestImpl;

  @override
  int get valueSats;
}

/// @nodoc
mixin _$PreflightOpenChannelResponse {
  int get feeEstimateSats => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PreflightOpenChannelResponseImpl
    implements _PreflightOpenChannelResponse {
  const _$PreflightOpenChannelResponseImpl({required this.feeEstimateSats});

  @override
  final int feeEstimateSats;

  @override
  String toString() {
    return 'PreflightOpenChannelResponse(feeEstimateSats: $feeEstimateSats)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PreflightOpenChannelResponseImpl &&
            (identical(other.feeEstimateSats, feeEstimateSats) ||
                other.feeEstimateSats == feeEstimateSats));
  }

  @override
  int get hashCode => Object.hash(runtimeType, feeEstimateSats);
}

abstract class _PreflightOpenChannelResponse
    implements PreflightOpenChannelResponse {
  const factory _PreflightOpenChannelResponse(
          {required final int feeEstimateSats}) =
      _$PreflightOpenChannelResponseImpl;

  @override
  int get feeEstimateSats;
}

/// @nodoc
mixin _$PreflightPayInvoiceRequest {
  String get invoice => throw _privateConstructorUsedError;
  int? get fallbackAmountSats => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PreflightPayInvoiceRequestImpl implements _PreflightPayInvoiceRequest {
  const _$PreflightPayInvoiceRequestImpl(
      {required this.invoice, this.fallbackAmountSats});

  @override
  final String invoice;
  @override
  final int? fallbackAmountSats;

  @override
  String toString() {
    return 'PreflightPayInvoiceRequest(invoice: $invoice, fallbackAmountSats: $fallbackAmountSats)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PreflightPayInvoiceRequestImpl &&
            (identical(other.invoice, invoice) || other.invoice == invoice) &&
            (identical(other.fallbackAmountSats, fallbackAmountSats) ||
                other.fallbackAmountSats == fallbackAmountSats));
  }

  @override
  int get hashCode => Object.hash(runtimeType, invoice, fallbackAmountSats);
}

abstract class _PreflightPayInvoiceRequest
    implements PreflightPayInvoiceRequest {
  const factory _PreflightPayInvoiceRequest(
      {required final String invoice,
      final int? fallbackAmountSats}) = _$PreflightPayInvoiceRequestImpl;

  @override
  String get invoice;
  @override
  int? get fallbackAmountSats;
}

/// @nodoc
mixin _$PreflightPayInvoiceResponse {
  int get amountSats => throw _privateConstructorUsedError;
  int get feesSats => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PreflightPayInvoiceResponseImpl
    implements _PreflightPayInvoiceResponse {
  const _$PreflightPayInvoiceResponseImpl(
      {required this.amountSats, required this.feesSats});

  @override
  final int amountSats;
  @override
  final int feesSats;

  @override
  String toString() {
    return 'PreflightPayInvoiceResponse(amountSats: $amountSats, feesSats: $feesSats)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PreflightPayInvoiceResponseImpl &&
            (identical(other.amountSats, amountSats) ||
                other.amountSats == amountSats) &&
            (identical(other.feesSats, feesSats) ||
                other.feesSats == feesSats));
  }

  @override
  int get hashCode => Object.hash(runtimeType, amountSats, feesSats);
}

abstract class _PreflightPayInvoiceResponse
    implements PreflightPayInvoiceResponse {
  const factory _PreflightPayInvoiceResponse(
      {required final int amountSats,
      required final int feesSats}) = _$PreflightPayInvoiceResponseImpl;

  @override
  int get amountSats;
  @override
  int get feesSats;
}

/// @nodoc
mixin _$PreflightPayOnchainRequest {
  String get address => throw _privateConstructorUsedError;
  int get amountSats => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PreflightPayOnchainRequestImpl implements _PreflightPayOnchainRequest {
  const _$PreflightPayOnchainRequestImpl(
      {required this.address, required this.amountSats});

  @override
  final String address;
  @override
  final int amountSats;

  @override
  String toString() {
    return 'PreflightPayOnchainRequest(address: $address, amountSats: $amountSats)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PreflightPayOnchainRequestImpl &&
            (identical(other.address, address) || other.address == address) &&
            (identical(other.amountSats, amountSats) ||
                other.amountSats == amountSats));
  }

  @override
  int get hashCode => Object.hash(runtimeType, address, amountSats);
}

abstract class _PreflightPayOnchainRequest
    implements PreflightPayOnchainRequest {
  const factory _PreflightPayOnchainRequest(
      {required final String address,
      required final int amountSats}) = _$PreflightPayOnchainRequestImpl;

  @override
  String get address;
  @override
  int get amountSats;
}

/// @nodoc
mixin _$PreflightPayOnchainResponse {
  FeeEstimate? get high => throw _privateConstructorUsedError;
  FeeEstimate get normal => throw _privateConstructorUsedError;
  FeeEstimate get background => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PreflightPayOnchainResponseImpl
    implements _PreflightPayOnchainResponse {
  const _$PreflightPayOnchainResponseImpl(
      {this.high, required this.normal, required this.background});

  @override
  final FeeEstimate? high;
  @override
  final FeeEstimate normal;
  @override
  final FeeEstimate background;

  @override
  String toString() {
    return 'PreflightPayOnchainResponse(high: $high, normal: $normal, background: $background)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PreflightPayOnchainResponseImpl &&
            (identical(other.high, high) || other.high == high) &&
            (identical(other.normal, normal) || other.normal == normal) &&
            (identical(other.background, background) ||
                other.background == background));
  }

  @override
  int get hashCode => Object.hash(runtimeType, high, normal, background);
}

abstract class _PreflightPayOnchainResponse
    implements PreflightPayOnchainResponse {
  const factory _PreflightPayOnchainResponse(
          {final FeeEstimate? high,
          required final FeeEstimate normal,
          required final FeeEstimate background}) =
      _$PreflightPayOnchainResponseImpl;

  @override
  FeeEstimate? get high;
  @override
  FeeEstimate get normal;
  @override
  FeeEstimate get background;
}

/// @nodoc
mixin _$UpdateClientRequest {
  String get pubkey => throw _privateConstructorUsedError;
  bool? get isRevoked => throw _privateConstructorUsedError;
}

/// @nodoc

class _$UpdateClientRequestImpl implements _UpdateClientRequest {
  const _$UpdateClientRequestImpl({required this.pubkey, this.isRevoked});

  @override
  final String pubkey;
  @override
  final bool? isRevoked;

  @override
  String toString() {
    return 'UpdateClientRequest(pubkey: $pubkey, isRevoked: $isRevoked)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$UpdateClientRequestImpl &&
            (identical(other.pubkey, pubkey) || other.pubkey == pubkey) &&
            (identical(other.isRevoked, isRevoked) ||
                other.isRevoked == isRevoked));
  }

  @override
  int get hashCode => Object.hash(runtimeType, pubkey, isRevoked);
}

abstract class _UpdateClientRequest implements UpdateClientRequest {
  const factory _UpdateClientRequest(
      {required final String pubkey,
      final bool? isRevoked}) = _$UpdateClientRequestImpl;

  @override
  String get pubkey;
  @override
  bool? get isRevoked;
}

/// @nodoc
mixin _$UpdatePaymentNote {
  PaymentIndex get index => throw _privateConstructorUsedError;
  String? get note => throw _privateConstructorUsedError;
}

/// @nodoc

class _$UpdatePaymentNoteImpl implements _UpdatePaymentNote {
  const _$UpdatePaymentNoteImpl({required this.index, this.note});

  @override
  final PaymentIndex index;
  @override
  final String? note;

  @override
  String toString() {
    return 'UpdatePaymentNote(index: $index, note: $note)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$UpdatePaymentNoteImpl &&
            (identical(other.index, index) || other.index == index) &&
            (identical(other.note, note) || other.note == note));
  }

  @override
  int get hashCode => Object.hash(runtimeType, index, note);
}

abstract class _UpdatePaymentNote implements UpdatePaymentNote {
  const factory _UpdatePaymentNote(
      {required final PaymentIndex index,
      final String? note}) = _$UpdatePaymentNoteImpl;

  @override
  PaymentIndex get index;
  @override
  String? get note;
}
