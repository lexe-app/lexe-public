// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'types.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$AppUserInfo {

 String get userPk; String get nodePk; String get nodePkProof;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AppUserInfo&&(identical(other.userPk, userPk) || other.userPk == userPk)&&(identical(other.nodePk, nodePk) || other.nodePk == nodePk)&&(identical(other.nodePkProof, nodePkProof) || other.nodePkProof == nodePkProof));
}


@override
int get hashCode => Object.hash(runtimeType,userPk,nodePk,nodePkProof);

@override
String toString() {
  return 'AppUserInfo(userPk: $userPk, nodePk: $nodePk, nodePkProof: $nodePkProof)';
}


}





/// @nodoc


class _AppUserInfo implements AppUserInfo {
  const _AppUserInfo({required this.userPk, required this.nodePk, required this.nodePkProof});
  

@override final  String userPk;
@override final  String nodePk;
@override final  String nodePkProof;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _AppUserInfo&&(identical(other.userPk, userPk) || other.userPk == userPk)&&(identical(other.nodePk, nodePk) || other.nodePk == nodePk)&&(identical(other.nodePkProof, nodePkProof) || other.nodePkProof == nodePkProof));
}


@override
int get hashCode => Object.hash(runtimeType,userPk,nodePk,nodePkProof);

@override
String toString() {
  return 'AppUserInfo(userPk: $userPk, nodePk: $nodePk, nodePkProof: $nodePkProof)';
}


}




/// @nodoc
mixin _$ClientPaymentId {

 U8Array32 get id;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ClientPaymentId&&const DeepCollectionEquality().equals(other.id, id));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(id));

@override
String toString() {
  return 'ClientPaymentId(id: $id)';
}


}





/// @nodoc


class _ClientPaymentId extends ClientPaymentId {
  const _ClientPaymentId({required this.id}): super._();
  

@override final  U8Array32 id;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _ClientPaymentId&&const DeepCollectionEquality().equals(other.id, id));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(id));

@override
String toString() {
  return 'ClientPaymentId(id: $id)';
}


}




/// @nodoc
mixin _$Config {

 DeployEnv get deployEnv; Network get network; String get gatewayUrl; bool get useSgx; String get lexeDataDir; bool get useMockSecretStore; String get userAgent;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is Config&&(identical(other.deployEnv, deployEnv) || other.deployEnv == deployEnv)&&(identical(other.network, network) || other.network == network)&&(identical(other.gatewayUrl, gatewayUrl) || other.gatewayUrl == gatewayUrl)&&(identical(other.useSgx, useSgx) || other.useSgx == useSgx)&&(identical(other.lexeDataDir, lexeDataDir) || other.lexeDataDir == lexeDataDir)&&(identical(other.useMockSecretStore, useMockSecretStore) || other.useMockSecretStore == useMockSecretStore)&&(identical(other.userAgent, userAgent) || other.userAgent == userAgent));
}


@override
int get hashCode => Object.hash(runtimeType,deployEnv,network,gatewayUrl,useSgx,lexeDataDir,useMockSecretStore,userAgent);

@override
String toString() {
  return 'Config(deployEnv: $deployEnv, network: $network, gatewayUrl: $gatewayUrl, useSgx: $useSgx, lexeDataDir: $lexeDataDir, useMockSecretStore: $useMockSecretStore, userAgent: $userAgent)';
}


}





/// @nodoc


class _Config extends Config {
  const _Config({required this.deployEnv, required this.network, required this.gatewayUrl, required this.useSgx, required this.lexeDataDir, required this.useMockSecretStore, required this.userAgent}): super._();
  

@override final  DeployEnv deployEnv;
@override final  Network network;
@override final  String gatewayUrl;
@override final  bool useSgx;
@override final  String lexeDataDir;
@override final  bool useMockSecretStore;
@override final  String userAgent;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _Config&&(identical(other.deployEnv, deployEnv) || other.deployEnv == deployEnv)&&(identical(other.network, network) || other.network == network)&&(identical(other.gatewayUrl, gatewayUrl) || other.gatewayUrl == gatewayUrl)&&(identical(other.useSgx, useSgx) || other.useSgx == useSgx)&&(identical(other.lexeDataDir, lexeDataDir) || other.lexeDataDir == lexeDataDir)&&(identical(other.useMockSecretStore, useMockSecretStore) || other.useMockSecretStore == useMockSecretStore)&&(identical(other.userAgent, userAgent) || other.userAgent == userAgent));
}


