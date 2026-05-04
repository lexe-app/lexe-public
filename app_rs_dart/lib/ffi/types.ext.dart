/// Extension methods on Dart/Rust FFI types.
library;

import 'package:app_rs_dart/ffi/types.dart'
    show
        Invoice,
        Offer,
        Payment,
        PaymentCreatedIndex,
        PaymentDirection,
        PaymentKind,
        PaymentKind_Invoice,
        PaymentKind_Offer,
        PaymentKind_Onchain,
        PaymentKind_Spontaneous,
        PaymentKind_Unknown,
        PaymentKind_WaivedChannelFee,
        PaymentKind_WaivedLiquidityFee,
        PaymentMethod,
        PaymentMethod_Invoice,
        PaymentMethod_LnurlPayRequest,
        PaymentMethod_Offer,
        PaymentMethod_Onchain,
        PaymentStatus,
        ShortPayment;

//
// PaymentCreatedIndex
//

extension PaymentCreatedIndexExt on PaymentCreatedIndex {
  // HACK: parsing the serialized form like this is ugly af.
  String body() {
    final paymentCreatedIndex = this.field0;
    final splitIdx = paymentCreatedIndex.lastIndexOf('_');
    if (splitIdx < 0) {
      return paymentCreatedIndex;
    } else {
      return paymentCreatedIndex.substring(splitIdx + 1);
    }
  }
}

//
// Payment
//

extension PaymentExt on Payment {
  /// The total payment amount, inclusive of fees (`amount + fee`).
  int? get totalSats =>
      this.amountSats != null ? this.amountSats! + this.feesSats : null;

  ShortPayment intoShort() => ShortPayment(
    index: this.index,
    kind: this.kind,
    direction: this.direction,
    amountSats: this.amountSats,
    feesSats: this.feesSats,
    status: this.status,
    description: this.description,
    message: this.message,
    personalNote: this.personalNote,
    createdAt: this.createdAt,
  );

  Payment copyWith({
    PaymentCreatedIndex? index,
    PaymentKind? kind,
    PaymentDirection? direction,
    Invoice? invoice,
    String? offerId,
    Offer? offer,
    String? preimage,
    String? hash,
    String? txid,
    String? replacement,
    int? amountSats,
    int? feesSats,
    PaymentStatus? status,
    String? statusStr,
    String? description,
    String? payerName,
    String? message,
    String? personalNote,
    int? createdAt,
    int? finalizedAt,
  }) => Payment(
    index: index ?? this.index,
    kind: kind ?? this.kind,
    direction: direction ?? this.direction,
    invoice: invoice ?? this.invoice,
    offerId: offerId ?? this.offerId,
    offer: offer ?? this.offer,
    preimage: preimage ?? this.preimage,
    hash: hash ?? this.hash,
    txid: txid ?? this.txid,
    replacement: replacement ?? this.replacement,
    amountSats: amountSats ?? this.amountSats,
    feesSats: feesSats ?? this.feesSats,
    status: status ?? this.status,
    statusStr: statusStr ?? this.statusStr,
    description: description ?? this.description,
    payerName: payerName ?? this.payerName,
    message: message ?? this.message,
    personalNote: personalNote ?? this.personalNote,
    createdAt: createdAt ?? this.createdAt,
    finalizedAt: finalizedAt ?? this.finalizedAt,
  );

  bool isPending() => this.status == PaymentStatus.pending;
  bool isPendingNotJunk() => this.isPending() && !this.isJunk();
  bool isFinalized() => this.status != PaymentStatus.pending;
  bool isFinalizedNotJunk() => this.isFinalized() && !this.isJunk();

  // Keep in sync with [`BasicPaymentV1::is_junk()`] in
  // `lexe-common/src/ln/payments.rs`.
  bool isJunk() =>
      // junk amountless invoice
      this.status != PaymentStatus.completed &&
      this.kind is PaymentKind_Invoice &&
      this.direction == PaymentDirection.inbound &&
      (this.amountSats == null || this.noteOrDescription == null);

  /// Returns the user's personal note, invoice/offer description, or message.
  /// Precedence: personalNote > description > message.
  String? get noteOrDescription {
    final n = this.personalNote;
    if (n != null && n.isNotEmpty) return n;
    final d = this.description;
    if (d != null && d.isNotEmpty) return d;
    return this.message;
  }
}

//
// PaymentMethod
//

extension PaymentMethodExt on PaymentMethod {
  PaymentKind kind() => switch (this) {
    PaymentMethod_Onchain() => const PaymentKind_Onchain(),
    PaymentMethod_Invoice() => const PaymentKind_Invoice(),
    PaymentMethod_Offer() => const PaymentKind_Offer(),
    PaymentMethod_LnurlPayRequest() => const PaymentKind_Invoice(),
  };
}

extension PaymentKindExt on PaymentKind {
  bool isLightning() => switch (this) {
    PaymentKind_Onchain() => false,
    PaymentKind_Invoice() => true,
    PaymentKind_Spontaneous() => true,
    PaymentKind_Offer() => true,
    PaymentKind_WaivedChannelFee() ||
    PaymentKind_WaivedLiquidityFee() ||
    PaymentKind_Unknown() => false,
  };
}

//
// ShortPayment
//

extension ShortPaymentExt on ShortPayment {
  /// The total payment amount, inclusive of fees (`amount + fee`).
  int? get totalSats =>
      this.amountSats != null ? this.amountSats! + this.feesSats : null;

  /// Returns the user's personal note, invoice/offer description, or message.
  /// Precedence: personalNote > description > message.
  String? get noteOrDescription {
    final n = this.personalNote;
    if (n != null && n.isNotEmpty) return n;
    final d = this.description;
    if (d != null && d.isNotEmpty) return d;
    return this.message;
  }
}
