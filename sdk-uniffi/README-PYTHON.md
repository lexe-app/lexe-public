# Lexe Python SDK

[![pypi.org](https://img.shields.io/pypi/v/lexe-sdk)](https://pypi.org/project/lexe-sdk/)
[![pypi.org - requires-python](https://img.shields.io/python/required-version-toml?tomlFilePath=https%3A%2F%2Fraw.githubusercontent.com%2Flexe-app%2Flexe-public%2Fmaster%2Fsdk-uniffi%2Fpyproject.toml)](https://pypi.org/project/lexe-sdk/)
[![python.lexe.tech](https://img.shields.io/badge/docs-passing-brightgreen)](https://python.lexe.tech/)
[![CI](https://img.shields.io/github/actions/workflow/status/lexe-app/lexe-public/.github%2Fworkflows%2Fci.yml)](https://github.com/lexe-app/lexe-public/actions/workflows/ci.yml)
[![MIT](https://img.shields.io/pypi/l/lexe-sdk)](../LICENSE-MIT)
[![Discord](https://img.shields.io/discord/1151246286549434398)](https://discord.gg/zybuBYgdbr)

The Lexe Python SDK provides a Python interface for developers to control
self-custodial, always-online [Lexe](https://lexe.app) Lightning nodes.

* [Quickstart guide](https://docs.lexe.tech/python/quickstart/)
* [API reference](https://python.lexe.tech)
* [GitHub repo](https://github.com/lexe-app/lexe-public/tree/master/sdk-uniffi)

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
