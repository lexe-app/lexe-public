// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'ffi.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

T _$identity<T>(T value) => value;

final _privateConstructorUsedError = UnsupportedError(
    'It seems like you constructed your class using `MyClass._()`. This constructor is only meant to be used by freezed and you are not supposed to need it nor use it.\nPlease check the documentation here for more information: https://github.com/rrousselGit/freezed#adding-getters-and-methods-to-our-models');

/// @nodoc
mixin _$Balance {
  int get totalSats => throw _privateConstructorUsedError;
  int get lightningSats => throw _privateConstructorUsedError;
  int get onchainSats => throw _privateConstructorUsedError;
}

/// @nodoc

class _$BalanceImpl implements _Balance {
  const _$BalanceImpl(
      {required this.totalSats,
      required this.lightningSats,
      required this.onchainSats});

  @override
  final int totalSats;
  @override
  final int lightningSats;
  @override
  final int onchainSats;

  @override
  String toString() {
    return 'Balance(totalSats: $totalSats, lightningSats: $lightningSats, onchainSats: $onchainSats)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$BalanceImpl &&
            (identical(other.totalSats, totalSats) ||
                other.totalSats == totalSats) &&
            (identical(other.lightningSats, lightningSats) ||
                other.lightningSats == lightningSats) &&
            (identical(other.onchainSats, onchainSats) ||
                other.onchainSats == onchainSats));
  }

  @override
  int get hashCode =>
      Object.hash(runtimeType, totalSats, lightningSats, onchainSats);
}

abstract class _Balance implements Balance {
  const factory _Balance(
      {required final int totalSats,
      required final int lightningSats,
      required final int onchainSats}) = _$BalanceImpl;

  @override
  int get totalSats;
  @override
  int get lightningSats;
  @override
  int get onchainSats;
}

/// @nodoc
mixin _$ClientPaymentId {
  U8Array32 get id => throw _privateConstructorUsedError;
}

/// @nodoc

class _$ClientPaymentIdImpl implements _ClientPaymentId {
  const _$ClientPaymentIdImpl({required this.id});

  @override
  final U8Array32 id;

  @override
  String toString() {
    return 'ClientPaymentId(id: $id)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$ClientPaymentIdImpl &&
            const DeepCollectionEquality().equals(other.id, id));
  }

  @override
  int get hashCode =>
      Object.hash(runtimeType, const DeepCollectionEquality().hash(id));
}

abstract class _ClientPaymentId implements ClientPaymentId {
  const factory _ClientPaymentId({required final U8Array32 id}) =
      _$ClientPaymentIdImpl;

  @override
  U8Array32 get id;
}

/// @nodoc
mixin _$Config {
  DeployEnv get deployEnv => throw _privateConstructorUsedError;
  Network get network => throw _privateConstructorUsedError;
  String get gatewayUrl => throw _privateConstructorUsedError;
  bool get useSgx => throw _privateConstructorUsedError;
  String get baseAppDataDir => throw _privateConstructorUsedError;
  bool get useMockSecretStore => throw _privateConstructorUsedError;
}

/// @nodoc

class _$ConfigImpl implements _Config {
  const _$ConfigImpl(
      {required this.deployEnv,
      required this.network,
      required this.gatewayUrl,
      required this.useSgx,
      required this.baseAppDataDir,
      required this.useMockSecretStore});

  @override
  final DeployEnv deployEnv;
  @override
  final Network network;
  @override
  final String gatewayUrl;
  @override
  final bool useSgx;
  @override
  final String baseAppDataDir;
  @override
  final bool useMockSecretStore;

  @override
  String toString() {
    return 'Config(deployEnv: $deployEnv, network: $network, gatewayUrl: $gatewayUrl, useSgx: $useSgx, baseAppDataDir: $baseAppDataDir, useMockSecretStore: $useMockSecretStore)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$ConfigImpl &&
            (identical(other.deployEnv, deployEnv) ||
                other.deployEnv == deployEnv) &&
            (identical(other.network, network) || other.network == network) &&
            (identical(other.gatewayUrl, gatewayUrl) ||
                other.gatewayUrl == gatewayUrl) &&
            (identical(other.useSgx, useSgx) || other.useSgx == useSgx) &&
            (identical(other.baseAppDataDir, baseAppDataDir) ||
                other.baseAppDataDir == baseAppDataDir) &&
            (identical(other.useMockSecretStore, useMockSecretStore) ||
                other.useMockSecretStore == useMockSecretStore));
  }

  @override
  int get hashCode => Object.hash(runtimeType, deployEnv, network, gatewayUrl,
      useSgx, baseAppDataDir, useMockSecretStore);
}

