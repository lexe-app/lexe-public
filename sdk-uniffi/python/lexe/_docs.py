"""Python-specific docstring enrichments for the Lexe SDK.

This module overrides the auto-generated UniFFI docstrings with
Python-flavored documentation (Args, Returns, Raises, Examples).

The Rust ``///`` doc comments in ``lib.rs`` are kept clean and
language-agnostic so they can be reused across Swift, Kotlin, and JS.
This file adds the Python-specific layer on top.

Imported by ``__init__.py`` on package load.
"""

import inspect

from . import lexe


def _set_method_doc(cls: type, name: str, doc: str) -> None:
    """Set __doc__ on a class method, handling classmethods.

    Accessing ``cls.method`` triggers the descriptor protocol and returns
    a bound ``method`` object whose ``__doc__`` is read-only. We bypass
    this by using ``inspect.getattr_static`` to get the raw descriptor.
    """
    raw = inspect.getattr_static(cls, name)
    if isinstance(raw, classmethod):
        raw.__func__.__doc__ = doc
    else:
        raw.__doc__ = doc

# ================== #
# --- Global API --- #
# ================== #

lexe.default_lexe_data_dir.__doc__ = """\
Returns the default Lexe data directory (``~/.lexe``).

Returns:
    The absolute path to the default data directory.

Raises:
    FfiError: If the home directory cannot be determined.

Example::

    data_dir = default_lexe_data_dir()
    # '/home/user/.lexe'
"""

lexe.seedphrase_path.__doc__ = """\
Returns the path to the seedphrase file for the given environment.

- Mainnet: ``<lexe_data_dir>/seedphrase.txt``
- Other environments: ``<lexe_data_dir>/seedphrase.<env>.txt``

Args:
    env_config: Wallet environment configuration.
    lexe_data_dir: Base data directory path.

Returns:
    The absolute path to the seedphrase file.

Example::

    config = WalletEnvConfig.mainnet()
    path = seedphrase_path(config, "/home/user/.lexe")
    # '/home/user/.lexe/seedphrase.txt'
"""

lexe.read_seed.__doc__ = """\
Reads a root seed from ``~/.lexe/seedphrase[.env].txt``.

Args:
    env_config: Wallet environment configuration.

Returns:
    The root seed, or ``None`` if the file doesn't exist.

Raises:
    FfiError: If the file exists but cannot be read or parsed.

Example::

    config = WalletEnvConfig.mainnet()
    seed = read_seed(config)
    if seed is not None:
        print(f"Loaded seed ({len(seed.seed_bytes)} bytes)")
"""

lexe.read_seed_from_path.__doc__ = """\
Reads a root seed from a seedphrase file at the given path.

The file should contain a BIP39 mnemonic phrase.

Args:
    path: Absolute path to the seedphrase file.

Returns:
    The root seed, or ``None`` if the file doesn't exist.

Raises:
    FfiError: If the file exists but cannot be read or parsed.
"""

lexe.write_seed.__doc__ = """\
Writes a root seed's mnemonic to ``~/.lexe/seedphrase[.env].txt``.

Creates parent directories if needed.

Args:
    root_seed: The root seed to persist.
    env_config: Wallet environment configuration.

Raises:
    FfiError: If the file already exists or cannot be written.
"""

lexe.write_seed_to_path.__doc__ = """\
Writes a root seed's mnemonic to the given file path.

Creates parent directories if needed.

Args:
    root_seed: The root seed to persist.
    path: Absolute path to write the seedphrase file.

Raises:
    FfiError: If the file already exists or cannot be written.
"""

lexe.init_logger.__doc__ = """\
Initialize the Lexe logger with the given default log level.

Call once at startup to enable logging.

Args:
    default_level: Log level string. One of
        ``"trace"``, ``"debug"``, ``"info"``, ``"warn"``, ``"error"``.

Example::

    init_logger("info")
"""

