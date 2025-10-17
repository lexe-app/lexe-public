// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'api.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$Balance {

 int get totalSats; int get onchainSats; int get lightningSats; int get lightningUsableSats; int get lightningMaxSendableSats;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is Balance&&(identical(other.totalSats, totalSats) || other.totalSats == totalSats)&&(identical(other.onchainSats, onchainSats) || other.onchainSats == onchainSats)&&(identical(other.lightningSats, lightningSats) || other.lightningSats == lightningSats)&&(identical(other.lightningUsableSats, lightningUsableSats) || other.lightningUsableSats == lightningUsableSats)&&(identical(other.lightningMaxSendableSats, lightningMaxSendableSats) || other.lightningMaxSendableSats == lightningMaxSendableSats));
}


@override
int get hashCode => Object.hash(runtimeType,totalSats,onchainSats,lightningSats,lightningUsableSats,lightningMaxSendableSats);

@override
String toString() {
  return 'Balance(totalSats: $totalSats, onchainSats: $onchainSats, lightningSats: $lightningSats, lightningUsableSats: $lightningUsableSats, lightningMaxSendableSats: $lightningMaxSendableSats)';
}


}





/// @nodoc


class _Balance implements Balance {
  const _Balance({required this.totalSats, required this.onchainSats, required this.lightningSats, required this.lightningUsableSats, required this.lightningMaxSendableSats});
  

@override final  int totalSats;
@override final  int onchainSats;
@override final  int lightningSats;
@override final  int lightningUsableSats;
@override final  int lightningMaxSendableSats;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _Balance&&(identical(other.totalSats, totalSats) || other.totalSats == totalSats)&&(identical(other.onchainSats, onchainSats) || other.onchainSats == onchainSats)&&(identical(other.lightningSats, lightningSats) || other.lightningSats == lightningSats)&&(identical(other.lightningUsableSats, lightningUsableSats) || other.lightningUsableSats == lightningUsableSats)&&(identical(other.lightningMaxSendableSats, lightningMaxSendableSats) || other.lightningMaxSendableSats == lightningMaxSendableSats));
}


@override
int get hashCode => Object.hash(runtimeType,totalSats,onchainSats,lightningSats,lightningUsableSats,lightningMaxSendableSats);

@override
String toString() {
  return 'Balance(totalSats: $totalSats, onchainSats: $onchainSats, lightningSats: $lightningSats, lightningUsableSats: $lightningUsableSats, lightningMaxSendableSats: $lightningMaxSendableSats)';
}


}




/// @nodoc
mixin _$CloseChannelRequest {

 String get channelId;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is CloseChannelRequest&&(identical(other.channelId, channelId) || other.channelId == channelId));
}


@override
int get hashCode => Object.hash(runtimeType,channelId);

@override
String toString() {
  return 'CloseChannelRequest(channelId: $channelId)';
}


}





/// @nodoc


class _CloseChannelRequest implements CloseChannelRequest {
  const _CloseChannelRequest({required this.channelId});
  

@override final  String channelId;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _CloseChannelRequest&&(identical(other.channelId, channelId) || other.channelId == channelId));
}


@override
int get hashCode => Object.hash(runtimeType,channelId);

@override
String toString() {
  return 'CloseChannelRequest(channelId: $channelId)';
}


}




/// @nodoc
mixin _$CreateClientRequest {

 String? get label; Scope get scope;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is CreateClientRequest&&(identical(other.label, label) || other.label == label)&&(identical(other.scope, scope) || other.scope == scope));
}


@override
int get hashCode => Object.hash(runtimeType,label,scope);

@override
String toString() {
  return 'CreateClientRequest(label: $label, scope: $scope)';
}


}





/// @nodoc


class _CreateClientRequest implements CreateClientRequest {
  const _CreateClientRequest({this.label, required this.scope});
  

@override final  String? label;
@override final  Scope scope;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _CreateClientRequest&&(identical(other.label, label) || other.label == label)&&(identical(other.scope, scope) || other.scope == scope));
}


@override
int get hashCode => Object.hash(runtimeType,label,scope);

@override
String toString() {
  return 'CreateClientRequest(label: $label, scope: $scope)';
}


}




/// @nodoc
mixin _$CreateClientResponse {

 RevocableClient get client; String get credentials;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is CreateClientResponse&&(identical(other.client, client) || other.client == client)&&(identical(other.credentials, credentials) || other.credentials == credentials));
}


