// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'wallet.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

T _$identity<T>(T value) => value;

final _privateConstructorUsedError = UnsupportedError(
    'It seems like you constructed your class using `MyClass._()`. This constructor is only meant to be used by freezed and you are not supposed to need it nor use it.\nPlease check the documentation here for more information: https://github.com/rrousselGit/freezed#custom-getters-and-methods');

/// @nodoc
mixin _$BalanceState {
  int? get satsBalance => throw _privateConstructorUsedError;
  String get fiatName => throw _privateConstructorUsedError;
  FiatRate? get fiatRate => throw _privateConstructorUsedError;
}

/// @nodoc

class _$_BalanceState extends _BalanceState {
  const _$_BalanceState(
      {required this.satsBalance,
      required this.fiatName,
      required this.fiatRate})
      : super._();

  @override
  final int? satsBalance;
  @override
  final String fiatName;
  @override
  final FiatRate? fiatRate;

  @override
  String toString() {
    return 'BalanceState(satsBalance: $satsBalance, fiatName: $fiatName, fiatRate: $fiatRate)';
  }

  @override
  bool operator ==(dynamic other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$_BalanceState &&
            (identical(other.satsBalance, satsBalance) ||
                other.satsBalance == satsBalance) &&
            (identical(other.fiatName, fiatName) ||
                other.fiatName == fiatName) &&
            (identical(other.fiatRate, fiatRate) ||
                other.fiatRate == fiatRate));
  }

  @override
  int get hashCode => Object.hash(runtimeType, satsBalance, fiatName, fiatRate);
}

abstract class _BalanceState extends BalanceState {
  const factory _BalanceState(
      {required final int? satsBalance,
      required final String fiatName,
      required final FiatRate? fiatRate}) = _$_BalanceState;
  const _BalanceState._() : super._();

  @override
  int? get satsBalance;
  @override
  String get fiatName;
  @override
  FiatRate? get fiatRate;
}
