/// Extension methods on Dart/Rust FFI types.
library;

import 'package:lexeapp/ffi/ffi.dart' show api;

import 'package:lexeapp/ffi/ffi_generated_api.dart'
    show
        Balance,
        ClientPaymentId,
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

//
// ClientPaymentId
//

extension ClientPaymentIdExt on ClientPaymentId {
  static ClientPaymentId generate() => api.genClientPaymentId();
}

//
// PaymentMethod
//

extension PaymentMethodExt on PaymentMethod {
  /// Return the payment method amount in satoshis, if any.
  int? amountSats() => switch (this) {
        PaymentMethod_Onchain(:final field0) => field0.amountSats,
        PaymentMethod_Invoice(:final field0) => field0.amountSats,
        PaymentMethod_Offer() =>
          throw UnsupportedError("BOLT12 offers not supported yet"),
      };

  PaymentKind kind() => switch (this) {
        PaymentMethod_Onchain() => PaymentKind.Onchain,
        PaymentMethod_Invoice() => PaymentKind.Invoice,
        // TODO(phlip9): impl BOLT12 offers
        PaymentMethod_Offer() => throw UnimplementedError(),
      };
}

//
// Balance
//

extension BalanceExt on Balance {
  int balanceByKind(final PaymentKind kind) => switch (kind) {
        PaymentKind.Onchain => this.onchainSats,
        PaymentKind.Invoice => this.lightningSats,
        PaymentKind.Spontaneous => this.lightningSats,
      };
}