@override
int get hashCode => Object.hash(runtimeType,deployEnv,network,gatewayUrl,useSgx,lexeDataDir,useMockSecretStore,userAgent);

@override
String toString() {
  return 'Config(deployEnv: $deployEnv, network: $network, gatewayUrl: $gatewayUrl, useSgx: $useSgx, lexeDataDir: $lexeDataDir, useMockSecretStore: $useMockSecretStore, userAgent: $userAgent)';
}


}




/// @nodoc
mixin _$GDriveStatus {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is GDriveStatus);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'GDriveStatus()';
}


}





/// @nodoc


class GDriveStatus_Ok extends GDriveStatus {
  const GDriveStatus_Ok(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is GDriveStatus_Ok);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'GDriveStatus.ok()';
}


}




/// @nodoc


class GDriveStatus_Error extends GDriveStatus {
  const GDriveStatus_Error(this.field0): super._();
  

 final  String field0;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is GDriveStatus_Error&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'GDriveStatus.error(field0: $field0)';
}


}




/// @nodoc


class GDriveStatus_Disabled extends GDriveStatus {
  const GDriveStatus_Disabled(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is GDriveStatus_Disabled);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'GDriveStatus.disabled()';
}


}




/// @nodoc
mixin _$Invoice {

 String get string; String? get description; int get createdAt; int get expiresAt; int? get amountSats; String get payeePubkey;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is Invoice&&(identical(other.string, string) || other.string == string)&&(identical(other.description, description) || other.description == description)&&(identical(other.createdAt, createdAt) || other.createdAt == createdAt)&&(identical(other.expiresAt, expiresAt) || other.expiresAt == expiresAt)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.payeePubkey, payeePubkey) || other.payeePubkey == payeePubkey));
}


@override
int get hashCode => Object.hash(runtimeType,string,description,createdAt,expiresAt,amountSats,payeePubkey);

@override
String toString() {
  return 'Invoice(string: $string, description: $description, createdAt: $createdAt, expiresAt: $expiresAt, amountSats: $amountSats, payeePubkey: $payeePubkey)';
}


}





/// @nodoc


class _Invoice implements Invoice {
  const _Invoice({required this.string, this.description, required this.createdAt, required this.expiresAt, this.amountSats, required this.payeePubkey});
  

@override final  String string;
@override final  String? description;
@override final  int createdAt;
@override final  int expiresAt;
@override final  int? amountSats;
@override final  String payeePubkey;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _Invoice&&(identical(other.string, string) || other.string == string)&&(identical(other.description, description) || other.description == description)&&(identical(other.createdAt, createdAt) || other.createdAt == createdAt)&&(identical(other.expiresAt, expiresAt) || other.expiresAt == expiresAt)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.payeePubkey, payeePubkey) || other.payeePubkey == payeePubkey));
}


@override
int get hashCode => Object.hash(runtimeType,string,description,createdAt,expiresAt,amountSats,payeePubkey);

@override
String toString() {
  return 'Invoice(string: $string, description: $description, createdAt: $createdAt, expiresAt: $expiresAt, amountSats: $amountSats, payeePubkey: $payeePubkey)';
}


}




/// @nodoc
mixin _$Offer {

 String get string; String? get description; int? get expiresAt; int? get amountSats; String? get payee; String? get payeePubkey;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is Offer&&(identical(other.string, string) || other.string == string)&&(identical(other.description, description) || other.description == description)&&(identical(other.expiresAt, expiresAt) || other.expiresAt == expiresAt)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.payee, payee) || other.payee == payee)&&(identical(other.payeePubkey, payeePubkey) || other.payeePubkey == payeePubkey));
}


@override
int get hashCode => Object.hash(runtimeType,string,description,expiresAt,amountSats,payee,payeePubkey);

@override
String toString() {
  return 'Offer(string: $string, description: $description, expiresAt: $expiresAt, amountSats: $amountSats, payee: $payee, payeePubkey: $payeePubkey)';
}


}





/// @nodoc