@override
int get hashCode => Object.hash(runtimeType,client,credentials);

@override
String toString() {
  return 'CreateClientResponse(client: $client, credentials: $credentials)';
}


}





/// @nodoc


class _CreateClientResponse implements CreateClientResponse {
  const _CreateClientResponse({required this.client, required this.credentials});
  

@override final  RevocableClient client;
@override final  String credentials;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _CreateClientResponse&&(identical(other.client, client) || other.client == client)&&(identical(other.credentials, credentials) || other.credentials == credentials));
}


@override
int get hashCode => Object.hash(runtimeType,client,credentials);

@override
String toString() {
  return 'CreateClientResponse(client: $client, credentials: $credentials)';
}


}




/// @nodoc
mixin _$CreateInvoiceRequest {

 int get expirySecs; int? get amountSats; String? get description;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is CreateInvoiceRequest&&(identical(other.expirySecs, expirySecs) || other.expirySecs == expirySecs)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.description, description) || other.description == description));
}


@override
int get hashCode => Object.hash(runtimeType,expirySecs,amountSats,description);

@override
String toString() {
  return 'CreateInvoiceRequest(expirySecs: $expirySecs, amountSats: $amountSats, description: $description)';
}


}





/// @nodoc


class _CreateInvoiceRequest implements CreateInvoiceRequest {
  const _CreateInvoiceRequest({required this.expirySecs, this.amountSats, this.description});
  

@override final  int expirySecs;
@override final  int? amountSats;
@override final  String? description;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _CreateInvoiceRequest&&(identical(other.expirySecs, expirySecs) || other.expirySecs == expirySecs)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.description, description) || other.description == description));
}


@override
int get hashCode => Object.hash(runtimeType,expirySecs,amountSats,description);

@override
String toString() {
  return 'CreateInvoiceRequest(expirySecs: $expirySecs, amountSats: $amountSats, description: $description)';
}


}




/// @nodoc
mixin _$CreateInvoiceResponse {

 Invoice get invoice;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is CreateInvoiceResponse&&(identical(other.invoice, invoice) || other.invoice == invoice));
}


@override
int get hashCode => Object.hash(runtimeType,invoice);

@override
String toString() {
  return 'CreateInvoiceResponse(invoice: $invoice)';
}


}





/// @nodoc


class _CreateInvoiceResponse implements CreateInvoiceResponse {
  const _CreateInvoiceResponse({required this.invoice});
  

@override final  Invoice invoice;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _CreateInvoiceResponse&&(identical(other.invoice, invoice) || other.invoice == invoice));
}


@override
int get hashCode => Object.hash(runtimeType,invoice);

@override
String toString() {
  return 'CreateInvoiceResponse(invoice: $invoice)';
}


}




/// @nodoc
mixin _$CreateOfferRequest {

 int? get expirySecs; int? get amountSats; String? get description;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is CreateOfferRequest&&(identical(other.expirySecs, expirySecs) || other.expirySecs == expirySecs)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.description, description) || other.description == description));
}


@override
int get hashCode => Object.hash(runtimeType,expirySecs,amountSats,description);

@override
String toString() {
  return 'CreateOfferRequest(expirySecs: $expirySecs, amountSats: $amountSats, description: $description)';
}


}





/// @nodoc


class _CreateOfferRequest implements CreateOfferRequest {
  const _CreateOfferRequest({this.expirySecs, this.amountSats, this.description});
  

@override final  int? expirySecs;
@override final  int? amountSats;
@override final  String? description;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _CreateOfferRequest&&(identical(other.expirySecs, expirySecs) || other.expirySecs == expirySecs)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.description, description) || other.description == description));
}


@override
int get hashCode => Object.hash(runtimeType,expirySecs,amountSats,description);

@override
String toString() {
  return 'CreateOfferRequest(expirySecs: $expirySecs, amountSats: $amountSats, description: $description)';
}


}




/// @nodoc
mixin _$CreateOfferResponse {

 Offer get offer;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is CreateOfferResponse&&(identical(other.offer, offer) || other.offer == offer));
}


@override
int get hashCode => Object.hash(runtimeType,offer);

@override
String toString() {
  return 'CreateOfferResponse(offer: $offer)';
}


}





/// @nodoc


class _CreateOfferResponse implements CreateOfferResponse {
  const _CreateOfferResponse({required this.offer});
  

@override final  Offer offer;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _CreateOfferResponse&&(identical(other.offer, offer) || other.offer == offer));
}


