/// Extension methods on Dart/Rust FFI types.
library;

import 'package:lexeapp/bindings_generated_api.dart'
    show
        Invoice,
        Payment,
        PaymentDirection,
        PaymentKind,
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
