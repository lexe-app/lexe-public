// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'types.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

T _$identity<T>(T value) => value;

final _privateConstructorUsedError = UnsupportedError(
    'It seems like you constructed your class using `MyClass._()`. This constructor is only meant to be used by freezed and you are not supposed to need it nor use it.\nPlease check the documentation here for more information: https://github.com/rrousselGit/freezed#adding-getters-and-methods-to-our-models');

/// @nodoc
mixin _$AppUserInfo {
  String get userPk => throw _privateConstructorUsedError;
  String get nodePk => throw _privateConstructorUsedError;
  String get nodePkProof => throw _privateConstructorUsedError;
}

/// @nodoc

class _$AppUserInfoImpl implements _AppUserInfo {
  const _$AppUserInfoImpl(
      {required this.userPk, required this.nodePk, required this.nodePkProof});

  @override
  final String userPk;
  @override
  final String nodePk;
  @override
  final String nodePkProof;

  @override
  String toString() {
    return 'AppUserInfo(userPk: $userPk, nodePk: $nodePk, nodePkProof: $nodePkProof)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$AppUserInfoImpl &&
            (identical(other.userPk, userPk) || other.userPk == userPk) &&
            (identical(other.nodePk, nodePk) || other.nodePk == nodePk) &&
            (identical(other.nodePkProof, nodePkProof) ||
                other.nodePkProof == nodePkProof));
  }

  @override
  int get hashCode => Object.hash(runtimeType, userPk, nodePk, nodePkProof);
}

abstract class _AppUserInfo implements AppUserInfo {
  const factory _AppUserInfo(
      {required final String userPk,
      required final String nodePk,
      required final String nodePkProof}) = _$AppUserInfoImpl;

  @override
  String get userPk;
  @override
  String get nodePk;
  @override
  String get nodePkProof;
}

/// @nodoc
mixin _$ClientPaymentId {
  U8Array32 get id => throw _privateConstructorUsedError;
}

/// @nodoc

class _$ClientPaymentIdImpl extends _ClientPaymentId {
  const _$ClientPaymentIdImpl({required this.id}) : super._();

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

abstract class _ClientPaymentId extends ClientPaymentId {
  const factory _ClientPaymentId({required final U8Array32 id}) =
      _$ClientPaymentIdImpl;
  const _ClientPaymentId._() : super._();

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
  String get userAgent => throw _privateConstructorUsedError;
}

/// @nodoc

class _$ConfigImpl implements _Config {
  const _$ConfigImpl(
      {required this.deployEnv,
      required this.network,
      required this.gatewayUrl,
      required this.useSgx,
      required this.baseAppDataDir,
      required this.useMockSecretStore,
      required this.userAgent});

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
  final String userAgent;

  @override
  String toString() {
    return 'Config(deployEnv: $deployEnv, network: $network, gatewayUrl: $gatewayUrl, useSgx: $useSgx, baseAppDataDir: $baseAppDataDir, useMockSecretStore: $useMockSecretStore, userAgent: $userAgent)';
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
                other.useMockSecretStore == useMockSecretStore) &&
            (identical(other.userAgent, userAgent) ||
                other.userAgent == userAgent));
  }

  @override
  int get hashCode => Object.hash(runtimeType, deployEnv, network, gatewayUrl,
      useSgx, baseAppDataDir, useMockSecretStore, userAgent);
}

abstract class _Config implements Config {
  const factory _Config(
      {required final DeployEnv deployEnv,
      required final Network network,
      required final String gatewayUrl,
      required final bool useSgx,
      required final String baseAppDataDir,
      required final bool useMockSecretStore,
      required final String userAgent}) = _$ConfigImpl;

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
  @override
  String get userAgent;
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
mixin _$Offer {
  String get string => throw _privateConstructorUsedError;
  String? get description => throw _privateConstructorUsedError;
  int? get expiresAt => throw _privateConstructorUsedError;
  int? get amountSats => throw _privateConstructorUsedError;
  String? get payee => throw _privateConstructorUsedError;
  String? get payeePubkey => throw _privateConstructorUsedError;
}

/// @nodoc

class _$OfferImpl implements _Offer {
  const _$OfferImpl(
      {required this.string,
      this.description,
      this.expiresAt,
      this.amountSats,
      this.payee,
      this.payeePubkey});

