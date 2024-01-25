// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'bindings_generated_api.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

T _$identity<T>(T value) => value;

final _privateConstructorUsedError = UnsupportedError(
    'It seems like you constructed your class using `MyClass._()`. This constructor is only meant to be used by freezed and you are not supposed to need it nor use it.\nPlease check the documentation here for more information: https://github.com/rrousselGit/freezed#custom-getters-and-methods');

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
  bool operator ==(dynamic other) {
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
  bool operator ==(dynamic other) {
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
  bool operator ==(dynamic other) {
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
  bool operator ==(dynamic other) {
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
mixin _$NodeInfo {
  String get nodePk => throw _privateConstructorUsedError;
  String get version => throw _privateConstructorUsedError;
  String get measurement => throw _privateConstructorUsedError;
  int get spendableBalanceSats => throw _privateConstructorUsedError;
}

/// @nodoc

class _$NodeInfoImpl implements _NodeInfo {
  const _$NodeInfoImpl(
      {required this.nodePk,
      required this.version,
      required this.measurement,
      required this.spendableBalanceSats});

  @override
  final String nodePk;
  @override
  final String version;
  @override
  final String measurement;
  @override
  final int spendableBalanceSats;

  @override
  String toString() {
    return 'NodeInfo(nodePk: $nodePk, version: $version, measurement: $measurement, spendableBalanceSats: $spendableBalanceSats)';
  }

  @override
  bool operator ==(dynamic other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$NodeInfoImpl &&
            (identical(other.nodePk, nodePk) || other.nodePk == nodePk) &&
            (identical(other.version, version) || other.version == version) &&
            (identical(other.measurement, measurement) ||
                other.measurement == measurement) &&
            (identical(other.spendableBalanceSats, spendableBalanceSats) ||
                other.spendableBalanceSats == spendableBalanceSats));
  }

  @override
  int get hashCode => Object.hash(
      runtimeType, nodePk, version, measurement, spendableBalanceSats);
}

abstract class _NodeInfo implements NodeInfo {
  const factory _NodeInfo(
      {required final String nodePk,
      required final String version,
      required final String measurement,
      required final int spendableBalanceSats}) = _$NodeInfoImpl;

  @override
  String get nodePk;
  @override
  String get version;
  @override
  String get measurement;
  @override
  int get spendableBalanceSats;
}

/// @nodoc
mixin _$ShortPayment {
  String get index => throw _privateConstructorUsedError;
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
  final String index;
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
  bool operator ==(dynamic other) {
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
      {required final String index,
      required final PaymentKind kind,
      required final PaymentDirection direction,
      final int? amountSat,
      required final PaymentStatus status,
      final String? note,
      required final int createdAt}) = _$ShortPaymentImpl;

  @override
  String get index;
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