abstract class _Config implements Config {
  const factory _Config(
      {required final DeployEnv deployEnv,
      required final Network network,
      required final String gatewayUrl,
      required final bool useSgx,
      required final String baseAppDataDir,
      required final bool useMockSecretStore}) = _$ConfigImpl;

  @override
  DeployEnv get deployEnv;
  @override
  Network get network;
  @override
  String get gatewayUrl;
  @override
  bool get useSgx;
  @override
  String get baseAppDataDir;
  @override
  bool get useMockSecretStore;
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
mixin _$Invoice {
  String get string => throw _privateConstructorUsedError;
  String? get description => throw _privateConstructorUsedError;
  int get createdAt => throw _privateConstructorUsedError;
  int get expiresAt => throw _privateConstructorUsedError;
  int? get amountSats => throw _privateConstructorUsedError;
  String get payeePubkey => throw _privateConstructorUsedError;
}

/// @nodoc

class _$InvoiceImpl implements _Invoice {
  const _$InvoiceImpl(
      {required this.string,
      this.description,
      required this.createdAt,
      required this.expiresAt,
      this.amountSats,
      required this.payeePubkey});

  @override
  final String string;
  @override
  final String? description;
  @override
  final int createdAt;
  @override
  final int expiresAt;
  @override
  final int? amountSats;
  @override
  final String payeePubkey;

  @override
  String toString() {
    return 'Invoice(string: $string, description: $description, createdAt: $createdAt, expiresAt: $expiresAt, amountSats: $amountSats, payeePubkey: $payeePubkey)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$InvoiceImpl &&
            (identical(other.string, string) || other.string == string) &&
            (identical(other.description, description) ||
                other.description == description) &&
            (identical(other.createdAt, createdAt) ||
                other.createdAt == createdAt) &&
            (identical(other.expiresAt, expiresAt) ||
                other.expiresAt == expiresAt) &&
            (identical(other.amountSats, amountSats) ||
                other.amountSats == amountSats) &&
            (identical(other.payeePubkey, payeePubkey) ||
                other.payeePubkey == payeePubkey));
  }

  @override
  int get hashCode => Object.hash(runtimeType, string, description, createdAt,
      expiresAt, amountSats, payeePubkey);
}

abstract class _Invoice implements Invoice {
  const factory _Invoice(
      {required final String string,
      final String? description,
      required final int createdAt,
      required final int expiresAt,
      final int? amountSats,
      required final String payeePubkey}) = _$InvoiceImpl;

  @override
  String get string;
  @override
  String? get description;
  @override
  int get createdAt;
  @override
  int get expiresAt;
  @override
  int? get amountSats;
  @override
  String get payeePubkey;
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
mixin _$Onchain {
  String get address => throw _privateConstructorUsedError;
  int? get amountSats => throw _privateConstructorUsedError;
  String? get label => throw _privateConstructorUsedError;
  String? get message => throw _privateConstructorUsedError;
}

/// @nodoc

class _$OnchainImpl implements _Onchain {
  const _$OnchainImpl(
      {required this.address, this.amountSats, this.label, this.message});

  @override
  final String address;
  @override
  final int? amountSats;
  @override
  final String? label;
  @override
  final String? message;

  @override
  String toString() {
    return 'Onchain(address: $address, amountSats: $amountSats, label: $label, message: $message)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$OnchainImpl &&
            (identical(other.address, address) || other.address == address) &&
            (identical(other.amountSats, amountSats) ||
                other.amountSats == amountSats) &&
            (identical(other.label, label) || other.label == label) &&
            (identical(other.message, message) || other.message == message));
  }

  @override
  int get hashCode =>
      Object.hash(runtimeType, address, amountSats, label, message);
}

abstract class _Onchain implements Onchain {
  const factory _Onchain(
      {required final String address,
      final int? amountSats,
      final String? label,
      final String? message}) = _$OnchainImpl;

