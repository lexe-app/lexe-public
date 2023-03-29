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
mixin _$FiatRate {
  int get timestampMs => throw _privateConstructorUsedError;
  double get rate => throw _privateConstructorUsedError;
}

/// @nodoc

class _$_FiatRate implements _FiatRate {
  const _$_FiatRate({required this.timestampMs, required this.rate});

  @override
  final int timestampMs;
  @override
  final double rate;

  @override
  String toString() {
    return 'FiatRate(timestampMs: $timestampMs, rate: $rate)';
  }

  @override
  bool operator ==(dynamic other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$_FiatRate &&
            (identical(other.timestampMs, timestampMs) ||
                other.timestampMs == timestampMs) &&
            (identical(other.rate, rate) || other.rate == rate));
  }

  @override
  int get hashCode => Object.hash(runtimeType, timestampMs, rate);
}

abstract class _FiatRate implements FiatRate {
  const factory _FiatRate(
      {required final int timestampMs,
      required final double rate}) = _$_FiatRate;

  @override
  int get timestampMs;
  @override
  double get rate;
}

/// @nodoc
mixin _$NodeInfo {
  String get nodePk => throw _privateConstructorUsedError;
  int get localBalanceMsat => throw _privateConstructorUsedError;
}

/// @nodoc

class _$_NodeInfo implements _NodeInfo {
  const _$_NodeInfo({required this.nodePk, required this.localBalanceMsat});

  @override
  final String nodePk;
  @override
  final int localBalanceMsat;

  @override
  String toString() {
    return 'NodeInfo(nodePk: $nodePk, localBalanceMsat: $localBalanceMsat)';
  }

  @override
  bool operator ==(dynamic other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$_NodeInfo &&
            (identical(other.nodePk, nodePk) || other.nodePk == nodePk) &&
            (identical(other.localBalanceMsat, localBalanceMsat) ||
                other.localBalanceMsat == localBalanceMsat));
  }

  @override
  int get hashCode => Object.hash(runtimeType, nodePk, localBalanceMsat);
}

abstract class _NodeInfo implements NodeInfo {
  const factory _NodeInfo(
      {required final String nodePk,
      required final int localBalanceMsat}) = _$_NodeInfo;

  @override
  String get nodePk;
  @override
  int get localBalanceMsat;
}
