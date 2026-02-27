"""Python-specific docstring enrichments for the Lexe SDK.

This module overrides the auto-generated UniFFI docstrings with
Python-flavored documentation (Args, Returns, Raises, Examples).

The Rust ``///`` doc comments in ``lib.rs`` are kept clean and
language-agnostic so they can be reused across Swift, Kotlin, and JS.
This file adds the Python-specific layer on top.

Imported by ``__init__.py`` on package load (after ``_preprocess.py``).
"""

import inspect

from . import lexe

# Aliases for local reasoning: the public-facing names after re-export.
from .lexe import BlockingLexeWallet as LexeWallet
from .lexe import AsyncLexeWallet


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

_set_method_doc(lexe.WalletEnvConfig, "seedphrase_path", """\
Returns the path to the seedphrase file for this environment.

- Mainnet: ``<lexe_data_dir>/seedphrase.txt``
- Other environments: ``<lexe_data_dir>/seedphrase.<env>.txt``

Args:
    lexe_data_dir: Base data directory path.

Returns:
    The absolute path to the seedphrase file.

Example::

    config = WalletEnvConfig.mainnet()
    path = config.seedphrase_path("/home/user/.lexe")
    # '/home/user/.lexe/seedphrase.txt'
""")

_set_method_doc(lexe.WalletEnvConfig, "read_seed", """\
Reads a root seed from ``~/.lexe/seedphrase[.env].txt``.

Returns:
    The root seed loaded from the file.

Raises:
    SeedFileError.NotFound: If the seedphrase file doesn't exist.
    SeedFileError.ParseError: If the file exists but cannot be parsed.

Example::

    config = WalletEnvConfig.mainnet()
    try:
        seed = config.read_seed()
    except SeedFileError.NotFound:
        seed = RootSeed(os.urandom(32))
""")

_set_method_doc(lexe.RootSeed, "read_from_path", """\
Reads a root seed from a seedphrase file containing a BIP39 mnemonic.

Args:
    path: Absolute path to the seedphrase file.

Returns:
    The root seed loaded from the file.

Raises:
    SeedFileError.NotFound: If the file doesn't exist.
    SeedFileError.ParseError: If the file cannot be parsed.

Example::

    try:
        seed = RootSeed.read_from_path("/home/user/.lexe/seedphrase.txt")
    except SeedFileError.NotFound:
        seed = RootSeed(os.urandom(32))
""")

_set_method_doc(lexe.WalletEnvConfig, "write_seed", """\
Writes a root seed's mnemonic to ``~/.lexe/seedphrase[.env].txt``.

Creates parent directories if needed.

Args:
    root_seed: The root seed to persist.

Raises:
    SeedFileError.AlreadyExists: If the file already exists.
    SeedFileError.IoError: If the file cannot be written.

Example::

    config = WalletEnvConfig.mainnet()
    config.write_seed(seed)
""")

_set_method_doc(lexe.RootSeed, "write_to_path", """\
Writes this root seed's mnemonic to the given file path.

Creates parent directories if needed.

Args:
    path: Absolute path to write the seedphrase file.

Raises:
    SeedFileError.AlreadyExists: If the file already exists.
    SeedFileError.IoError: If the file cannot be written.

Example::

    seed = RootSeed(os.urandom(32))
    seed.write_to_path("/home/user/.lexe/seedphrase.txt")
""")

lexe.init_logger.__doc__ = """\
Initialize the Lexe logger with the given default log level.

Call once at startup to enable logging.

Args:
    default_level: Log level string. One of
        ``"trace"``, ``"debug"``, ``"info"``, ``"warn"``, ``"error"``.

Example::

    init_logger("info")
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

Create with :class:`RootSeed` from raw bytes, or load from a file
with :meth:`RootSeed.read_from_path`.

Example::

    import os

    # Create from random bytes
    seed = RootSeed(os.urandom(32))
    print(f"Seed: {len(seed.seed_bytes)} bytes")

    # Load from file
    seed = RootSeed.read_from_path("/home/user/.lexe/seedphrase.txt")
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

# ========================= #
# --- LexeWallet (sync) --- #
# ========================= #

LexeWallet.__doc__ = """\
Synchronous wallet handle for interacting with a Lexe Lightning node.