lexe.try_load_wallet.__doc__ = """\
Try to load an existing wallet from local state.

Returns ``None`` if no local data exists for this user/environment.
If this returns ``None``, use :meth:`LexeWallet.fresh` to create local state.

It is recommended to always pass the same ``lexe_data_dir``,
regardless of environment or user. Data is namespaced internally.

Args:
    env_config: Wallet environment configuration.
    root_seed: The user's root seed.
    lexe_data_dir: Base data directory (default: ``~/.lexe``).

Returns:
    The loaded wallet, or ``None`` if no local data exists.

Raises:
    FfiError: If local data exists but is corrupted.

Example::

    config = WalletEnvConfig.mainnet()
    wallet = try_load_wallet(config, seed)
    if wallet is None:
        wallet = LexeWallet.fresh(config, seed)
"""

# ===================== #
# --- Configuration --- #
# ===================== #

lexe.DeployEnv.__doc__ = """\
Deployment environment for a wallet.

- **DEV** -- Development environment (local regtest).
- **STAGING** -- Staging environment (testnet).
- **PROD** -- Production environment (mainnet).
"""

lexe.LxNetwork.__doc__ = """\
Bitcoin network to use.

- **MAINNET** -- Bitcoin mainnet.
- **TESTNET3** -- Bitcoin testnet3.
- **TESTNET4** -- Bitcoin testnet4.
- **SIGNET** -- Bitcoin signet.
- **REGTEST** -- Bitcoin regtest (local development).
"""

lexe.WalletEnvConfig.__doc__ = """\
Configuration for a wallet environment.

Use the factory constructors to create configs for standard environments.

Example::

    # Production (mainnet)
    config = WalletEnvConfig.mainnet()

    # Staging (testnet)
    config = WalletEnvConfig.testnet3()

    # Local development (regtest)
    config = WalletEnvConfig.regtest()
    config = WalletEnvConfig.regtest(use_sgx=True, gateway_url="http://localhost:8080")
"""

# =================== #
# --- Credentials --- #
# =================== #

lexe.RootSeed.__doc__ = """\
The secret root seed for deriving all user keys and credentials.

Attributes:
    seed_bytes: 32-byte root seed.
"""

lexe.ClientCredentials.__doc__ = """\
Client credentials for authenticating with Lexe.

Attributes:
    credentials_base64: Base64-encoded credentials blob.
"""

# ================= #
# --- Node Info --- #
# ================= #

lexe.NodeInfo.__doc__ = """\
Information about a Lexe Lightning node.

Attributes:
    version: Node's current semver version, e.g. ``"0.6.9"``.
    measurement: Hex-encoded SGX measurement of the current node.
    user_pk: Hex-encoded ed25519 user public key.
    node_pk: Hex-encoded secp256k1 node public key ("node_id").
    balance_sats: Total balance in sats (Lightning + on-chain).
    lightning_balance_sats: Total Lightning balance in sats.
    lightning_sendable_balance_sats: Estimated Lightning sendable balance in sats.
    lightning_max_sendable_balance_sats: Maximum Lightning sendable balance in sats.
    onchain_balance_sats: Total on-chain balance in sats (includes unconfirmed).
    onchain_trusted_balance_sats: Trusted on-chain balance in sats.
    num_channels: Total number of Lightning channels.
    num_usable_channels: Number of usable Lightning channels.
"""

# ================== #
# --- LexeWallet --- #
# ================== #

lexe.LexeWallet.__doc__ = """\
The main wallet handle for interacting with a Lexe Lightning node.

Create a wallet using the factory constructors, then call
:meth:`signup` and :meth:`provision` before using payment methods.

Example::

    config = WalletEnvConfig.mainnet()
    seed = read_seed(config)

    wallet = LexeWallet.load_or_fresh(config, seed)
    await wallet.signup(seed)
    await wallet.provision(seed)

    info = await wallet.node_info()
    print(f"Balance: {info.balance_sats} sats")
"""

# LexeWallet constructors

_set_method_doc(lexe.LexeWallet, "fresh", """\
Create a fresh wallet, deleting any existing local state for this user.

Data for other users and environments is not affected.

Args:
    env_config: Wallet environment configuration.
    root_seed: The user's root seed.
    lexe_data_dir: Base data directory (default: ``~/.lexe``).

Returns:
    A new LexeWallet instance.

Raises:
    FfiError: If wallet creation fails.
""")