  @override
  String get address;
  @override
  int? get amountSats;
  @override
  String? get label;
  @override
  String? get message;
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
mixin _$Payment {
  PaymentIndex get index => throw _privateConstructorUsedError;
  PaymentKind get kind => throw _privateConstructorUsedError;
  PaymentDirection get direction => throw _privateConstructorUsedError;
  Invoice? get invoice => throw _privateConstructorUsedError;
  String? get replacement => throw _privateConstructorUsedError;
  int? get amountSat => throw _privateConstructorUsedError;
  int get feesSat => throw _privateConstructorUsedError;
  PaymentStatus get status => throw _privateConstructorUsedError;
  String get statusStr => throw _privateConstructorUsedError;
  String? get note => throw _privateConstructorUsedError;
  int get createdAt => throw _privateConstructorUsedError;
  int? get finalizedAt => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PaymentImpl implements _Payment {
  const _$PaymentImpl(
      {required this.index,
      required this.kind,
      required this.direction,
      this.invoice,
      this.replacement,
      this.amountSat,
      required this.feesSat,
      required this.status,
      required this.statusStr,
      this.note,
      required this.createdAt,
      this.finalizedAt});

  @override
  final PaymentIndex index;
  @override
  final PaymentKind kind;
  @override
  final PaymentDirection direction;
  @override
  final Invoice? invoice;
  @override
  final String? replacement;
  @override
  final int? amountSat;
  @override
  final int feesSat;
  @override
  final PaymentStatus status;
  @override
  final String statusStr;
  @override
  final String? note;
  @override
  final int createdAt;
  @override
  final int? finalizedAt;

  @override
  String toString() {
    return 'Payment(index: $index, kind: $kind, direction: $direction, invoice: $invoice, replacement: $replacement, amountSat: $amountSat, feesSat: $feesSat, status: $status, statusStr: $statusStr, note: $note, createdAt: $createdAt, finalizedAt: $finalizedAt)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PaymentImpl &&
            (identical(other.index, index) || other.index == index) &&
            (identical(other.kind, kind) || other.kind == kind) &&
            (identical(other.direction, direction) ||
                other.direction == direction) &&
            (identical(other.invoice, invoice) || other.invoice == invoice) &&
            (identical(other.replacement, replacement) ||
                other.replacement == replacement) &&
            (identical(other.amountSat, amountSat) ||
                other.amountSat == amountSat) &&
            (identical(other.feesSat, feesSat) || other.feesSat == feesSat) &&
            (identical(other.status, status) || other.status == status) &&
            (identical(other.statusStr, statusStr) ||
                other.statusStr == statusStr) &&
            (identical(other.note, note) || other.note == note) &&
            (identical(other.createdAt, createdAt) ||
                other.createdAt == createdAt) &&
            (identical(other.finalizedAt, finalizedAt) ||
                other.finalizedAt == finalizedAt));
  }

  @override
  int get hashCode => Object.hash(
      runtimeType,
      index,
      kind,
      direction,
      invoice,
      replacement,
      amountSat,
      feesSat,
      status,
      statusStr,
      note,
      createdAt,
      finalizedAt);
}

abstract class _Payment implements Payment {
  const factory _Payment(
      {required final PaymentIndex index,
      required final PaymentKind kind,
      required final PaymentDirection direction,
      final Invoice? invoice,
      final String? replacement,
      final int? amountSat,
      required final int feesSat,
      required final PaymentStatus status,
      required final String statusStr,
      final String? note,
      required final int createdAt,
      final int? finalizedAt}) = _$PaymentImpl;

  @override
  PaymentIndex get index;
  @override
  PaymentKind get kind;
  @override
  PaymentDirection get direction;
  @override
  Invoice? get invoice;
  @override
  String? get replacement;
  @override
  int? get amountSat;
  @override
  int get feesSat;
  @override
  PaymentStatus get status;
  @override
  String get statusStr;
  @override
  String? get note;
  @override
  int get createdAt;
  @override
  int? get finalizedAt;
}

/// @nodoc
mixin _$PaymentIndex {
  String get field0 => throw _privateConstructorUsedError;
}

/// @nodoc

class _$PaymentIndexImpl implements _PaymentIndex {
  const _$PaymentIndexImpl({required this.field0});

  @override
  final String field0;

  @override
  String toString() {
    return 'PaymentIndex(field0: $field0)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PaymentIndexImpl &&
            (identical(other.field0, field0) || other.field0 == field0));
  }

  @override
  int get hashCode => Object.hash(runtimeType, field0);
}

abstract class _PaymentIndex implements PaymentIndex {
  const factory _PaymentIndex({required final String field0}) =
      _$PaymentIndexImpl;

  @override
  String get field0;
}

/// @nodoc
mixin _$PaymentMethod {}

/// @nodoc

