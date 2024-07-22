// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'settings.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

T _$identity<T>(T value) => value;

final _privateConstructorUsedError = UnsupportedError(
    'It seems like you constructed your class using `MyClass._()`. This constructor is only meant to be used by freezed and you are not supposed to need it nor use it.\nPlease check the documentation here for more information: https://github.com/rrousselGit/freezed#adding-getters-and-methods-to-our-models');

/// @nodoc
mixin _$Settings {
  String? get locale => throw _privateConstructorUsedError;
  String? get fiatCurrency => throw _privateConstructorUsedError;
}

/// @nodoc

class _$SettingsImpl implements _Settings {
  const _$SettingsImpl({this.locale, this.fiatCurrency});

  @override
  final String? locale;
  @override
  final String? fiatCurrency;

  @override
  String toString() {
    return 'Settings(locale: $locale, fiatCurrency: $fiatCurrency)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == runtimeType &&
            other is _$SettingsImpl &&
            (identical(other.locale, locale) || other.locale == locale) &&
            (identical(other.fiatCurrency, fiatCurrency) ||
                other.fiatCurrency == fiatCurrency));
  }

  @override
  int get hashCode => Object.hash(runtimeType, locale, fiatCurrency);
}

abstract class _Settings implements Settings {
  const factory _Settings({final String? locale, final String? fiatCurrency}) =
      _$SettingsImpl;

  @override
  String? get locale;
  @override
  String? get fiatCurrency;
}