@override
int get hashCode => Object.hash(runtimeType,offer);

@override
String toString() {
  return 'CreateOfferResponse(offer: $offer)';
}


}




/// @nodoc
mixin _$FeeEstimate {

 int get amountSats;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is FeeEstimate&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats));
}


@override
int get hashCode => Object.hash(runtimeType,amountSats);

@override
String toString() {
  return 'FeeEstimate(amountSats: $amountSats)';
}


}





/// @nodoc


class _FeeEstimate implements FeeEstimate {
  const _FeeEstimate({required this.amountSats});
  

@override final  int amountSats;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _FeeEstimate&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats));
}


@override
int get hashCode => Object.hash(runtimeType,amountSats);

@override
String toString() {
  return 'FeeEstimate(amountSats: $amountSats)';
}


}




/// @nodoc
mixin _$FiatRate {

 String get fiat; double get rate;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is FiatRate&&(identical(other.fiat, fiat) || other.fiat == fiat)&&(identical(other.rate, rate) || other.rate == rate));
}


@override
int get hashCode => Object.hash(runtimeType,fiat,rate);

@override
String toString() {
  return 'FiatRate(fiat: $fiat, rate: $rate)';
}


}





/// @nodoc


class _FiatRate implements FiatRate {
  const _FiatRate({required this.fiat, required this.rate});
  

@override final  String fiat;
@override final  double rate;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _FiatRate&&(identical(other.fiat, fiat) || other.fiat == fiat)&&(identical(other.rate, rate) || other.rate == rate));
}


@override
int get hashCode => Object.hash(runtimeType,fiat,rate);

@override
String toString() {
  return 'FiatRate(fiat: $fiat, rate: $rate)';
}


}




/// @nodoc
mixin _$FiatRates {

 int get timestampMs; List<FiatRate> get rates;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is FiatRates&&(identical(other.timestampMs, timestampMs) || other.timestampMs == timestampMs)&&const DeepCollectionEquality().equals(other.rates, rates));
}


@override
int get hashCode => Object.hash(runtimeType,timestampMs,const DeepCollectionEquality().hash(rates));

@override
String toString() {
  return 'FiatRates(timestampMs: $timestampMs, rates: $rates)';
}


}





/// @nodoc


class _FiatRates implements FiatRates {
  const _FiatRates({required this.timestampMs, required final  List<FiatRate> rates}): _rates = rates;
  

@override final  int timestampMs;
 final  List<FiatRate> _rates;
@override List<FiatRate> get rates {
  if (_rates is EqualUnmodifiableListView) return _rates;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_rates);
}





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _FiatRates&&(identical(other.timestampMs, timestampMs) || other.timestampMs == timestampMs)&&const DeepCollectionEquality().equals(other._rates, _rates));
}


@override
int get hashCode => Object.hash(runtimeType,timestampMs,const DeepCollectionEquality().hash(_rates));

@override
String toString() {
  return 'FiatRates(timestampMs: $timestampMs, rates: $rates)';
}


}




/// @nodoc
mixin _$ListChannelsResponse {

 List<LxChannelDetails> get channels;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ListChannelsResponse&&const DeepCollectionEquality().equals(other.channels, channels));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(channels));

@override
String toString() {
  return 'ListChannelsResponse(channels: $channels)';
}


}





/// @nodoc


class _ListChannelsResponse implements ListChannelsResponse {
  const _ListChannelsResponse({required final  List<LxChannelDetails> channels}): _channels = channels;
  

 final  List<LxChannelDetails> _channels;
@override List<LxChannelDetails> get channels {
  if (_channels is EqualUnmodifiableListView) return _channels;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_channels);
}





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _ListChannelsResponse&&const DeepCollectionEquality().equals(other._channels, _channels));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_channels));

@override
String toString() {
  return 'ListChannelsResponse(channels: $channels)';
}


}




/// @nodoc
mixin _$NodeInfo {

 String get nodePk; String get version; String get measurement; Balance get balance;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is NodeInfo&&(identical(other.nodePk, nodePk) || other.nodePk == nodePk)&&(identical(other.version, version) || other.version == version)&&(identical(other.measurement, measurement) || other.measurement == measurement)&&(identical(other.balance, balance) || other.balance == balance));
}


@override
int get hashCode => Object.hash(runtimeType,nodePk,version,measurement,balance);