  @override
  final String string;
  @override
  final String? description;
  @override
  final int? expiresAt;
  @override
  final int? amountSats;
  @override
  final String? payee;
  @override
  final String? payeePubkey;

  @override
  String toString() {
    return 'Offer(string: $string, description: $description, expiresAt: $expiresAt, amountSats: $amountSats, payee: $payee, payeePubkey: $payeePubkey)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$OfferImpl &&
            (identical(other.string, string) || other.string == string) &&
            (identical(other.description, description) ||
                other.description == description) &&
            (identical(other.expiresAt, expiresAt) ||
                other.expiresAt == expiresAt) &&
            (identical(other.amountSats, amountSats) ||
                other.amountSats == amountSats) &&
            (identical(other.payee, payee) || other.payee == payee) &&
            (identical(other.payeePubkey, payeePubkey) ||
                other.payeePubkey == payeePubkey));
  }

  @override
  int get hashCode => Object.hash(runtimeType, string, description, expiresAt,
      amountSats, payee, payeePubkey);
}

abstract class _Offer implements Offer {
  const factory _Offer(
      {required final String string,
      final String? description,
      final int? expiresAt,
      final int? amountSats,
      final String? payee,
      final String? payeePubkey}) = _$OfferImpl;

  @override
  String get string;
  @override
  String? get description;
  @override
  int? get expiresAt;
  @override
  int? get amountSats;
  @override
  String? get payee;
  @override
  String? get payeePubkey;
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
mixin _$Payment {
  PaymentIndex get index => throw _privateConstructorUsedError;
  PaymentKind get kind => throw _privateConstructorUsedError;
  PaymentDirection get direction => throw _privateConstructorUsedError;
  Invoice? get invoice => throw _privateConstructorUsedError;
  Offer? get offer => throw _privateConstructorUsedError;
  String? get txid => throw _privateConstructorUsedError;
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
      this.offer,
      this.txid,
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
  final Offer? offer;
  @override
  final String? txid;
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
    return 'Payment(index: $index, kind: $kind, direction: $direction, invoice: $invoice, offer: $offer, txid: $txid, replacement: $replacement, amountSat: $amountSat, feesSat: $feesSat, status: $status, statusStr: $statusStr, note: $note, createdAt: $createdAt, finalizedAt: $finalizedAt)';
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
            (identical(other.offer, offer) || other.offer == offer) &&
            (identical(other.txid, txid) || other.txid == txid) &&
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
      offer,
      txid,
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
      final Offer? offer,
      final String? txid,
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
  Offer? get offer;
  @override
  String? get txid;
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
mixin _$PaymentMethod {
  Object get field0 => throw _privateConstructorUsedError;
}

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

  @override
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

  @override
  Invoice get field0;
}

/// @nodoc

class _$PaymentMethod_OfferImpl extends PaymentMethod_Offer {
  const _$PaymentMethod_OfferImpl(this.field0) : super._();

  @override
  final Offer field0;

  @override
  String toString() {
    return 'PaymentMethod.offer(field0: $field0)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$PaymentMethod_OfferImpl &&
            (identical(other.field0, field0) || other.field0 == field0));
  }

  @override
  int get hashCode => Object.hash(runtimeType, field0);
}

abstract class PaymentMethod_Offer extends PaymentMethod {
  const factory PaymentMethod_Offer(final Offer field0) =
      _$PaymentMethod_OfferImpl;
  const PaymentMethod_Offer._() : super._();

  @override
  Offer get field0;
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
mixin _$UserChannelId {
  U8Array16 get id => throw _privateConstructorUsedError;
}

/// @nodoc

class _$UserChannelIdImpl extends _UserChannelId {
  const _$UserChannelIdImpl({required this.id}) : super._();

  @override
  final U8Array16 id;

  @override
  String toString() {
    return 'UserChannelId(id: $id)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$UserChannelIdImpl &&
            const DeepCollectionEquality().equals(other.id, id));
  }

  @override
  int get hashCode =>
      Object.hash(runtimeType, const DeepCollectionEquality().hash(id));
}

abstract class _UserChannelId extends UserChannelId {
  const factory _UserChannelId({required final U8Array16 id}) =
      _$UserChannelIdImpl;
  const _UserChannelId._() : super._();

  @override
  U8Array16 get id;
}