class _Offer implements Offer {
  const _Offer({required this.string, this.description, this.expiresAt, this.amountSats, this.payee, this.payeePubkey});
  

@override final  String string;
@override final  String? description;
@override final  int? expiresAt;
@override final  int? amountSats;
@override final  String? payee;
@override final  String? payeePubkey;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _Offer&&(identical(other.string, string) || other.string == string)&&(identical(other.description, description) || other.description == description)&&(identical(other.expiresAt, expiresAt) || other.expiresAt == expiresAt)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.payee, payee) || other.payee == payee)&&(identical(other.payeePubkey, payeePubkey) || other.payeePubkey == payeePubkey));
}


@override
int get hashCode => Object.hash(runtimeType,string,description,expiresAt,amountSats,payee,payeePubkey);

@override
String toString() {
  return 'Offer(string: $string, description: $description, expiresAt: $expiresAt, amountSats: $amountSats, payee: $payee, payeePubkey: $payeePubkey)';
}


}




/// @nodoc
mixin _$Onchain {

 String get address; int? get amountSats; String? get label; String? get message;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is Onchain&&(identical(other.address, address) || other.address == address)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.label, label) || other.label == label)&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode => Object.hash(runtimeType,address,amountSats,label,message);

@override
String toString() {
  return 'Onchain(address: $address, amountSats: $amountSats, label: $label, message: $message)';
}


}





/// @nodoc


class _Onchain implements Onchain {
  const _Onchain({required this.address, this.amountSats, this.label, this.message});
  

@override final  String address;
@override final  int? amountSats;
@override final  String? label;
@override final  String? message;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _Onchain&&(identical(other.address, address) || other.address == address)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.label, label) || other.label == label)&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode => Object.hash(runtimeType,address,amountSats,label,message);

@override
String toString() {
  return 'Onchain(address: $address, amountSats: $amountSats, label: $label, message: $message)';
}


}




/// @nodoc
mixin _$Payment {

 PaymentCreatedIndex get index; PaymentKind get kind; PaymentDirection get direction; Invoice? get invoice; String? get offerId; Offer? get offer; String? get txid; String? get replacement; int? get amountSat; int get feesSat; PaymentStatus get status; String get statusStr; String? get description; String? get note; String? get payerName; String? get payerNote; int get createdAt; int? get finalizedAt;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is Payment&&(identical(other.index, index) || other.index == index)&&(identical(other.kind, kind) || other.kind == kind)&&(identical(other.direction, direction) || other.direction == direction)&&(identical(other.invoice, invoice) || other.invoice == invoice)&&(identical(other.offerId, offerId) || other.offerId == offerId)&&(identical(other.offer, offer) || other.offer == offer)&&(identical(other.txid, txid) || other.txid == txid)&&(identical(other.replacement, replacement) || other.replacement == replacement)&&(identical(other.amountSat, amountSat) || other.amountSat == amountSat)&&(identical(other.feesSat, feesSat) || other.feesSat == feesSat)&&(identical(other.status, status) || other.status == status)&&(identical(other.statusStr, statusStr) || other.statusStr == statusStr)&&(identical(other.description, description) || other.description == description)&&(identical(other.note, note) || other.note == note)&&(identical(other.payerName, payerName) || other.payerName == payerName)&&(identical(other.payerNote, payerNote) || other.payerNote == payerNote)&&(identical(other.createdAt, createdAt) || other.createdAt == createdAt)&&(identical(other.finalizedAt, finalizedAt) || other.finalizedAt == finalizedAt));
}


@override
int get hashCode => Object.hash(runtimeType,index,kind,direction,invoice,offerId,offer,txid,replacement,amountSat,feesSat,status,statusStr,description,note,payerName,payerNote,createdAt,finalizedAt);

@override
String toString() {
  return 'Payment(index: $index, kind: $kind, direction: $direction, invoice: $invoice, offerId: $offerId, offer: $offer, txid: $txid, replacement: $replacement, amountSat: $amountSat, feesSat: $feesSat, status: $status, statusStr: $statusStr, description: $description, note: $note, payerName: $payerName, payerNote: $payerNote, createdAt: $createdAt, finalizedAt: $finalizedAt)';
}


}





/// @nodoc


