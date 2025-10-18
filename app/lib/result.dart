//! A port of Rust's `Result` type to dart.

// ignore_for_file: nullable_type_in_catch_clause, only_throw_errors

import 'package:app_rs_dart/frb.dart' show AnyhowException;
import 'package:flutter/foundation.dart' show immutable;

/// [Result]s from the Rust FFI layer.
typedef FfiResult<T> = Result<T, FfiError>;

@immutable
sealed class Result<T, E> {
  const Result();

  T? get ok;
  E? get err;

  bool get isOk;
  bool get isErr;

  T unwrap();
  E unwrapErr();

  Result<U, E> map<U>(final U Function(T) fn);

  Result<T, F> mapErr<F>(final F Function(E) fn);

  Result<U, E> andThen<U>(final Result<U, E> Function(T) fn);

  Result<T, E> inspect(final void Function(T) fn);
  Result<T, E> inspectErr(final void Function(E) fn);

  /// Wrap `fn()` in a try/catch.
  factory Result.try_(final T Function() fn) {
    try {
      return Ok(fn());
    } on E catch (err) {
      return Err(err);
    }
  }

  /// Wrap an async `fn()` in a try/catch.
  static Future<Result<T, E>> tryAsync<T, E>(
    final Future<T> Function() fn,
  ) async {
    try {
      return Ok(await fn());
    } on E catch (err) {
      return Err(err);
    }
  }

  /// Convenience for `Result.try_` but specialized for calling the Rust ffi.
  static FfiResult<T> tryFfi<T>(final T Function() fn) =>
      Result<T, AnyhowException>.try_(fn).mapErr(FfiError.fromFfi);

  /// Convenience for `Result.tryAsync` but specialized for calling the Rust
  /// ffi.
  static Future<FfiResult<T>> tryFfiAsync<T>(
    final Future<T> Function() fn,
  ) async {
    final res = await Result.tryAsync<T, AnyhowException>(fn);
    return res.mapErr(FfiError.fromFfi);
  }
}

@immutable
final class Ok<T, E> extends Result<T, E> {
  const Ok(this.ok);

  @override
  final T ok;

  @override
  E? get err => null;

  @override
  bool get isOk => true;

  @override
  bool get isErr => false;

  @override
  T unwrap() => this.ok;

  @override
  E unwrapErr() =>
      throw Exception("called Result.unwrapErr on an Ok value: ${this.ok}");

  @override
  Result<U, E> map<U>(final U Function(T) fn) => Ok(fn(this.ok));

  @override
  Result<T, F> mapErr<F>(final F Function(E) fn) => Ok(this.ok);

  @override
  Result<U, E> andThen<U>(final Result<U, E> Function(T) fn) => fn(this.ok);

  @override
  Result<T, E> inspect(final void Function(T) fn) {
    fn(this.ok);
    return this;
  }

  @override
  Result<T, E> inspectErr(final void Function(E) fn) => this;

  @override
  String toString() {
    return "Ok(${this.ok})";
  }

  @override
  bool operator ==(Object other) {
    if (identical(this, other)) return true;

    return other is Ok && this.ok == other.ok;
  }

  @override
  int get hashCode => this.ok.hashCode;
}

@immutable
final class Err<T, E> extends Result<T, E> {
  const Err(this.err);

  @override
  final E err;

  @override
  T? get ok => null;

  @override
  bool get isOk => false;

  @override
  bool get isErr => true;

  @override
  T unwrap() {
    switch (this.err) {
      case Exception err:
        throw err;
      default:
        throw Exception("called Result.unwrap on an Err value: ${this.err}");
    }
  }

  @override
  E unwrapErr() => this.err;

  @override
  Result<U, E> map<U>(final U Function(T) fn) => Err(this.err);

  @override
  Result<T, F> mapErr<F>(final F Function(E) fn) => Err(fn(this.err));

  @override
  Result<U, E> andThen<U>(final Result<U, E> Function(T) fn) => Err(this.err);

  @override
  Result<T, E> inspect(final void Function(T) fn) => this;

  @override
  Result<T, E> inspectErr(final void Function(E) fn) {
    fn(this.err);
    return this;
  }

  @override
  String toString() {
    return "Err(${this.err})";
  }

  @override
  bool operator ==(Object other) {
    if (identical(this, other)) return true;

    return other is Err && this.err == other.err;
  }

  @override
  int get hashCode => this.err.hashCode;
}

/// Used to extract only `anyhow::Result`s from Rust FFI layer exceptions.
/// Rust panics are _not_ caught. Panics are not for control flow and are not
/// meant to be recoverable.
final class FfiError implements Exception {
  const FfiError(this.message);

  FfiError.fromFfi(final AnyhowException err) : this(err.message);

  AnyhowException toFfi() => AnyhowException(this.message);

  final String message;

  @override
  String toString() => this.message;
}

final class MessageException implements Exception {
  const MessageException(this.message);

  final String message;

  @override
  String toString() => this.message;
}