For async usage, use :class:`AsyncLexeWallet`.

Create a wallet using one of the constructors:

- :meth:`load` -- Load from existing local state.
- :meth:`fresh` -- Create fresh local state (deletes existing).
- :meth:`load_or_fresh` -- Load or create if none exists.

Then call :meth:`signup` and :meth:`provision` before using payment methods.

Example::

    config = WalletEnvConfig.mainnet()
    seed = config.read_seed()  # Raises SeedFileError.NotFound if missing

    wallet = LexeWallet.load_or_fresh(config, seed)
    wallet.signup(seed)
    wallet.provision(seed)

    info = wallet.node_info()
    print(f"Balance: {info.balance_sats} sats")
"""

_set_method_doc(LexeWallet, "load", """\
Load an existing wallet from local state.

Raises :class:`LoadWalletError.NotFound` if no local data exists.
Use :meth:`LexeWallet.fresh` to create local state.

Args:
    env_config: Wallet environment configuration.
    root_seed: The user's root seed.
    lexe_data_dir: Base data directory (default: ``~/.lexe``).

Returns:
    The loaded LexeWallet instance.

Raises:
    LoadWalletError.NotFound: If no local data exists.
    LoadWalletError.LoadFailed: If local data is corrupted.

Example::

    try:
        wallet = LexeWallet.load(config, seed)
    except LoadWalletError.NotFound:
        wallet = LexeWallet.fresh(config, seed)
""")

_set_method_doc(LexeWallet, "fresh", """\
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

