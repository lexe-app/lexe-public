# Lexe SDK sidecar API

## Overview

The Lexe SDK sidecar presents a simplified API for developers to interact with
their Lexe wallet. With Lexe, you can easily send and receive Bitcoin over the
Lightning network.

The Lexe SDK sidecar runs as a separate process or container alongside your
application and presents a simple HTTP API at `http://localhost:5393`.

## REST API

The Lexe SDK sidecar exposes the following REST API endpoints:

```
GET /v1/health
GET /v1/node/node_info
POST /v1/node/create_invoice
POST /v1/node/pay_invoice
GET /v1/node/payment
```

### Conventions

Payments are uniquely identified by their `index` value string. Fetch the status
of a payment using the `GET /v1/node/payment?index=<index>` endpoint.

`amount` values are fixed-precision decimal values denominated in satoshis
(1 BTC = 100,000,000 satoshis) and serialized as strings. The representation
supports up-to millisatoshi precision (1 satoshi = 1,000 millisatoshis).

Prefer longer request timeouts (e.g. 15 seconds) since your node may need time
to startup and sync if it hasn't received any requests in a while.

### `GET /v1/health`

Get the health status of the Lexe SDK sidecar. Returns HTTP 200 once the sidecar
is running and ready to accept requests.

**Examples:**

```bash
$ curl http://localhost:5393/v1/health
{ "status": "ok" }
```

### `GET /v1/node/node_info`

Fetch information about the node and wallet balance.

**Examples:**

```bash
$ curl http://localhost:5393/v1/node/node_info | jq .
{
  "version": "0.7.5",
  "measurement": "061e666a31d6aadfa1a70b4664632729ce4aa07814e7af8132d1d923380e8f45",
  "node_pk": "03d82ea1c737865c93bf7468b7f350203e71ac092af32c85e2d186eb007ced3ad6",
  "num_peers": 1,
  "num_usable_channels": 2,
  "num_channels": 2,
  "lightning_balance": {
    "usable": "100246",
    "sendable": "87613.268",
    "max_sendable": "87971.572",
    "pending": "0"
  },
  "onchain_balance": {
    "immature": 0,
    "trusted_pending": 0,
    "untrusted_pending": 0,
    "confirmed": 0
  },
  "pending_monitor_updates": 1
}
```

### `POST /v1/node/create_invoice`

Create a new BOLT11 Lightning invoice to receive Bitcoin over the Lightning
network.

**Request:**

The request body should be a JSON object with the following fields:

* `expiry_secs: Int`: The number of seconds until the invoice expires.
* `amount: String` (optional): The amount to request in satoshis, as a string.
  If not specified, the payer will decide the amount.
* `description: String` (optional): The payment description that will be
  presented to the payer.

**Response:**

The response includes the encoded `invoice` string, which should be presented to
the payer to complete the payment.

The `index` is a unique identifier for the invoice, which can be used to track
the payment status via `GET /v1/node/payment`.

**Examples:**

```bash
$ curl -X POST http://localhost:5393/v1/node/create_invoice \
    --header "content-type: application/json" \
    --data '{ "expiry_secs": 3600 }' \
    | jq .
{
  "index": "0000001744926519917-ln_9be5e4e3a0356cc4a7a1dce5a4af39e2896b7eb7b007ec6ca8c2f8434f21a63a",
  "invoice": "lnbc1p5qzaehdqqpp5n0j7fcaqx4kvffapmnj6fteeu2ykkl4hkqr7cm9gctuyxnep5caqcqpcsp5slzxgxrsu3jq8xq7rp2gx3ge0thlt3446jpp8kqs87pve60679ls9qyysgqxqrrssnp4q0vzagw8x7r9eyalw35t0u6syql8rtqf9tejep0z6xrwkqrua5advrzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wzmk4uqqj5sqqyqqqqqqqqqqhwqqfq89vuhjlg2tt56sv9pdt8t5cvdgfaaf6nxqtt0av74ragpql7l2d42euknlw06fcgp8xhe93xe7c802z3hrnysfsjgavmwfts7zdvj2cqka3672",
  "description": null,
  "amount": null,
  "created_at": 1744926519000,
  "expires_at": 1744930119000,
  "payment_hash": "9be5e4e3a0356cc4a7a1dce5a4af39e2896b7eb7b007ec6ca8c2f8434f21a63a",
  "payment_secret": "87c4641870e46403981e18548345197aeff5c6b5d48213d8103f82cce9faf17f"
}

$ curl -X POST http://localhost:5393/v1/node/create_invoice \
    --header "content-type: application/json" \
    --data '{ "expiry_secs": 3600, "amount": "1000", "description": "Lunch" }' \
    | jq .
{
  "index": "0000001744926580307-ln_12c8ec9465cff06b756b9f20dbdfd9d4b03b3c153bd39a5401c61a0241bd1e96",
  "invoice": "lnbc10u1p5qzam5dqgf36kucmgpp5ztywe9r9elcxkattnusdhh7e6jcrk0q480fe54qpccdqysdar6tqcqpcsp5f3nvkgufsxxnsfa4wnyzgjk3sjpxcwsp8zw4ck0mstcyrgpyu8ls9qyysgqxqrrssnp4q0vzagw8x7r9eyalw35t0u6syql8rtqf9tejep0z6xrwkqrua5advrzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wzmk4uqqj5sqqyqqqqqqqqqqhwqqfqdtsc32py445jyfcdwcnf25kwwh0ezvw0890xlpfjxtm4a9pcuyjpvd54alrze0tzxzl4cgm82q3deh7w66zsukuccrgzq59vpp28lvgp4jesmt",
  "description": "Lunch",
  "amount": "1000",
  "created_at": 1744926580000,
  "expires_at": 1744930180000,
  "payment_hash": "12c8ec9465cff06b756b9f20dbdfd9d4b03b3c153bd39a5401c61a0241bd1e96",
  "payment_secret": "4c66cb2389818d3827b574c8244ad184826c3a01389d5c59fb82f041a024e1ff"
}
```

