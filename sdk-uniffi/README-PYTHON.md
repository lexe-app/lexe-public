# Lexe Python SDK

The Lexe Python SDK provides a Python interface for developers to control
self-custodial, always-online [Lexe](https://lexe.app) Lightning nodes.

* [Quickstart guide](https://docs.lexe.tech/python/quickstart/)
* [API reference](https://python.lexe.tech)

```bash
pip install lexe-sdk
```

## Example

```python
import lexe

# Load an existing Lexe wallet from ~/.lexe
config = lexe.WalletEnvConfig.mainnet()
seed = config.read_seed()
wallet = lexe.LexeWallet.load(config, seed)

# Pay a Lightning invoice
payment = wallet.pay_invoice(
    invoice="lnbc...",
    fallback_amount_sats=None,
    note="Paying for coffee",
)
payment = wallet.wait_for_payment(
    index=payment.index,
    timeout_secs=15,
)
print(f"payment: {payment.status}")
```
