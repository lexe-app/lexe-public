//! A port of Rust's `Result` type to dart.

import 'package:meta/meta.dart' show immutable;

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
  T unwrap() => ok;

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
  String toString() {
    return "Ok(${this.ok})";
  }

  @override
  bool operator ==(dynamic other) {
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
  String toString() {
    return "Err(${this.err})";
  }

  @override
  bool operator ==(dynamic other) {
    if (identical(this, other)) return true;

    return other is Err && this.err == other.err;
  }

  @override
  int get hashCode => this.err.hashCode;
}