_set_method_doc(lexe.LexeWallet, "load_or_fresh", """\
Load an existing wallet, or create a fresh one if none exists.

This is the recommended constructor for most use cases.

Args:
    env_config: Wallet environment configuration.
    root_seed: The user's root seed.
    lexe_data_dir: Base data directory (default: ``~/.lexe``).

Returns:
    A LexeWallet instance (loaded or newly created).

Raises:
    FfiError: If wallet creation fails.
""")

# LexeWallet methods

_set_method_doc(lexe.LexeWallet, "signup", """\
Register this user with Lexe and provision their node.

Call after creating the wallet for the first time. Idempotent:
calling again for an already-signed-up user is safe.

.. important::

    After signup, persist the user's root seed!
    Without it, users lose access to their funds permanently.

Args:
    root_seed: The user's root seed.
    partner_pk: Optional hex-encoded user public key of your
        company account. Set to earn a share of fees.

Raises:
    FfiError: If signup or provisioning fails.

Example::

    await wallet.signup(seed)
    write_seed(seed, config)  # Persist seed after signup!
""")

_set_method_doc(lexe.LexeWallet, "provision", """\
Ensure the wallet is provisioned to all recent trusted releases.

Call every time the wallet is loaded to keep the user running
the most up-to-date enclave software.

Args:
    root_seed: The user's root seed.

Raises:
    FfiError: If provisioning fails.

Example::

    wallet = LexeWallet.load_or_fresh(config, seed)
    await wallet.provision(seed)
""")

_set_method_doc(lexe.LexeWallet, "user_pk", """\
Get the user's hex-encoded ed25519 public key.

Returns:
    Hex string of the user's public key derived from their root seed.
""")

_set_method_doc(lexe.LexeWallet, "node_info", """\
Get information about the node (balance, channels, version).

Returns:
    A :class:`NodeInfo` with the node's current state.

Raises:
    FfiError: If the node is unreachable.

Example::

    info = await wallet.node_info()
    print(f"Balance: {info.balance_sats} sats")
    print(f"Channels: {info.num_usable_channels}/{info.num_channels}")
""")

_set_method_doc(lexe.LexeWallet, "create_invoice", """\
Create a BOLT11 Lightning invoice.

Args:
    expiration_secs: Invoice expiry in seconds (e.g. ``3600`` for 1 hour).
    amount_sats: Amount in satoshis, or ``None`` for an amountless invoice.
    description: Optional description shown to the payer.

Returns:
    A :class:`CreateInvoiceResponse` with the invoice string and metadata.

Raises:
    FfiError: If the node is offline or the request fails.

Example::

    resp = await wallet.create_invoice(3600, 1000, "Coffee")
    print(resp.invoice)      # BOLT11 invoice string
    print(resp.amount_sats)  # 1000
""")

_set_method_doc(lexe.LexeWallet, "pay_invoice", """\
Pay a BOLT11 Lightning invoice.

Args:
    invoice: BOLT11 invoice string to pay.
    fallback_amount_sats: Required if the invoice has no amount encoded.
    note: Optional private note (not visible to the receiver).

Returns:
    A :class:`PayInvoiceResponse` with the payment index and timestamp.

Raises:
    FfiError: If the invoice is invalid or payment initiation fails.

Example::

    resp = await wallet.pay_invoice(bolt11_string)
    payment = await wallet.wait_for_payment_completion(
        resp.payment_index, timeout_secs=120,
    )
    print(f"Payment {payment.status}")
""")

_set_method_doc(lexe.LexeWallet, "get_payment", """\
Get a specific payment by its index.

Args:
    payment_index: Full payment index string
        (format: ``<created_at_ms>-<payment_id>``).

Returns:
    The :class:`Payment`, or ``None`` if not found locally.

Raises:
    FfiError: If the index is malformed or the request fails.
""")