@override
String toString() {
  return 'NodeInfo(nodePk: $nodePk, version: $version, measurement: $measurement, balance: $balance)';
}


}





/// @nodoc


class _NodeInfo implements NodeInfo {
  const _NodeInfo({required this.nodePk, required this.version, required this.measurement, required this.balance});
  

@override final  String nodePk;
@override final  String version;
@override final  String measurement;
@override final  Balance balance;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _NodeInfo&&(identical(other.nodePk, nodePk) || other.nodePk == nodePk)&&(identical(other.version, version) || other.version == version)&&(identical(other.measurement, measurement) || other.measurement == measurement)&&(identical(other.balance, balance) || other.balance == balance));
}


@override
int get hashCode => Object.hash(runtimeType,nodePk,version,measurement,balance);

@override
String toString() {
  return 'NodeInfo(nodePk: $nodePk, version: $version, measurement: $measurement, balance: $balance)';
}


}




/// @nodoc
mixin _$OpenChannelRequest {

 UserChannelId get userChannelId; int get valueSats;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is OpenChannelRequest&&(identical(other.userChannelId, userChannelId) || other.userChannelId == userChannelId)&&(identical(other.valueSats, valueSats) || other.valueSats == valueSats));
}


@override
int get hashCode => Object.hash(runtimeType,userChannelId,valueSats);

@override
String toString() {
  return 'OpenChannelRequest(userChannelId: $userChannelId, valueSats: $valueSats)';
}


}





/// @nodoc


class _OpenChannelRequest implements OpenChannelRequest {
  const _OpenChannelRequest({required this.userChannelId, required this.valueSats});
  

@override final  UserChannelId userChannelId;
@override final  int valueSats;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _OpenChannelRequest&&(identical(other.userChannelId, userChannelId) || other.userChannelId == userChannelId)&&(identical(other.valueSats, valueSats) || other.valueSats == valueSats));
}


@override
int get hashCode => Object.hash(runtimeType,userChannelId,valueSats);

@override
String toString() {
  return 'OpenChannelRequest(userChannelId: $userChannelId, valueSats: $valueSats)';
}


}




/// @nodoc
mixin _$OpenChannelResponse {

 String get channelId;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is OpenChannelResponse&&(identical(other.channelId, channelId) || other.channelId == channelId));
}


@override
int get hashCode => Object.hash(runtimeType,channelId);

@override
String toString() {
  return 'OpenChannelResponse(channelId: $channelId)';
}


}





/// @nodoc


class _OpenChannelResponse implements OpenChannelResponse {
  const _OpenChannelResponse({required this.channelId});
  

@override final  String channelId;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _OpenChannelResponse&&(identical(other.channelId, channelId) || other.channelId == channelId));
}


@override
int get hashCode => Object.hash(runtimeType,channelId);

@override
String toString() {
  return 'OpenChannelResponse(channelId: $channelId)';
}


}




/// @nodoc
mixin _$PayInvoiceRequest {

 String get invoice; int? get fallbackAmountSats; String? get note;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PayInvoiceRequest&&(identical(other.invoice, invoice) || other.invoice == invoice)&&(identical(other.fallbackAmountSats, fallbackAmountSats) || other.fallbackAmountSats == fallbackAmountSats)&&(identical(other.note, note) || other.note == note));
}


@override
int get hashCode => Object.hash(runtimeType,invoice,fallbackAmountSats,note);

@override
String toString() {
  return 'PayInvoiceRequest(invoice: $invoice, fallbackAmountSats: $fallbackAmountSats, note: $note)';
}


}





/// @nodoc


class _PayInvoiceRequest implements PayInvoiceRequest {
  const _PayInvoiceRequest({required this.invoice, this.fallbackAmountSats, this.note});
  

@override final  String invoice;
@override final  int? fallbackAmountSats;
@override final  String? note;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PayInvoiceRequest&&(identical(other.invoice, invoice) || other.invoice == invoice)&&(identical(other.fallbackAmountSats, fallbackAmountSats) || other.fallbackAmountSats == fallbackAmountSats)&&(identical(other.note, note) || other.note == note));
}


@override
int get hashCode => Object.hash(runtimeType,invoice,fallbackAmountSats,note);

@override
String toString() {
  return 'PayInvoiceRequest(invoice: $invoice, fallbackAmountSats: $fallbackAmountSats, note: $note)';
}


}




/// @nodoc
mixin _$PayInvoiceResponse {

 PaymentIndex get index;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PayInvoiceResponse&&(identical(other.index, index) || other.index == index));
}