### `POST /v1/node/pay_invoice`

Pay a BOLT11 Lightning invoice.

**Request:**

The request body should be a JSON object with the following fields:

* `invoice: String`: The encoded invoice string to pay.
* `fallback_amount: String` (optional): For invoices without an amount specified, you
  must specify a fallback amount to pay.
* `note: String` (optional): A personal note to attach to the payment.

**Response:**

The response includes the `index` of the payment, which can be used to track the
payment status via `GET /v1/node/payment`.

**Examples:**

```bash
$ curl -X POST http://localhost:5393/v1/node/pay_invoice \
    --header "content-type: application/json" \
    --data '{ "invoice": "lnbc100n1p5qz7z2dq58skjqnr90pjjq4r9wd6qpp5u8uw073l8dp7ked0ujyhegwxx6yxx6aq5ganqyt3pepnk5dm87dqcqpcsp5nrs44f3upgxysnylrrpyrxs96mgazjjstuykyew74zv0najzkdeq9qyysgqxqyz5vqnp4q0w73a6xytxxrhuuvqnqjckemyhv6avveuftl64zzm5878vq3zr4jrzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wz5ecsqq2pgqqcqqqqqqqqqqhwqqfqrpeeq5xdys8vcfcark45w992h6j5nhajc62wet0q25ggxjwhtcfn8c3qx30fqzq8mqxfdtks57zw25zp0z2kl9yrfwkkthxclawxpfcqtdcpfu" }' \
    | jq .
{
  "index": "0000001744926842458-ln_e1f8e7fa3f3b43eb65afe4897ca1c63688636ba0a23b3011710e433b51bb3f9a",
  "created_at": 1744926842458
}
```

### `GET /v1/node/payment`

Use this endpoint to query the status of a payment or invoice. Payments will transition
through the following `status` states: `"pending" -> "completed"` or `"pending" -> "failed"`.
Once a payment is finalized (either completed or failed), you do not need to query
the payment any more.

**Request:**

The request should include the `index` of the payment query as a query string
parameter.

**Examples:**

```bash
$ curl 'http://localhost:5393/v1/node/payment?index=0000001744926519917-ln_9be5e4e3a0356cc4a7a1dce5a4af39e2896b7eb7b007ec6ca8c2f8434f21a63a' \ 
     | jq .
{
  "payment": {
    "index": "0000001744926519917-ln_9be5e4e3a0356cc4a7a1dce5a4af39e2896b7eb7b007ec6ca8c2f8434f21a63a",
    "kind": "invoice",
    "direction": "inbound",
    "invoice": "lnbc1p5qzaehdqqpp5n0j7fcaqx4kvffapmnj6fteeu2ykkl4hkqr7cm9gctuyxnep5caqcqpcsp5slzxgxrsu3jq8xq7rp2gx3ge0thlt3446jpp8kqs87pve60679ls9qyysgqxqrrssnp4q0vzagw8x7r9eyalw35t0u6syql8rtqf9tejep0z6xrwkqrua5advrzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wzmk4uqqj5sqqyqqqqqqqqqqhwqqfq89vuhjlg2tt56sv9pdt8t5cvdgfaaf6nxqtt0av74ragpql7l2d42euknlw06fcgp8xhe93xe7c802z3hrnysfsjgavmwfts7zdvj2cqka3672",
    "fees": "0",
    "status": "pending",
    "status_str": "invoice generated"
  }
}

$ curl 'http://localhost:5393/v1/node/payment?index=0000001744926842458-ln_e1f8e7fa3f3b43eb65afe4897ca1c63688636ba0a23b3011710e433b51bb3f9a' \
    | jq .
{
  "payment": {
    "index": "0000001744926842458-ln_e1f8e7fa3f3b43eb65afe4897ca1c63688636ba0a23b3011710e433b51bb3f9a",
    "kind": "invoice",
    "direction": "outbound",
    "invoice": "lnbc100n1p5qz7z2dq58skjqnr90pjjq4r9wd6qpp5u8uw073l8dp7ked0ujyhegwxx6yxx6aq5ganqyt3pepnk5dm87dqcqpcsp5nrs44f3upgxysnylrrpyrxs96mgazjjstuykyew74zv0najzkdeq9qyysgqxqyz5vqnp4q0w73a6xytxxrhuuvqnqjckemyhv6avveuftl64zzm5878vq3zr4jrzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wz5ecsqq2pgqqcqqqqqqqqqqhwqqfqrpeeq5xdys8vcfcark45w992h6j5nhajc62wet0q25ggxjwhtcfn8c3qx30fqzq8mqxfdtks57zw25zp0z2kl9yrfwkkthxclawxpfcqtdcpfu",
    "amount": "10",
    "fees": "0.03",
    "status": "completed",
    "status_str": "completed",
    "finalized_at": 1744926857989
  }
}
```