class _$PaymentMethod_OnchainImpl extends PaymentMethod_Onchain {
  const _$PaymentMethod_OnchainImpl(this.field0) : super._();

  @override
  final Onchain field0;

  @override
  String toString() {
    return 'PaymentMethod.onchain(field0: $field0)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PaymentMethod_OnchainImpl &&
            (identical(other.field0, field0) || other.field0 == field0));
  }

  @override
  int get hashCode => Object.hash(runtimeType, field0);
}

abstract class PaymentMethod_Onchain extends PaymentMethod {
  const factory PaymentMethod_Onchain(final Onchain field0) =
      _$PaymentMethod_OnchainImpl;
  const PaymentMethod_Onchain._() : super._();

  Onchain get field0;
}

/// @nodoc

class _$PaymentMethod_InvoiceImpl extends PaymentMethod_Invoice {
  const _$PaymentMethod_InvoiceImpl(this.field0) : super._();

  @override
  final Invoice field0;

  @override
  String toString() {
    return 'PaymentMethod.invoice(field0: $field0)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PaymentMethod_InvoiceImpl &&
            (identical(other.field0, field0) || other.field0 == field0));
  }

  @override
  int get hashCode => Object.hash(runtimeType, field0);
}

abstract class PaymentMethod_Invoice extends PaymentMethod {
  const factory PaymentMethod_Invoice(final Invoice field0) =
      _$PaymentMethod_InvoiceImpl;
  const PaymentMethod_Invoice._() : super._();

  Invoice get field0;
}

/// @nodoc

class _$PaymentMethod_OfferImpl extends PaymentMethod_Offer {
  const _$PaymentMethod_OfferImpl() : super._();

  @override
  String toString() {
    return 'PaymentMethod.offer()';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PaymentMethod_OfferImpl);
  }

  @override
  int get hashCode => runtimeType.hashCode;
}

abstract class PaymentMethod_Offer extends PaymentMethod {
  const factory PaymentMethod_Offer() = _$PaymentMethod_OfferImpl;
  const PaymentMethod_Offer._() : super._();
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
mixin _$ShortPayment {
  PaymentIndex get index => throw _privateConstructorUsedError;
  PaymentKind get kind => throw _privateConstructorUsedError;
  PaymentDirection get direction => throw _privateConstructorUsedError;
  int? get amountSat => throw _privateConstructorUsedError;
  PaymentStatus get status => throw _privateConstructorUsedError;
  String? get note => throw _privateConstructorUsedError;
  int get createdAt => throw _privateConstructorUsedError;
}

/// @nodoc

class _$ShortPaymentImpl implements _ShortPayment {
  const _$ShortPaymentImpl(
      {required this.index,
      required this.kind,
      required this.direction,
      this.amountSat,
      required this.status,
      this.note,
      required this.createdAt});

  @override
  final PaymentIndex index;
  @override
  final PaymentKind kind;
  @override
  final PaymentDirection direction;
  @override
  final int? amountSat;
  @override
  final PaymentStatus status;
  @override
  final String? note;
  @override
  final int createdAt;

  @override
  String toString() {
    return 'ShortPayment(index: $index, kind: $kind, direction: $direction, amountSat: $amountSat, status: $status, note: $note, createdAt: $createdAt)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$ShortPaymentImpl &&
            (identical(other.index, index) || other.index == index) &&
            (identical(other.kind, kind) || other.kind == kind) &&
            (identical(other.direction, direction) ||
                other.direction == direction) &&
            (identical(other.amountSat, amountSat) ||
                other.amountSat == amountSat) &&
            (identical(other.status, status) || other.status == status) &&
            (identical(other.note, note) || other.note == note) &&
            (identical(other.createdAt, createdAt) ||
                other.createdAt == createdAt));
  }

  @override
  int get hashCode => Object.hash(
      runtimeType, index, kind, direction, amountSat, status, note, createdAt);
}

abstract class _ShortPayment implements ShortPayment {
  const factory _ShortPayment(
      {required final PaymentIndex index,
      required final PaymentKind kind,
      required final PaymentDirection direction,
      final int? amountSat,
      required final PaymentStatus status,
      final String? note,
      required final int createdAt}) = _$ShortPaymentImpl;

  @override
  PaymentIndex get index;
  @override
  PaymentKind get kind;
  @override
  PaymentDirection get direction;
  @override
  int? get amountSat;
  @override
  PaymentStatus get status;
  @override
  String? get note;
  @override
  int get createdAt;
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