@override
int get hashCode => Object.hash(runtimeType,index);

@override
String toString() {
  return 'PayInvoiceResponse(index: $index)';
}


}





/// @nodoc


class _PayInvoiceResponse implements PayInvoiceResponse {
  const _PayInvoiceResponse({required this.index});
  

@override final  PaymentIndex index;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PayInvoiceResponse&&(identical(other.index, index) || other.index == index));
}


@override
int get hashCode => Object.hash(runtimeType,index);

@override
String toString() {
  return 'PayInvoiceResponse(index: $index)';
}


}




/// @nodoc
mixin _$PayOfferRequest {

 ClientPaymentId get cid; String get offer; int? get fallbackAmountSats; String? get note;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PayOfferRequest&&(identical(other.cid, cid) || other.cid == cid)&&(identical(other.offer, offer) || other.offer == offer)&&(identical(other.fallbackAmountSats, fallbackAmountSats) || other.fallbackAmountSats == fallbackAmountSats)&&(identical(other.note, note) || other.note == note));
}


@override
int get hashCode => Object.hash(runtimeType,cid,offer,fallbackAmountSats,note);

@override
String toString() {
  return 'PayOfferRequest(cid: $cid, offer: $offer, fallbackAmountSats: $fallbackAmountSats, note: $note)';
}


}





/// @nodoc


class _PayOfferRequest implements PayOfferRequest {
  const _PayOfferRequest({required this.cid, required this.offer, this.fallbackAmountSats, this.note});
  

@override final  ClientPaymentId cid;
@override final  String offer;
@override final  int? fallbackAmountSats;
@override final  String? note;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PayOfferRequest&&(identical(other.cid, cid) || other.cid == cid)&&(identical(other.offer, offer) || other.offer == offer)&&(identical(other.fallbackAmountSats, fallbackAmountSats) || other.fallbackAmountSats == fallbackAmountSats)&&(identical(other.note, note) || other.note == note));
}


@override
int get hashCode => Object.hash(runtimeType,cid,offer,fallbackAmountSats,note);

@override
String toString() {
  return 'PayOfferRequest(cid: $cid, offer: $offer, fallbackAmountSats: $fallbackAmountSats, note: $note)';
}


}




/// @nodoc
mixin _$PayOfferResponse {

 PaymentIndex get index;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PayOfferResponse&&(identical(other.index, index) || other.index == index));
}


@override
int get hashCode => Object.hash(runtimeType,index);

@override
String toString() {
  return 'PayOfferResponse(index: $index)';
}


}





/// @nodoc


class _PayOfferResponse implements PayOfferResponse {
  const _PayOfferResponse({required this.index});
  

@override final  PaymentIndex index;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PayOfferResponse&&(identical(other.index, index) || other.index == index));
}


@override
int get hashCode => Object.hash(runtimeType,index);

@override
String toString() {
  return 'PayOfferResponse(index: $index)';
}


}




/// @nodoc
mixin _$PayOnchainRequest {

 ClientPaymentId get cid; String get address; int get amountSats; ConfirmationPriority get priority; String? get note;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PayOnchainRequest&&(identical(other.cid, cid) || other.cid == cid)&&(identical(other.address, address) || other.address == address)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.priority, priority) || other.priority == priority)&&(identical(other.note, note) || other.note == note));
}


@override
int get hashCode => Object.hash(runtimeType,cid,address,amountSats,priority,note);

@override
String toString() {
  return 'PayOnchainRequest(cid: $cid, address: $address, amountSats: $amountSats, priority: $priority, note: $note)';
}


}





/// @nodoc


class _PayOnchainRequest implements PayOnchainRequest {
  const _PayOnchainRequest({required this.cid, required this.address, required this.amountSats, required this.priority, this.note});
  

@override final  ClientPaymentId cid;
@override final  String address;
@override final  int amountSats;
@override final  ConfirmationPriority priority;
@override final  String? note;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PayOnchainRequest&&(identical(other.cid, cid) || other.cid == cid)&&(identical(other.address, address) || other.address == address)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.priority, priority) || other.priority == priority)&&(identical(other.note, note) || other.note == note));
}


@override
int get hashCode => Object.hash(runtimeType,cid,address,amountSats,priority,note);

@override
String toString() {
  return 'PayOnchainRequest(cid: $cid, address: $address, amountSats: $amountSats, priority: $priority, note: $note)';
}


}