class _Payment implements Payment {
  const _Payment({required this.index, required this.kind, required this.direction, this.invoice, this.offerId, this.offer, this.txid, this.replacement, this.amountSat, required this.feesSat, required this.status, required this.statusStr, this.description, this.note, this.payerName, this.payerNote, required this.createdAt, this.finalizedAt});
  

@override final  PaymentCreatedIndex index;
@override final  PaymentKind kind;
@override final  PaymentDirection direction;
@override final  Invoice? invoice;
@override final  String? offerId;
@override final  Offer? offer;
@override final  String? txid;
@override final  String? replacement;
@override final  int? amountSat;
@override final  int feesSat;
@override final  PaymentStatus status;
@override final  String statusStr;
@override final  String? description;
@override final  String? note;
@override final  String? payerName;
@override final  String? payerNote;
@override final  int createdAt;
@override final  int? finalizedAt;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _Payment&&(identical(other.index, index) || other.index == index)&&(identical(other.kind, kind) || other.kind == kind)&&(identical(other.direction, direction) || other.direction == direction)&&(identical(other.invoice, invoice) || other.invoice == invoice)&&(identical(other.offerId, offerId) || other.offerId == offerId)&&(identical(other.offer, offer) || other.offer == offer)&&(identical(other.txid, txid) || other.txid == txid)&&(identical(other.replacement, replacement) || other.replacement == replacement)&&(identical(other.amountSat, amountSat) || other.amountSat == amountSat)&&(identical(other.feesSat, feesSat) || other.feesSat == feesSat)&&(identical(other.status, status) || other.status == status)&&(identical(other.statusStr, statusStr) || other.statusStr == statusStr)&&(identical(other.description, description) || other.description == description)&&(identical(other.note, note) || other.note == note)&&(identical(other.payerName, payerName) || other.payerName == payerName)&&(identical(other.payerNote, payerNote) || other.payerNote == payerNote)&&(identical(other.createdAt, createdAt) || other.createdAt == createdAt)&&(identical(other.finalizedAt, finalizedAt) || other.finalizedAt == finalizedAt));
}


@override
int get hashCode => Object.hash(runtimeType,index,kind,direction,invoice,offerId,offer,txid,replacement,amountSat,feesSat,status,statusStr,description,note,payerName,payerNote,createdAt,finalizedAt);

@override
String toString() {
  return 'Payment(index: $index, kind: $kind, direction: $direction, invoice: $invoice, offerId: $offerId, offer: $offer, txid: $txid, replacement: $replacement, amountSat: $amountSat, feesSat: $feesSat, status: $status, statusStr: $statusStr, description: $description, note: $note, payerName: $payerName, payerNote: $payerNote, createdAt: $createdAt, finalizedAt: $finalizedAt)';
}


}




/// @nodoc
mixin _$PaymentCreatedIndex {

 String get field0;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentCreatedIndex&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'PaymentCreatedIndex(field0: $field0)';
}


}





/// @nodoc


class _PaymentCreatedIndex implements PaymentCreatedIndex {
  const _PaymentCreatedIndex({required this.field0});
  

@override final  String field0;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PaymentCreatedIndex&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'PaymentCreatedIndex(field0: $field0)';
}


}




/// @nodoc
mixin _$PaymentKind {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentKind);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'PaymentKind()';
}


}





/// @nodoc


class PaymentKind_Onchain extends PaymentKind {
  const PaymentKind_Onchain(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentKind_Onchain);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'PaymentKind.onchain()';
}


}




/// @nodoc


class PaymentKind_Invoice extends PaymentKind {
  const PaymentKind_Invoice(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentKind_Invoice);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'PaymentKind.invoice()';
}


}




/// @nodoc


class PaymentKind_Offer extends PaymentKind {
  const PaymentKind_Offer(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentKind_Offer);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'PaymentKind.offer()';
}


}




/// @nodoc


class PaymentKind_Spontaneous extends PaymentKind {
  const PaymentKind_Spontaneous(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentKind_Spontaneous);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'PaymentKind.spontaneous()';
}


}




/// @nodoc


class PaymentKind_WaivedChannelFee extends PaymentKind {
  const PaymentKind_WaivedChannelFee(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentKind_WaivedChannelFee);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'PaymentKind.waivedChannelFee()';
}


}




/// @nodoc


class PaymentKind_WaivedLiquidityFee extends PaymentKind {
  const PaymentKind_WaivedLiquidityFee(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentKind_WaivedLiquidityFee);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'PaymentKind.waivedLiquidityFee()';
}


}




/// @nodoc


class PaymentKind_Unknown extends PaymentKind {
  const PaymentKind_Unknown(this.field0): super._();
  