_set_method_doc(lexe.LexeWallet, "update_payment_note", """\
Update a payment's personal note.

Call :meth:`sync_payments` first so the payment exists locally.

Args:
    payment_index: Full payment index string.
    note: New note text, or ``None`` to clear.

Raises:
    FfiError: If the payment doesn't exist locally.
""")

_set_method_doc(lexe.LexeWallet, "sync_payments", """\
Sync payments from the remote node to local storage.

Call periodically to keep local payment data up to date.

Returns:
    A :class:`PaymentSyncSummary` with counts of new and updated payments.

Raises:
    FfiError: If the node is unreachable.

Example::

    summary = await wallet.sync_payments()
    print(f"New: {summary.num_new}, Updated: {summary.num_updated}")
""")

_set_method_doc(lexe.LexeWallet, "list_payments", """\
List payments from local storage.

Reads from the local database only (no network calls).
Call :meth:`sync_payments` first to ensure data is fresh.

Args:
    filter: Which payments to include
        (:attr:`PaymentFilter.ALL`, :attr:`PaymentFilter.PENDING`,
        or :attr:`PaymentFilter.FINALIZED`).
    offset: Pagination offset (0-based).
    limit: Maximum number of payments to return.

Returns:
    A :class:`ListPaymentsResponse` with payments and total count.

Example::

    await wallet.sync_payments()
    resp = wallet.list_payments(PaymentFilter.ALL, offset=0, limit=20)
    for p in resp.payments:
        print(f"{p.payment_index}: {p.amount_sats} sats ({p.status})")
""")

_set_method_doc(lexe.LexeWallet, "latest_payment_sync_index", """\
Get the latest payment sync watermark.

Returns:
    The ``updated_at`` index of the most recently synced payment,
    or ``None`` if no payments have been synced yet.
""")

_set_method_doc(lexe.LexeWallet, "delete_local_payments", """\
Delete all local payment data for this wallet.

Clears the local payment cache only. Remote data on the node is
not affected. Call :meth:`sync_payments` to re-populate.

Raises:
    FfiError: If the local database cannot be cleared.
""")

_set_method_doc(lexe.LexeWallet, "wait_for_payment_completion", """\
Wait for a payment to reach a terminal state (completed or failed).

Polls the node with exponential backoff until the payment finalizes
or the timeout is reached.

Args:
    payment_index: Full payment index string.
    timeout_secs: Maximum wait time in seconds (recommended: ``120``,
        max: ``10800`` i.e. 3 hours).

Returns:
    The finalized :class:`Payment`.

Raises:
    FfiError: If the timeout is exceeded or the node is unreachable.

Example::

    resp = await wallet.pay_invoice(invoice_str)
    payment = await wallet.wait_for_payment_completion(
        resp.payment_index, timeout_secs=120,
    )
    assert payment.status in (PaymentStatus.COMPLETED, PaymentStatus.FAILED)
""")

# ================ #
# --- Payments --- #
# ================ #

lexe.ConfirmationPriority.__doc__ = """\
Confirmation priority for on-chain Bitcoin transactions.

- **HIGH** -- Fastest confirmation (highest fees).
- **NORMAL** -- Standard confirmation target.
- **BACKGROUND** -- Lowest fees (slowest confirmation).
"""

lexe.PaymentDirection.__doc__ = """\
Direction of a payment relative to this wallet.

- **INBOUND** -- Incoming payment (receiving funds).
- **OUTBOUND** -- Outgoing payment (sending funds).
- **INFO** -- Informational (e.g. channel open/close events).
"""

lexe.PaymentRail.__doc__ = """\
Technical rail used to fulfill a payment.

- **ONCHAIN** -- On-chain Bitcoin transaction.
- **INVOICE** -- Lightning BOLT11 invoice.
- **OFFER** -- Lightning BOLT12 offer.
- **SPONTANEOUS** -- Spontaneous (keysend) Lightning payment.
- **WAIVED_FEE** -- Internal waived fee.
- **UNKNOWN** -- Unknown rail from a newer node version.
"""

lexe.PaymentStatus.__doc__ = """\
Status of a payment.

- **PENDING** -- Payment is in progress.
- **COMPLETED** -- Payment completed successfully.
- **FAILED** -- Payment failed.
"""

