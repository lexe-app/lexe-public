/// Receive payment page models and state machines.
library;

import 'package:flutter/foundation.dart' show immutable;

/// The kind of payment to receive, across both BTC and LN.
enum PaymentOfferKind {
  lightningInvoice,
  lightningOffer,
  btcAddress,

  // TODO(phlip9): impl
  // lightningSpontaneous,
  // btcTaproot,
  ;
}

/// The Bitcoin address type to receive with.
enum BtcAddrKind {
  segwit,
  // TODO(phlip9): impl
  // taproot,
  ;

  PaymentOfferKind toOfferKind() => switch (this) {
        BtcAddrKind.segwit => PaymentOfferKind.btcAddress,
      };
}

/// The inputs used to generate a Lightning invoice [PaymentOffer].
@immutable
class LnInvoiceInputs {
  const LnInvoiceInputs({
    required this.amountSats,
    required this.description,
  });

  final int? amountSats;
  final String? description;

  @override
  String toString() {
    return "LnInvoiceInputs(amountSats: $amountSats, description: $description)";
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == this.runtimeType &&
            other is LnInvoiceInputs &&
            (identical(other.amountSats, this.amountSats) ||
                other.amountSats == this.amountSats) &&
            (identical(other.description, this.description) ||
                other.description == this.description));
  }

  @override
  int get hashCode =>
      Object.hash(this.runtimeType, this.amountSats, this.description);
}

/// The inputs used to generate a Lightning BOLT12 offer [PaymentOffer].
@immutable
class LnOfferInputs {
  const LnOfferInputs({
    required this.amountSats,
    required this.description,
  });

  final int? amountSats;
  final String? description;

  @override
  String toString() {
    return "LnOfferInputs(amountSats: $amountSats, description: $description)";
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == this.runtimeType &&
            other is LnOfferInputs &&
            (identical(other.amountSats, this.amountSats) ||
                other.amountSats == this.amountSats) &&
            (identical(other.description, this.description) ||
                other.description == this.description));
  }

  @override
  int get hashCode =>
      Object.hash(this.runtimeType, this.amountSats, this.description);
}

/// The inputs used to generate a Bitcoin address [PaymentOffer].
@immutable
class BtcAddrInputs {
  const BtcAddrInputs({required this.kind});

  final BtcAddrKind kind;

  @override
  String toString() {
    return "BitcoinAddressInputs(kind: $kind)";
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == this.runtimeType &&
            other is BtcAddrInputs &&
            (identical(other.kind, this.kind) || other.kind == this.kind));
  }

  @override
  int get hashCode => Object.hash(this.runtimeType, this.kind);
}

/// A generic PaymentOffer that we can display in a [PaymentOfferPage].
@immutable
class PaymentOffer {
  const PaymentOffer({
    required this.kind,
    required this.code,
    required this.amountSats,
    required this.description,
    required this.expiresAt,
  });

  /// An initial, unloaded PaymentOffer.
  const PaymentOffer.unloaded({required this.kind})
      : code = null,
        amountSats = null,
        description = null,
        expiresAt = null;

  final PaymentOfferKind kind;
  final String? code;
  final int? amountSats;
  final String? description;
  final DateTime? expiresAt;

  /// When refreshing a payment offer, we want to reset it back to the default
  /// unloaded state first. However, we also want to keep displaying the
  /// amount/description to avoid the UI reflowing and jumping around.
  PaymentOffer resetForRefresh() => PaymentOffer(
        kind: this.kind,
        code: null,
        amountSats: this.amountSats,
        description: this.description,
        expiresAt: null,
      );

  String titleStr() => switch (this.kind) {
        PaymentOfferKind.lightningInvoice => "Lightning invoice",
        PaymentOfferKind.lightningOffer => "Lightning offer",
        PaymentOfferKind.btcAddress => "Bitcoin address",

        // PaymentOfferKind.lightningSpontaneous => "Lightning spontaneous payment",
        // PaymentOfferKind.btcTaproot => "Bitcoin taproot address",
      };

  String subtitleStr() => switch (this.kind) {
        PaymentOfferKind.lightningInvoice =>
          "Receive Bitcoin instantly with Lightning",
        // PaymentOfferKind.lightningOffer => "Reusable Lightning payment code",
        // PaymentOfferKind.lightningOffer => "Reusable Lightning payment request",
        PaymentOfferKind.lightningOffer =>
          "Receive Bitcoin over Lightning many times with one reusable code",
        PaymentOfferKind.btcAddress =>
          "Receive Bitcoin from anywhere. Slower and more expensive than via Lightning.",

        // TODO(phlip9): impl
        // PaymentOfferKind.btcTaproot => "",
        // PaymentOfferKind.lightningSpontaneous => "",
      };

  String? warningStr() => switch (this.kind) {
        PaymentOfferKind.lightningInvoice =>
          "Invoices can only be paid once.\nReusing an invoice may result in lost payments.",
        PaymentOfferKind.lightningOffer =>
          "Lightning offers (BOLT12) are new and may not be supported by all wallets.",
        PaymentOfferKind.btcAddress => null,
      };

  // TODO(phlip9): do this in rust, more robustly. Also uppercase for QR
  // encoding.
  Uri? uri() {
    final code = this.code;
    if (code == null) return null;

    return switch (this.kind) {
      PaymentOfferKind.lightningInvoice => Uri(scheme: "lightning", path: code),
      PaymentOfferKind.lightningOffer => Uri(scheme: "lightning", path: code),
      PaymentOfferKind.btcAddress => Uri(scheme: "bitcoin", path: code),
    };
  }

  @override
  String toString() {
    return 'PaymentOffer(kind: $kind, code: $code, amountSats: $amountSats, description: $description, expiresAt: $expiresAt)';
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == this.runtimeType &&
            other is PaymentOffer &&
            (identical(other.kind, this.kind) || other.kind == this.kind) &&
            (identical(other.code, this.code) || other.code == this.code) &&
            (identical(other.amountSats, this.amountSats) ||
                other.amountSats == this.amountSats) &&
            (identical(other.description, this.description) ||
                other.description == this.description) &&
            (identical(other.expiresAt, this.expiresAt) ||
                other.expiresAt == this.expiresAt));
  }

  @override
  int get hashCode => Object.hash(this.runtimeType, this.kind, this.code,
      this.amountSats, this.description, this.expiresAt);
}
