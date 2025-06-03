/// Extension methods on Dart/Rust FFI types.
library;

import 'package:app_rs_dart/ffi/types.dart'
    show
        Invoice,
        Payment,
        PaymentDirection,
        PaymentIndex,
        PaymentKind,
        PaymentMethod,
        PaymentMethod_Invoice,
        PaymentMethod_Offer,
        PaymentMethod_Onchain,
        PaymentStatus,
        ShortPayment;

//
// PaymentIndex
//

extension PaymentIndexExt on PaymentIndex {
  // HACK: parsing the serialized form like this is ugly af.
  String body() {
    final paymentIndex = this.field0;
    final splitIdx = paymentIndex.lastIndexOf('_');
    if (splitIdx < 0) {
      return paymentIndex;
    } else {
      return paymentIndex.substring(splitIdx + 1);
    }
  }
}

//
// Payment
//

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
    PaymentIndex? index,
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

  bool isPending() => this.status == PaymentStatus.pending;
  bool isPendingNotJunk() => this.isPending() && !this.isJunk();
  bool isFinalized() => this.status != PaymentStatus.pending;
  bool isFinalizedNotJunk() => this.isFinalized() && !this.isJunk();

  // Keep in sync with [`BasicPayment::is_junk()`] in `common/src/ln/payments.rs`.
  bool isJunk() =>
      // junk amountless invoice
      this.status != PaymentStatus.completed &&
      this.kind == PaymentKind.invoice &&
      this.direction == PaymentDirection.inbound &&
      (this.amountSat == null || this.note == null);
}

//
// PaymentMethod
//

extension PaymentMethodExt on PaymentMethod {
  /// Return the payment method amount in satoshis, if any.
  int? amountSats() => switch (this) {
        PaymentMethod_Onchain(:final field0) => field0.amountSats,
        PaymentMethod_Invoice(:final field0) => field0.amountSats,
        PaymentMethod_Offer(:final field0) => field0.amountSats,
      };

  PaymentKind kind() => switch (this) {
        PaymentMethod_Onchain() => PaymentKind.onchain,
        PaymentMethod_Invoice() => PaymentKind.invoice,
        PaymentMethod_Offer() => PaymentKind.offer,
      };
}

extension PaymentKindExt on PaymentKind {
  bool isLightning() => switch (this) {
        PaymentKind.onchain => false,
        PaymentKind.invoice => true,
        PaymentKind.spontaneous => true,
        PaymentKind.offer => true,
      };
}
