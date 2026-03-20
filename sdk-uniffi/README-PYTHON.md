# Lexe Python SDK

The Lexe Python SDK provides a Python interface for developers to control
self-custodial, always-online [Lexe](https://lexe.app) Lightning nodes.

* [Quickstart guide](https://docs.lexe.tech/python/quickstart/)
* [API reference](https://python.lexe.tech)

```bash
pip install lexe-sdk
```

## Create a wallet

```python
from lexe import Credentials, LexeWallet, RootSeed, WalletConfig

# Configure `mainnet()` or `testnet3()`
config = WalletConfig.mainnet()

# Sample a new seed and write it to ~/.lexe/seedphrase.txt
seed = RootSeed.generate()
seed.write(config)

# Create wallet and register with Lexe (data stored in ~/.lexe)
creds = Credentials.from_root_seed(seed)
wallet = LexeWallet.fresh(config, creds)
wallet.signup(root_seed=seed, partner_pk=None)

# Create a Lightning invoice
invoice = wallet.create_invoice(
    expiration_secs=3600,
    amount_sats=1000,
    description="Initial deposit",
)
print(f"Invoice: {invoice.invoice}")

# Wait for payment
payment = wallet.wait_for_payment(index=invoice.index, timeout_secs=300)
print(f"Payment received: {payment.amount_sats} sats")
```

## Load an existing wallet

```python
from lexe import Credentials, LexeWallet, RootSeed, WalletConfig

# Load existing wallet from ~/.lexe
config = WalletConfig.mainnet()
seed = RootSeed.read(config)
creds = Credentials.from_root_seed(seed)
wallet = LexeWallet.load(config, creds)

# Update to the latest node software
wallet.provision(creds)

# Pay a Lightning invoice
payment = wallet.pay_invoice(
    invoice="lnbc...",
    fallback_amount_sats=None,
    note="Paying for coffee",
)
payment = wallet.wait_for_payment(index=payment.index, timeout_secs=15)
print(f"Payment: {payment.status}")
```