 final  String field0;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentKind_Unknown&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'PaymentKind.unknown(field0: $field0)';
}


}




/// @nodoc
mixin _$PaymentMethod {

 Object get field0;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentMethod&&const DeepCollectionEquality().equals(other.field0, field0));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(field0));

@override
String toString() {
  return 'PaymentMethod(field0: $field0)';
}


}





/// @nodoc


class PaymentMethod_Onchain extends PaymentMethod {
  const PaymentMethod_Onchain(this.field0): super._();
  

@override final  Onchain field0;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentMethod_Onchain&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'PaymentMethod.onchain(field0: $field0)';
}


}




/// @nodoc


class PaymentMethod_Invoice extends PaymentMethod {
  const PaymentMethod_Invoice(this.field0): super._();
  

@override final  Invoice field0;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentMethod_Invoice&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'PaymentMethod.invoice(field0: $field0)';
}


}




/// @nodoc


class PaymentMethod_Offer extends PaymentMethod {
  const PaymentMethod_Offer(this.field0): super._();
  

@override final  Offer field0;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentMethod_Offer&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'PaymentMethod.offer(field0: $field0)';
}


}




/// @nodoc


class PaymentMethod_LnurlPayRequest extends PaymentMethod {
  const PaymentMethod_LnurlPayRequest(this.field0): super._();
  

@override final  LnurlPayRequest field0;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PaymentMethod_LnurlPayRequest&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'PaymentMethod.lnurlPayRequest(field0: $field0)';
}


}




/// @nodoc
mixin _$ShortPayment {

 PaymentCreatedIndex get index; PaymentKind get kind; PaymentDirection get direction; int? get amountSat; PaymentStatus get status; String? get description; String? get note; int get createdAt;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ShortPayment&&(identical(other.index, index) || other.index == index)&&(identical(other.kind, kind) || other.kind == kind)&&(identical(other.direction, direction) || other.direction == direction)&&(identical(other.amountSat, amountSat) || other.amountSat == amountSat)&&(identical(other.status, status) || other.status == status)&&(identical(other.description, description) || other.description == description)&&(identical(other.note, note) || other.note == note)&&(identical(other.createdAt, createdAt) || other.createdAt == createdAt));
}


@override
int get hashCode => Object.hash(runtimeType,index,kind,direction,amountSat,status,description,note,createdAt);

@override
String toString() {
  return 'ShortPayment(index: $index, kind: $kind, direction: $direction, amountSat: $amountSat, status: $status, description: $description, note: $note, createdAt: $createdAt)';
}


}





/// @nodoc


class _ShortPayment implements ShortPayment {
  const _ShortPayment({required this.index, required this.kind, required this.direction, this.amountSat, required this.status, this.description, this.note, required this.createdAt});
  

@override final  PaymentCreatedIndex index;
@override final  PaymentKind kind;
@override final  PaymentDirection direction;
@override final  int? amountSat;
@override final  PaymentStatus status;
@override final  String? description;
@override final  String? note;
@override final  int createdAt;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _ShortPayment&&(identical(other.index, index) || other.index == index)&&(identical(other.kind, kind) || other.kind == kind)&&(identical(other.direction, direction) || other.direction == direction)&&(identical(other.amountSat, amountSat) || other.amountSat == amountSat)&&(identical(other.status, status) || other.status == status)&&(identical(other.description, description) || other.description == description)&&(identical(other.note, note) || other.note == note)&&(identical(other.createdAt, createdAt) || other.createdAt == createdAt));
}


@override
int get hashCode => Object.hash(runtimeType,index,kind,direction,amountSat,status,description,note,createdAt);

@override
String toString() {
  return 'ShortPayment(index: $index, kind: $kind, direction: $direction, amountSat: $amountSat, status: $status, description: $description, note: $note, createdAt: $createdAt)';
}


}




/// @nodoc
mixin _$UserChannelId {

 U8Array16 get id;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is UserChannelId&&const DeepCollectionEquality().equals(other.id, id));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(id));

@override
String toString() {
  return 'UserChannelId(id: $id)';
}


}





/// @nodoc


class _UserChannelId extends UserChannelId {
  const _UserChannelId({required this.id}): super._();
  

@override final  U8Array16 id;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _UserChannelId&&const DeepCollectionEquality().equals(other.id, id));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(id));

@override
String toString() {
  return 'UserChannelId(id: $id)';
}


}




// dart format on
