import 'package:flutter_test/flutter_test.dart' show expect, test;

import 'package:lexeapp/route/receive/state.dart'
    show PaymentOffer, PaymentOfferKind;

void main() {
  test("PaymentOffer.uri() (invoice)", () {
    const code =
        "lnbcrt4693500n1pjgld4pxq8pjglhd3pp5h038tqal0m3xjwrmht2gcj8u4cgwg9fh6d0ynv2ds8x8xph5sm9ssp5d4jx76ttd4ek76tnv3hkv6tpdfekgenvdfkx76t2wdskg6nxda5s9qrsgqdp4wdhk6efqdehhgefqw35x2grfdemx76trv5sxxun9v96x7u3qwdjhgcqpcnp4qgywe59xssrqj004k24477svqtgynw4am39hz06hk4dlu4l0ssk8w2rpkgvpsusjrwde5qym0t9g42px0dahyh7jz9lvn5umk9gzqxtc8r0rdplu9psdewwqnw6t7uvdqtvn6heqfgxvn9a76kkl760cy4rqpewlfe6";

    const payment = PaymentOffer(
      kind: PaymentOfferKind.lightningInvoice,
      code: code,
      amountSats: null,
      description: null,
      expiresAt: null,
    );

    expect(payment.uri()!.toString(), "lightning:$code");
  });

  test("PaymentOffer.uri() (address)", () {
    const code = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";

    const payment = PaymentOffer(
      kind: PaymentOfferKind.btcAddress,
      code: code,
      amountSats: null,
      description: null,
      expiresAt: null,
    );

    expect(payment.uri()!.toString(), "bitcoin:$code");
  });

  test("PaymentOffer.uri() (offer)", () {
    const code =
        "lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q";

    const payment = PaymentOffer(
      kind: PaymentOfferKind.lightningOffer,
      code: code,
      amountSats: null,
      description: null,
      expiresAt: null,
    );

    expect(payment.uri()!.toString(), "bitcoin:?lno=$code");
  });
}