/// @nodoc
mixin _$PayOnchainResponse {

 PaymentIndex get index; String get txid;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PayOnchainResponse&&(identical(other.index, index) || other.index == index)&&(identical(other.txid, txid) || other.txid == txid));
}


@override
int get hashCode => Object.hash(runtimeType,index,txid);

@override
String toString() {
  return 'PayOnchainResponse(index: $index, txid: $txid)';
}


}





/// @nodoc


class _PayOnchainResponse implements PayOnchainResponse {
  const _PayOnchainResponse({required this.index, required this.txid});
  

@override final  PaymentIndex index;
@override final  String txid;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PayOnchainResponse&&(identical(other.index, index) || other.index == index)&&(identical(other.txid, txid) || other.txid == txid));
}


@override
int get hashCode => Object.hash(runtimeType,index,txid);

@override
String toString() {
  return 'PayOnchainResponse(index: $index, txid: $txid)';
}


}




/// @nodoc
mixin _$PreflightCloseChannelResponse {

 int get feeEstimateSats;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PreflightCloseChannelResponse&&(identical(other.feeEstimateSats, feeEstimateSats) || other.feeEstimateSats == feeEstimateSats));
}


@override
int get hashCode => Object.hash(runtimeType,feeEstimateSats);

@override
String toString() {
  return 'PreflightCloseChannelResponse(feeEstimateSats: $feeEstimateSats)';
}


}





/// @nodoc


class _PreflightCloseChannelResponse implements PreflightCloseChannelResponse {
  const _PreflightCloseChannelResponse({required this.feeEstimateSats});
  

@override final  int feeEstimateSats;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PreflightCloseChannelResponse&&(identical(other.feeEstimateSats, feeEstimateSats) || other.feeEstimateSats == feeEstimateSats));
}


@override
int get hashCode => Object.hash(runtimeType,feeEstimateSats);

@override
String toString() {
  return 'PreflightCloseChannelResponse(feeEstimateSats: $feeEstimateSats)';
}


}




/// @nodoc
mixin _$PreflightOpenChannelRequest {

 int get valueSats;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PreflightOpenChannelRequest&&(identical(other.valueSats, valueSats) || other.valueSats == valueSats));
}


@override
int get hashCode => Object.hash(runtimeType,valueSats);

@override
String toString() {
  return 'PreflightOpenChannelRequest(valueSats: $valueSats)';
}


}





/// @nodoc


class _PreflightOpenChannelRequest implements PreflightOpenChannelRequest {
  const _PreflightOpenChannelRequest({required this.valueSats});
  

@override final  int valueSats;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PreflightOpenChannelRequest&&(identical(other.valueSats, valueSats) || other.valueSats == valueSats));
}


@override
int get hashCode => Object.hash(runtimeType,valueSats);

@override
String toString() {
  return 'PreflightOpenChannelRequest(valueSats: $valueSats)';
}


}




/// @nodoc
mixin _$PreflightOpenChannelResponse {

 int get feeEstimateSats;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PreflightOpenChannelResponse&&(identical(other.feeEstimateSats, feeEstimateSats) || other.feeEstimateSats == feeEstimateSats));
}


@override
int get hashCode => Object.hash(runtimeType,feeEstimateSats);

@override
String toString() {
  return 'PreflightOpenChannelResponse(feeEstimateSats: $feeEstimateSats)';
}


}





/// @nodoc


class _PreflightOpenChannelResponse implements PreflightOpenChannelResponse {
  const _PreflightOpenChannelResponse({required this.feeEstimateSats});
  

@override final  int feeEstimateSats;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PreflightOpenChannelResponse&&(identical(other.feeEstimateSats, feeEstimateSats) || other.feeEstimateSats == feeEstimateSats));
}


@override
int get hashCode => Object.hash(runtimeType,feeEstimateSats);

@override
String toString() {
  return 'PreflightOpenChannelResponse(feeEstimateSats: $feeEstimateSats)';
}


}




/// @nodoc
mixin _$PreflightPayInvoiceRequest {

 String get invoice; int? get fallbackAmountSats;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PreflightPayInvoiceRequest&&(identical(other.invoice, invoice) || other.invoice == invoice)&&(identical(other.fallbackAmountSats, fallbackAmountSats) || other.fallbackAmountSats == fallbackAmountSats));
}


@override
int get hashCode => Object.hash(runtimeType,invoice,fallbackAmountSats);