_set_method_doc(LexeWallet, "load_or_fresh", """\
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

_set_method_doc(LexeWallet, "signup", """\
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

    wallet.signup(seed)
    config.write_seed(seed)  # Persist seed after signup!
""")

_set_method_doc(LexeWallet, "provision", """\
Ensure the wallet is provisioned to all recent trusted releases.

Call every time the wallet is loaded to keep the user running
the most up-to-date enclave software.

Args:
    root_seed: The user's root seed.

Raises:
    FfiError: If provisioning fails.

Example::

    wallet = LexeWallet.load_or_fresh(config, seed)
    wallet.provision(seed)
""")

_set_method_doc(LexeWallet, "node_info", """\
Get information about the node (balance, channels, version).

Returns:
    A :class:`NodeInfo` with the node's current state.

Raises:
    FfiError: If the node is unreachable.

Example::

    info = wallet.node_info()
    print(f"Balance: {info.balance_sats} sats")
    print(f"Channels: {info.num_usable_channels}/{info.num_channels}")
""")

_set_method_doc(LexeWallet, "create_invoice", """\
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

    resp = wallet.create_invoice(3600, 1000, "Coffee")
    print(resp.invoice)      # BOLT11 invoice string
    print(resp.amount_sats)  # 1000
""")

_set_method_doc(LexeWallet, "pay_invoice", """\
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

    resp = wallet.pay_invoice(bolt11_string)
    payment = wallet.wait_for_payment_completion(
        resp.payment_index, timeout_secs=120,
    )
    print(f"Payment {payment.status}")
""")

_set_method_doc(LexeWallet, "get_payment", """\
Get a specific payment by its index.

Args:
    payment_index: Full payment index string
        (format: ``<created_at_ms>-<payment_id>``).

Returns:
    The :class:`Payment`, or ``None`` if not found locally.

Raises:
    FfiError: If the index is malformed or the request fails.
""")

_set_method_doc(LexeWallet, "update_payment_note", """\
Update a payment's personal note.

Call :meth:`sync_payments` first so the payment exists locally.

Args:
    payment_index: Full payment index string.
    note: New note text, or ``None`` to clear.

Raises:
    FfiError: If the payment doesn't exist locally.
""")

_set_method_doc(LexeWallet, "sync_payments", """\
Sync payments from the remote node to local storage.

Call periodically to keep local payment data up to date.

Returns:
    A :class:`PaymentSyncSummary` with counts of new and updated payments.

Raises:
    FfiError: If the node is unreachable.

Example::

    summary = wallet.sync_payments()
    print(f"New: {summary.num_new}, Updated: {summary.num_updated}")
""")

_set_method_doc(LexeWallet, "list_payments", """\
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

    wallet.sync_payments()
    resp = wallet.list_payments(PaymentFilter.ALL, offset=0, limit=20)
    for p in resp.payments:
        print(f"{p.payment_index}: {p.amount_sats} sats ({p.status})")
""")

_set_method_doc(LexeWallet, "latest_payment_sync_index", """\
Get the latest payment sync watermark.

Returns:
    The ``updated_at`` index of the most recently synced payment,
    or ``None`` if no payments have been synced yet.
""")

_set_method_doc(LexeWallet, "delete_local_payments", """\
Delete all local payment data for this wallet.

Clears the local payment cache only. Remote data on the node is
not affected. Call :meth:`sync_payments` to re-populate.

Raises:
    FfiError: If the local database cannot be cleared.
""")

_set_method_doc(LexeWallet, "wait_for_payment_completion", """\
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

    resp = wallet.pay_invoice(invoice_str)
    payment = wallet.wait_for_payment_completion(
        resp.payment_index, timeout_secs=120,
    )
    assert payment.status in (PaymentStatus.COMPLETED, PaymentStatus.FAILED)
""")

# ======================= #
# --- AsyncLexeWallet --- #
# ======================= #

AsyncLexeWallet.__doc__ = """\
Async wallet handle for interacting with a Lexe Lightning node.

For synchronous usage, use :class:`LexeWallet`.

Example::

    from lexe import AsyncLexeWallet, WalletEnvConfig

    config = WalletEnvConfig.mainnet()
    seed = config.read_seed()

    wallet = AsyncLexeWallet.load_or_fresh(config, seed)
    await wallet.signup(seed)
    await wallet.provision(seed)

    info = await wallet.node_info()
    print(f"Balance: {info.balance_sats} sats")
"""

_set_method_doc(AsyncLexeWallet, "load", """\
Load an existing wallet from local state.

Raises :class:`LoadWalletError.NotFound` if no local data exists.
Use :meth:`AsyncLexeWallet.fresh` to create local state.

Args:
    env_config: Wallet environment configuration.
    root_seed: The user's root seed.
    lexe_data_dir: Base data directory (default: ``~/.lexe``).

Returns:
    The loaded AsyncLexeWallet instance.

Raises:
    LoadWalletError.NotFound: If no local data exists.
    LoadWalletError.LoadFailed: If local data is corrupted.

Example::

    try:
        wallet = AsyncLexeWallet.load(config, seed)
    except LoadWalletError.NotFound:
        wallet = AsyncLexeWallet.fresh(config, seed)
""")

_set_method_doc(AsyncLexeWallet, "fresh", """\
Create a fresh wallet, deleting any existing local state for this user.

Data for other users and environments is not affected.

Args:
    env_config: Wallet environment configuration.
    root_seed: The user's root seed.
    lexe_data_dir: Base data directory (default: ``~/.lexe``).

Returns:
    A new AsyncLexeWallet instance.

Raises:
    FfiError: If wallet creation fails.
""")

_set_method_doc(AsyncLexeWallet, "load_or_fresh", """\
Load an existing wallet, or create a fresh one if none exists.

This is the recommended constructor for most use cases.

Args:
    env_config: Wallet environment configuration.
    root_seed: The user's root seed.
    lexe_data_dir: Base data directory (default: ``~/.lexe``).

Returns:
    An AsyncLexeWallet instance (loaded or newly created).

Raises:
    FfiError: If wallet creation fails.
""")

_set_method_doc(AsyncLexeWallet, "signup", """\
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
    config.write_seed(seed)  # Persist seed after signup!
""")

_set_method_doc(AsyncLexeWallet, "provision", """\
Ensure the wallet is provisioned to all recent trusted releases.

Call every time the wallet is loaded to keep the user running
the most up-to-date enclave software.

Args:
    root_seed: The user's root seed.

Raises:
    FfiError: If provisioning fails.

Example::

    wallet = AsyncLexeWallet.load_or_fresh(config, seed)
    await wallet.provision(seed)
""")

_set_method_doc(AsyncLexeWallet, "node_info", """\
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

_set_method_doc(AsyncLexeWallet, "create_invoice", """\
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

_set_method_doc(AsyncLexeWallet, "pay_invoice", """\
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

_set_method_doc(AsyncLexeWallet, "get_payment", """\
Get a specific payment by its index.

Args:
    payment_index: Full payment index string
        (format: ``<created_at_ms>-<payment_id>``).

Returns:
    The :class:`Payment`, or ``None`` if not found locally.

Raises:
    FfiError: If the index is malformed or the request fails.
""")

_set_method_doc(AsyncLexeWallet, "update_payment_note", """\
Update a payment's personal note.

Call :meth:`sync_payments` first so the payment exists locally.

Args:
    payment_index: Full payment index string.
    note: New note text, or ``None`` to clear.

Raises:
    FfiError: If the payment doesn't exist locally.
""")

_set_method_doc(AsyncLexeWallet, "sync_payments", """\
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

_set_method_doc(AsyncLexeWallet, "list_payments", """\
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

_set_method_doc(AsyncLexeWallet, "latest_payment_sync_index", """\
Get the latest payment sync watermark.

Returns:
    The ``updated_at`` index of the most recently synced payment,
    or ``None`` if no payments have been synced yet.
""")

_set_method_doc(AsyncLexeWallet, "delete_local_payments", """\
Delete all local payment data for this wallet.

Clears the local payment cache only. Remote data on the node is
not affected. Call :meth:`sync_payments` to re-populate.

Raises:
    FfiError: If the local database cannot be cleared.
""")

_set_method_doc(AsyncLexeWallet, "wait_for_payment_completion", """\
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

lexe.SeedFileError.__doc__ = """\
Error type for seedphrase file operations.

Raised by :meth:`RootSeed.read_from_path`, :meth:`RootSeed.write_to_path`,
:meth:`WalletEnvConfig.read_seed`, and :meth:`WalletEnvConfig.write_seed`.

Variants:

- **NotFound** -- Seedphrase file not found. Attributes: ``path``.
- **ParseError** -- Failed to parse the seedphrase. Attributes: ``message``.
- **AlreadyExists** -- Seedphrase file already exists. Attributes: ``path``.
- **IoError** -- I/O error during the file operation. Attributes: ``message``.

Example::

    try:
        seed = RootSeed.read_from_path(path)
    except SeedFileError.NotFound as e:
        print(f"No seed at {e.path}")
    except SeedFileError.ParseError as e:
        print(f"Bad seed: {e.message}")
"""

lexe.LoadWalletError.__doc__ = """\
Error type for wallet loading operations.

Raised by :meth:`LexeWallet.load` and :meth:`AsyncLexeWallet.load`.

Variants:

- **NotFound** -- No local wallet data found for this user/environment.
- **LoadFailed** -- Failed to load the wallet. Attributes: ``message``.

Example::

    try:
        wallet = LexeWallet.load(config, seed)
    except LoadWalletError.NotFound:
        wallet = LexeWallet.fresh(config, seed)
    except LoadWalletError.LoadFailed as e:
        print(f"Load failed: {e.message}")
"""

lexe.FfiError.__doc__ = """\
Error type raised by SDK methods.

Catch this to handle Lexe SDK errors.

Example::

    try:
        info = wallet.node_info()
    except FfiError as e:
        print(f"SDK error: {e.message()}")
"""
