/// Extension methods on Dart/Rust FFI types.
library;

import 'package:lexeapp/bindings.dart' show api;

import 'package:lexeapp/bindings_generated_api.dart'
    show
        ClientPaymentId,
        Invoice,
        Payment,
        PaymentDirection,
        PaymentKind,
        PaymentMethod,
        PaymentMethod_Invoice,
        PaymentMethod_Offer,
        PaymentMethod_Onchain,
        PaymentStatus,
        ShortPayment;

extension PaymentExt on Payment {
  ShortPayment intoShort() => ShortPayment(
        index: this.index,
        kind: this.kind,
        direction: this.direction,
        amountSat: this.amountSat,
        status: this.status,
        note: this.note,
        createdAt: this.createdAt,
      );

  Payment copyWith({
    String? index,
    PaymentKind? kind,
    PaymentDirection? direction,
    Invoice? invoice,
    String? replacement,
    int? amountSat,
    int? feesSat,
    PaymentStatus? status,
    String? statusStr,
    String? note,
    int? createdAt,
    int? finalizedAt,
  }) =>
      Payment(
        index: index ?? this.index,
        kind: kind ?? this.kind,
        direction: direction ?? this.direction,
        invoice: invoice ?? this.invoice,
        replacement: replacement ?? this.replacement,
        amountSat: amountSat ?? this.amountSat,
        feesSat: feesSat ?? this.feesSat,
        status: status ?? this.status,
        statusStr: statusStr ?? this.statusStr,
        note: note ?? this.note,
        createdAt: createdAt ?? this.createdAt,
        finalizedAt: finalizedAt ?? this.finalizedAt,
      );

  bool isPending() => this.status == PaymentStatus.Pending;
  bool isPendingNotJunk() => this.isPending() && !this.isJunk();
  bool isFinalized() => this.status != PaymentStatus.Pending;
  bool isFinalizedNotJunk() => this.isFinalized() && !this.isJunk();

  // Keep in sync with [`BasicPayment::is_junk()`] in `common/src/ln/payments.rs`.
  bool isJunk() =>
      // junk amountless invoice
      this.status != PaymentStatus.Completed &&
      this.kind == PaymentKind.Invoice &&
      this.direction == PaymentDirection.Inbound &&
      (this.amountSat == null || this.note == null);
}

extension ClientPaymentIdExt on ClientPaymentId {
  static ClientPaymentId generate() => api.genClientPaymentId();
}

extension PaymentMethodExt on PaymentMethod {
  /// Return the payment method amount in satoshis, if any.
  int? amountSats() => switch (this) {
        PaymentMethod_Onchain(:final field0) => field0.amountSats,
        PaymentMethod_Invoice(:final field0) => field0.amountSats,
        PaymentMethod_Offer() =>
          throw UnsupportedError("BOLT12 offers not supported yet"),
      };
}