lexe.PaymentFilter.__doc__ = """\
Filter for listing payments from local storage.

- **ALL** -- Include all payments.
- **PENDING** -- Only pending payments.
- **FINALIZED** -- Only finalized payments (completed or failed).
"""

lexe.PaymentKind.__doc__ = """\
Application-level kind for a payment.

- **ONCHAIN** -- On-chain Bitcoin payment.
- **INVOICE** -- Lightning BOLT11 invoice payment.
- **OFFER** -- Lightning BOLT12 offer payment.
- **SPONTANEOUS** -- Spontaneous (keysend) Lightning payment.
- **WAIVED_CHANNEL_FEE** -- Waived channel fee.
- **WAIVED_LIQUIDITY_FEE** -- Waived liquidity fee.
- **UNKNOWN** -- Unknown kind from a newer node version.
"""

lexe.Invoice.__doc__ = """\
A BOLT11 Lightning invoice.

Attributes:
    string: Full bech32-encoded invoice string.
    description: Invoice description, if present.
    created_at_ms: Creation timestamp (ms since UNIX epoch).
    expires_at_ms: Expiration timestamp (ms since UNIX epoch).
    amount_sats: Amount in satoshis, if specified.
    payee_pubkey: Hex-encoded payee node public key.
"""

lexe.Payment.__doc__ = """\
Information about a payment.

Attributes:
    payment_index: Full payment index (``<created_at_ms>-<payment_id>``).
    payment_id: Payment identifier without the timestamp.
    created_at_ms: When payment was created (ms since UNIX epoch).
    updated_at_ms: When payment was last updated (ms since UNIX epoch).
    rail: Technical rail used to fulfill this payment.
    kind: Application-level payment kind.
    direction: Payment direction (inbound, outbound, or info).
    status: Payment status.
    status_msg: Human-readable payment status message.
    amount_sats: Payment amount in satoshis, if known.
    fees_sats: Fees paid in satoshis.
    note: Optional personal note attached to this payment.
    invoice: BOLT11 invoice used for this payment, if any.
    txid: Hex-encoded Bitcoin txid (on-chain payments only).
    address: Bitcoin address for on-chain sends.
    expires_at_ms: Invoice/offer expiry time (ms since UNIX epoch).
    finalized_at_ms: When this payment finalized (ms since UNIX epoch).
    payer_name: Payer's self-reported name (offer payments).
    payer_note: Payer's provided note (offer payments).
    priority: Confirmation priority for on-chain sends.
"""

lexe.PaymentSyncSummary.__doc__ = """\
Summary of a payment sync operation.

Attributes:
    num_new: Number of new payments added to the local DB.
    num_updated: Number of existing payments that were updated.
"""

lexe.ListPaymentsResponse.__doc__ = """\
Response from listing payments.

Attributes:
    payments: Payments in the requested window.
    total_count: Total number of payments for this filter.
"""

# ================ #
# --- Invoices --- #
# ================ #

lexe.CreateInvoiceResponse.__doc__ = """\
Response from creating a Lightning invoice.

Attributes:
    payment_index: Payment created index for this invoice.
    invoice: BOLT11 invoice string.
    description: Description encoded in the invoice, if provided.
    amount_sats: Amount in satoshis, if specified.
    created_at_ms: Invoice creation time (ms since UNIX epoch).
    expires_at_ms: Invoice expiration time (ms since UNIX epoch).
    payment_hash: Hex-encoded payment hash.
    payment_secret: Payment secret.
"""

lexe.PayInvoiceResponse.__doc__ = """\
Response from paying a Lightning invoice.

Attributes:
    payment_index: Payment created index for this payment.
    created_at_ms: When payment was initiated (ms since UNIX epoch).
"""

# ================= #
# --- FfiError  --- #
# ================= #

lexe.FfiError.__doc__ = """\
Error type raised by SDK methods.

Catch this to handle Lexe SDK errors.

Example::

    try:
        info = await wallet.node_info()
    except FfiError as e:
        print(f"SDK error: {e.message()}")
"""