@override
String toString() {
  return 'PreflightPayInvoiceRequest(invoice: $invoice, fallbackAmountSats: $fallbackAmountSats)';
}


}





/// @nodoc


class _PreflightPayInvoiceRequest implements PreflightPayInvoiceRequest {
  const _PreflightPayInvoiceRequest({required this.invoice, this.fallbackAmountSats});
  

@override final  String invoice;
@override final  int? fallbackAmountSats;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PreflightPayInvoiceRequest&&(identical(other.invoice, invoice) || other.invoice == invoice)&&(identical(other.fallbackAmountSats, fallbackAmountSats) || other.fallbackAmountSats == fallbackAmountSats));
}


@override
int get hashCode => Object.hash(runtimeType,invoice,fallbackAmountSats);

@override
String toString() {
  return 'PreflightPayInvoiceRequest(invoice: $invoice, fallbackAmountSats: $fallbackAmountSats)';
}


}




/// @nodoc
mixin _$PreflightPayInvoiceResponse {

 int get amountSats; int get feesSats;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PreflightPayInvoiceResponse&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.feesSats, feesSats) || other.feesSats == feesSats));
}


@override
int get hashCode => Object.hash(runtimeType,amountSats,feesSats);

@override
String toString() {
  return 'PreflightPayInvoiceResponse(amountSats: $amountSats, feesSats: $feesSats)';
}


}





/// @nodoc


class _PreflightPayInvoiceResponse implements PreflightPayInvoiceResponse {
  const _PreflightPayInvoiceResponse({required this.amountSats, required this.feesSats});
  

@override final  int amountSats;
@override final  int feesSats;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PreflightPayInvoiceResponse&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.feesSats, feesSats) || other.feesSats == feesSats));
}


@override
int get hashCode => Object.hash(runtimeType,amountSats,feesSats);

@override
String toString() {
  return 'PreflightPayInvoiceResponse(amountSats: $amountSats, feesSats: $feesSats)';
}


}




/// @nodoc
mixin _$PreflightPayOfferRequest {

 ClientPaymentId get cid; String get offer; int? get fallbackAmountSats;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PreflightPayOfferRequest&&(identical(other.cid, cid) || other.cid == cid)&&(identical(other.offer, offer) || other.offer == offer)&&(identical(other.fallbackAmountSats, fallbackAmountSats) || other.fallbackAmountSats == fallbackAmountSats));
}


@override
int get hashCode => Object.hash(runtimeType,cid,offer,fallbackAmountSats);

@override
String toString() {
  return 'PreflightPayOfferRequest(cid: $cid, offer: $offer, fallbackAmountSats: $fallbackAmountSats)';
}


}





/// @nodoc


class _PreflightPayOfferRequest implements PreflightPayOfferRequest {
  const _PreflightPayOfferRequest({required this.cid, required this.offer, this.fallbackAmountSats});
  

@override final  ClientPaymentId cid;
@override final  String offer;
@override final  int? fallbackAmountSats;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PreflightPayOfferRequest&&(identical(other.cid, cid) || other.cid == cid)&&(identical(other.offer, offer) || other.offer == offer)&&(identical(other.fallbackAmountSats, fallbackAmountSats) || other.fallbackAmountSats == fallbackAmountSats));
}


@override
int get hashCode => Object.hash(runtimeType,cid,offer,fallbackAmountSats);

@override
String toString() {
  return 'PreflightPayOfferRequest(cid: $cid, offer: $offer, fallbackAmountSats: $fallbackAmountSats)';
}


}




/// @nodoc
mixin _$PreflightPayOfferResponse {

 int get amountSats; int get feesSats;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PreflightPayOfferResponse&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.feesSats, feesSats) || other.feesSats == feesSats));
}


@override
int get hashCode => Object.hash(runtimeType,amountSats,feesSats);

@override
String toString() {
  return 'PreflightPayOfferResponse(amountSats: $amountSats, feesSats: $feesSats)';
}


}





/// @nodoc


class _PreflightPayOfferResponse implements PreflightPayOfferResponse {
  const _PreflightPayOfferResponse({required this.amountSats, required this.feesSats});
  

@override final  int amountSats;
@override final  int feesSats;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PreflightPayOfferResponse&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats)&&(identical(other.feesSats, feesSats) || other.feesSats == feesSats));
}


@override
int get hashCode => Object.hash(runtimeType,amountSats,feesSats);

@override
String toString() {
  return 'PreflightPayOfferResponse(amountSats: $amountSats, feesSats: $feesSats)';
}


}




/// @nodoc
mixin _$PreflightPayOnchainRequest {

 String get address; int get amountSats;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PreflightPayOnchainRequest&&(identical(other.address, address) || other.address == address)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats));
}


@override
int get hashCode => Object.hash(runtimeType,address,amountSats);

@override
String toString() {
  return 'PreflightPayOnchainRequest(address: $address, amountSats: $amountSats)';
}


}





/// @nodoc


class _PreflightPayOnchainRequest implements PreflightPayOnchainRequest {
  const _PreflightPayOnchainRequest({required this.address, required this.amountSats});
  

@override final  String address;
@override final  int amountSats;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PreflightPayOnchainRequest&&(identical(other.address, address) || other.address == address)&&(identical(other.amountSats, amountSats) || other.amountSats == amountSats));
}


@override
int get hashCode => Object.hash(runtimeType,address,amountSats);

@override
String toString() {
  return 'PreflightPayOnchainRequest(address: $address, amountSats: $amountSats)';
}


}




/// @nodoc
mixin _$PreflightPayOnchainResponse {

 FeeEstimate? get high; FeeEstimate get normal; FeeEstimate get background;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PreflightPayOnchainResponse&&(identical(other.high, high) || other.high == high)&&(identical(other.normal, normal) || other.normal == normal)&&(identical(other.background, background) || other.background == background));
}


@override
int get hashCode => Object.hash(runtimeType,high,normal,background);

@override
String toString() {
  return 'PreflightPayOnchainResponse(high: $high, normal: $normal, background: $background)';
}


}





/// @nodoc


class _PreflightPayOnchainResponse implements PreflightPayOnchainResponse {
  const _PreflightPayOnchainResponse({this.high, required this.normal, required this.background});
  

@override final  FeeEstimate? high;
@override final  FeeEstimate normal;
@override final  FeeEstimate background;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _PreflightPayOnchainResponse&&(identical(other.high, high) || other.high == high)&&(identical(other.normal, normal) || other.normal == normal)&&(identical(other.background, background) || other.background == background));
}


@override
int get hashCode => Object.hash(runtimeType,high,normal,background);

@override
String toString() {
  return 'PreflightPayOnchainResponse(high: $high, normal: $normal, background: $background)';
}


}




/// @nodoc
mixin _$UpdateClientRequest {

 String get pubkey; bool? get isRevoked;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is UpdateClientRequest&&(identical(other.pubkey, pubkey) || other.pubkey == pubkey)&&(identical(other.isRevoked, isRevoked) || other.isRevoked == isRevoked));
}


@override
int get hashCode => Object.hash(runtimeType,pubkey,isRevoked);

@override
String toString() {
  return 'UpdateClientRequest(pubkey: $pubkey, isRevoked: $isRevoked)';
}


}





/// @nodoc


class _UpdateClientRequest implements UpdateClientRequest {
  const _UpdateClientRequest({required this.pubkey, this.isRevoked});
  

@override final  String pubkey;
@override final  bool? isRevoked;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _UpdateClientRequest&&(identical(other.pubkey, pubkey) || other.pubkey == pubkey)&&(identical(other.isRevoked, isRevoked) || other.isRevoked == isRevoked));
}


@override
int get hashCode => Object.hash(runtimeType,pubkey,isRevoked);

@override
String toString() {
  return 'UpdateClientRequest(pubkey: $pubkey, isRevoked: $isRevoked)';
}


}




/// @nodoc
mixin _$UpdatePaymentNote {

 PaymentIndex get index; String? get note;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is UpdatePaymentNote&&(identical(other.index, index) || other.index == index)&&(identical(other.note, note) || other.note == note));
}


@override
int get hashCode => Object.hash(runtimeType,index,note);

@override
String toString() {
  return 'UpdatePaymentNote(index: $index, note: $note)';
}


}





/// @nodoc


class _UpdatePaymentNote implements UpdatePaymentNote {
  const _UpdatePaymentNote({required this.index, this.note});
  

@override final  PaymentIndex index;
@override final  String? note;




@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _UpdatePaymentNote&&(identical(other.index, index) || other.index == index)&&(identical(other.note, note) || other.note == note));
}


@override
int get hashCode => Object.hash(runtimeType,index,note);

@override
String toString() {
  return 'UpdatePaymentNote(index: $index, note: $note)';
}


}




// dart format on
