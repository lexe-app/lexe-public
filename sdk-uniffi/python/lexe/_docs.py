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

_set_method_doc(lexe.WalletConfig, "seedphrase_path", """\
Returns the path to the seedphrase file for this environment.

- Mainnet: ``<lexe_data_dir>/seedphrase.txt``
- Other environments: ``<lexe_data_dir>/seedphrase.<env>.txt``

Args:
    lexe_data_dir: Base data directory path. Defaults to ``~/.lexe``.

Returns:
    The absolute path to the seedphrase file.

Example::

    config = WalletConfig.mainnet()
    path = config.seedphrase_path()
    # '/home/user/.lexe/seedphrase.txt'
""")

_set_method_doc(lexe.RootSeed, "read", """\
Reads a root seed from ``~/.lexe/seedphrase[.env].txt``.

Args:
    env_config: The wallet environment config.

Returns:
    The root seed loaded from the file.

Raises:
    SeedFileError.NotFound: If the seedphrase file doesn't exist.
    SeedFileError.ParseError: If the file exists but cannot be parsed.

Example::

    config = WalletConfig.mainnet()
    try:
        seed = RootSeed.read(config)
    except SeedFileError.NotFound:
        seed = RootSeed.from_bytes(os.urandom(32))
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
        seed = RootSeed.from_bytes(os.urandom(32))
""")

_set_method_doc(lexe.RootSeed, "write", """\
Writes this root seed's mnemonic to ``~/.lexe/seedphrase[.env].txt``.

Creates parent directories if needed.

Args:
    env_config: The wallet environment config.

Raises:
    SeedFileError.AlreadyExists: If the file already exists.
    SeedFileError.IoError: If the file cannot be written.

Example::

    config = WalletConfig.mainnet()
    seed.write(config)
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

    seed = RootSeed.from_bytes(os.urandom(32))
    seed.write_to_path("/home/user/.lexe/seedphrase.txt")
""")

_set_method_doc(lexe.RootSeed, "generate", """\
Generate a new random root seed.

Returns:
    A new randomly-generated RootSeed.

Example::

    seed = RootSeed.generate()
""")

_set_method_doc(lexe.RootSeed, "from_mnemonic", """\
Construct a root seed from a BIP39 mnemonic string.

Args:
    mnemonic: Space-separated BIP39 mnemonic words.

Returns:
    The root seed derived from the mnemonic.

Raises:
    FfiError: If the mnemonic is invalid.

Example::

    seed = RootSeed.from_mnemonic("abandon abandon ... about")
""")

_set_method_doc(lexe.RootSeed, "from_bytes", """\
Construct a root seed from raw bytes. The seed must be exactly 32 bytes.

Args:
    seed_bytes: Raw 32-byte seed.

Returns:
    A RootSeed from the given bytes.

Raises:
    FfiError: If the input is not exactly 32 bytes.

Example::

    import os
    seed = RootSeed.from_bytes(os.urandom(32))
""")

_set_method_doc(lexe.RootSeed, "from_hex", """\
Construct a root seed from a 64-character hex string.

Args:
    hex_string: 64-character hex-encoded seed.

Returns:
    A RootSeed from the given hex string.

Raises:
    FfiError: If the hex string is invalid or the wrong length.

Example::

    seed = RootSeed.from_hex("a1b2c3...")
""")

_set_method_doc(lexe.RootSeed, "to_bytes", """\
Return the 32-byte root seed.

Returns:
    The raw seed bytes.
""")

_set_method_doc(lexe.RootSeed, "to_hex", """\
Encode the root secret as a 64-character hex string.

Returns:
    The hex-encoded seed string.
""")

_set_method_doc(lexe.RootSeed, "to_mnemonic", """\
Return this root seed's mnemonic as a space-separated string.

Returns:
    The BIP39 mnemonic string.

Example::

    seed = RootSeed.generate()
    print(seed.to_mnemonic())
""")

_set_method_doc(lexe.RootSeed, "derive_user_pk", """\
Derive the user's public key.

Returns:
    The hex-encoded ed25519 user public key string.

Example::

    seed = RootSeed.generate()
    print(f"User PK: {seed.derive_user_pk()}")
""")

_set_method_doc(lexe.RootSeed, "derive_node_pk", """\
Derive the node public key.

Returns:
    The hex-encoded secp256k1 node public key string.

Example::

    seed = RootSeed.generate()
    print(f"Node PK: {seed.derive_node_pk()}")
""")

_set_method_doc(lexe.RootSeed, "password_encrypt", """\
Encrypt this root seed under the given password.

Args:
    password: The password to encrypt with.

Returns:
    The encrypted seed as bytes.

Raises:
    FfiError: If encryption fails.

Example::

    encrypted = seed.password_encrypt("my-password")
""")

_set_method_doc(lexe.RootSeed, "password_decrypt", """\
Decrypt a password-encrypted root seed.

Args:
    password: The password used to encrypt.
    encrypted: The encrypted seed bytes.

Returns:
    The decrypted RootSeed.

Raises:
    FfiError: If decryption fails (wrong password or corrupted data).

Example::

    encrypted = seed.password_encrypt("my-password")
    decrypted = RootSeed.password_decrypt("my-password", encrypted)
""")

lexe.init_logger.__doc__ = """\
Initialize the Lexe logger with the given default log level.

Call once at startup to enable logging.

Args:
    default_level: Log level string. One of
        ``"trace"``, ``"debug"``, ``"info"``, ``"warn"``, ``"error"``.
        Defaults to ``"info"``.

Example::

    init_logger()
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

lexe.Network.__doc__ = """\
Bitcoin network to use.

- **MAINNET** -- Bitcoin mainnet.
- **TESTNET3** -- Bitcoin testnet3.
- **TESTNET4** -- Bitcoin testnet4.
- **SIGNET** -- Bitcoin signet.
- **REGTEST** -- Bitcoin regtest (local development).
"""

lexe.WalletConfig.__doc__ = """\
Configuration for a wallet environment.

Use the factory constructors to create configs for standard environments.

Example::

    # Production (mainnet)
    config = WalletConfig.mainnet()

    # Staging (testnet)
    config = WalletConfig.testnet3()

    # Local development (regtest)
    config = WalletConfig.regtest()
    config = WalletConfig.regtest(use_sgx=True, gateway_url="http://localhost:8080")
"""

_set_method_doc(lexe.WalletConfig, "mainnet", """\
Create a config for mainnet (production).

Returns:
    A WalletConfig for the mainnet environment.
""")

_set_method_doc(lexe.WalletConfig, "testnet3", """\
Create a config for testnet3 (staging).

Returns:
    A WalletConfig for the testnet3 environment.
""")

_set_method_doc(lexe.WalletConfig, "regtest", """\
Create a config for regtest (local development).

Args:
    use_sgx: Whether SGX is enabled. Defaults to ``False``.
    gateway_url: Gateway URL override, or ``None`` for default.

Returns:
    A WalletConfig for the regtest environment.
""")

_set_method_doc(lexe.WalletConfig, "deploy_env", """\
Get the configured deployment environment.

Returns:
    The :class:`DeployEnv` for this config.
""")

_set_method_doc(lexe.WalletConfig, "network", """\
Get the configured Bitcoin network.

Returns:
    The :class:`Network` for this config.
""")

_set_method_doc(lexe.WalletConfig, "use_sgx", """\
Whether SGX is enabled for this config.

Returns:
    ``True`` if SGX is enabled, ``False`` otherwise.
""")

_set_method_doc(lexe.WalletConfig, "gateway_url", """\
Get the gateway URL for this environment.

Returns:
    The gateway URL string, or ``None`` if using the default.
""")

# =================== #
# --- Credentials --- #
# =================== #

lexe.RootSeed.__doc__ = """\
The secret root seed for deriving all user keys and credentials.

Create with :meth:`RootSeed.generate`, from raw bytes with
:meth:`RootSeed.from_bytes`, or load from a file with :meth:`RootSeed.read`
or :meth:`RootSeed.read_from_path`.

Example::

    # Generate a new random seed
    seed = RootSeed.generate()

    # Or create from raw bytes
    import os
    seed = RootSeed.from_bytes(os.urandom(32))

    # Or load from the default seedphrase path for this environment
    config = WalletConfig.mainnet()
    seed = RootSeed.read(config)

    # Or load from a specific file path
    seed = RootSeed.read_from_path("/home/user/.lexe/seedphrase.txt")
"""

lexe.ClientCredentials.__doc__ = """\
Scoped and revocable credentials for controlling a Lexe user node.

These are useful when you want node access without exposing the user's
:class:`RootSeed`, which is irrevocable.
Wrap into :class:`Credentials` to use with wallet constructors.

Methods:
    from_string(s): Parse credentials from a portable string.
    export_string(): Export credentials as a portable string
        (round-trips with ``from_string``).
"""

lexe.Credentials.__doc__ = """\
Authentication credentials for a Lexe user node.

Create from a :class:`RootSeed` or :class:`ClientCredentials`, then pass
to wallet constructors and :meth:`~LexeWallet.provision`.

Example::

    # From a root seed
    creds = Credentials.from_root_seed(seed)

    # From client credentials
    cc = ClientCredentials.from_string(token_string)
    creds = Credentials.from_client_credentials(cc)

    # Use with wallet constructors
    wallet = LexeWallet.load_or_fresh(config, creds)
    wallet.provision(creds)
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

    config = WalletConfig.mainnet()
    seed = RootSeed.read(config)  # Raises SeedFileError.NotFound if missing
    creds = Credentials.from_root_seed(seed)

    wallet = LexeWallet.load_or_fresh(config, creds)
    wallet.signup(seed)
    wallet.provision(creds)

    info = wallet.node_info()
    print(f"Balance: {info.balance_sats} sats")
"""

_set_method_doc(LexeWallet, "user_pk", """\
Get the user's hex-encoded public key.

Returns:
    The hex-encoded ed25519 user public key string.

Example::

    print(f"User PK: {wallet.user_pk()}")
""")

_set_method_doc(LexeWallet, "load", """\
Load an existing wallet from local state.

Raises :class:`LoadWalletError.NotFound` if no local data exists.
Use :meth:`LexeWallet.fresh` to create local state.

Args:
    env_config: Wallet environment configuration.
    credentials: Authentication credentials (see :class:`Credentials`).
    lexe_data_dir: Base data directory (default: ``~/.lexe``).

Returns:
    The loaded LexeWallet instance.

Raises:
    LoadWalletError.NotFound: If no local data exists.
    LoadWalletError.LoadFailed: If local data is corrupted.

Example::

    try:
        wallet = LexeWallet.load(config, creds)
    except LoadWalletError.NotFound:
        wallet = LexeWallet.fresh(config, creds)
""")

_set_method_doc(LexeWallet, "fresh", """\
Create a fresh wallet, deleting any existing local state for this user.

Data for other users and environments is not affected.

Args:
    env_config: Wallet environment configuration.
    credentials: Authentication credentials (see :class:`Credentials`).
    lexe_data_dir: Base data directory (default: ``~/.lexe``).

Returns:
    A new LexeWallet instance.

Raises:
    FfiError: If wallet creation fails.
""")

_set_method_doc(LexeWallet, "load_or_fresh", """\
Load an existing wallet, or create a fresh one if none exists.
If you are authenticating with client credentials, this is generally
what you want to use.

Args:
    env_config: Wallet environment configuration.
    credentials: Authentication credentials (see :class:`Credentials`).
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
    seed.write(config)  # Persist seed after signup!
""")

_set_method_doc(LexeWallet, "provision", """\
Ensures the wallet is provisioned to all recent trusted releases.

Call every time the wallet is loaded to keep the user running
the most up-to-date enclave software.

Args:
    credentials: Authentication credentials (see :class:`Credentials`).

Raises:
    FfiError: If provisioning fails.

Example::

    wallet = LexeWallet.load_or_fresh(config, creds)
    wallet.provision(creds)
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
    payer_note: Optional note received from the payer out-of-band via
        LNURL-pay that is stored with this payment. If provided, it must be
        non-empty and <= 200 chars / 512 UTF-8 bytes.

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
        If provided, it must be non-empty and <= 200 chars / 512 UTF-8 bytes.
    payer_note: Optional note that was sent to the receiver via LNURL-pay
        and is visible to them. If provided, it must be non-empty and <= 200
        chars / 512 UTF-8 bytes.

Returns:
    A :class:`PayInvoiceResponse` with the payment index and timestamp.

Raises:
    FfiError: If the invoice is invalid or payment initiation fails.

Example::

    resp = wallet.pay_invoice(bolt11_string)
    payment = wallet.wait_for_payment(resp.index)
    print(f"Payment {payment.status}")
""")

_set_method_doc(LexeWallet, "get_payment", """\
Get a specific payment by its index.

Args:
    index: Payment index string
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
    index: Payment index string.
    note: New note text, or ``None`` to clear.
        If provided, it must be non-empty and <= 200 chars / 512 UTF-8 bytes.

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
List payments from local storage with cursor-based pagination.

Reads from the local database only (no network calls).
Use :meth:`sync_payments` to fetch the latest data from the node
if needed.

Args:
    filter: Which payments to include:
        :attr:`PaymentFilter.ALL`,
        :attr:`PaymentFilter.PENDING`,
        :attr:`PaymentFilter.COMPLETED`,
        :attr:`PaymentFilter.FAILED`, or
        :attr:`PaymentFilter.FINALIZED` (completed or failed).
    order: Sort order (:attr:`Order.DESC` or :attr:`Order.ASC`).
        Defaults to ``DESC`` (newest first).
    limit: Maximum number of payments to return. Defaults to ``100``.
    after: Pagination cursor. Pass ``next_index`` from a previous
        response to get the next page. Defaults to ``None`` (first page).

Returns:
    A :class:`ListPaymentsResponse` with payments and a ``next_index``
    cursor for fetching the next page (``None`` if no more results).

Raises:
    FfiError: If ``after`` is not a valid payment index string.

Example::

    wallet.sync_payments()

    # First page
    resp = wallet.list_payments(PaymentFilter.ALL)
    for p in resp.payments:
        print(f"{p.index}: {p.amount_sats} sats ({p.status})")

    # Next page
    if resp.next_index is not None:
        resp = wallet.list_payments(PaymentFilter.ALL, after=resp.next_index)
""")

_set_method_doc(LexeWallet, "clear_payments", """\
Clear all local payment data for this wallet.

Clears the local payment cache only. Remote data on the node is
not affected. Call :meth:`sync_payments` to re-populate.

Raises:
    FfiError: If the local database cannot be cleared.
""")

_set_method_doc(LexeWallet, "wait_for_payment", """\
Wait for a payment to reach a terminal state (completed or failed).

Polls the node with exponential backoff until the payment finalizes
or the timeout is reached. Defaults to 10 minutes if not specified.
Maximum timeout is 86,400 seconds (24 hours).

Args:
    index: Payment index string.
    timeout_secs: Maximum wait time in seconds. Defaults to ``600``.
        Max: ``86400`` (24 hours).

Returns:
    The finalized :class:`Payment`.

Raises:
    FfiError: If the timeout is exceeded or the node is unreachable.

Example::

    resp = wallet.pay_invoice(invoice_str)
    payment = wallet.wait_for_payment(resp.index)
    assert payment.status in (PaymentStatus.COMPLETED, PaymentStatus.FAILED)
""")

# ======================= #
# --- AsyncLexeWallet --- #
# ======================= #

AsyncLexeWallet.__doc__ = """\
Async wallet handle for interacting with a Lexe Lightning node.

For synchronous usage, use :class:`LexeWallet`.

Example::

    from lexe import AsyncLexeWallet, Credentials, WalletConfig

    config = WalletConfig.mainnet()
    seed = RootSeed.read(config)
    creds = Credentials.from_root_seed(seed)

    wallet = AsyncLexeWallet.load_or_fresh(config, creds)
    await wallet.signup(seed)
    await wallet.provision(creds)

    info = await wallet.node_info()
    print(f"Balance: {info.balance_sats} sats")
"""

_set_method_doc(AsyncLexeWallet, "user_pk", """\
Get the user's hex-encoded public key.

Returns:
    The hex-encoded ed25519 user public key string.

Example::

    print(f"User PK: {wallet.user_pk()}")
""")

_set_method_doc(AsyncLexeWallet, "load", """\
Load an existing wallet from local state.

Raises :class:`LoadWalletError.NotFound` if no local data exists.
Use :meth:`AsyncLexeWallet.fresh` to create local state.

Args:
    env_config: Wallet environment configuration.
    credentials: Authentication credentials (see :class:`Credentials`).
    lexe_data_dir: Base data directory (default: ``~/.lexe``).

Returns:
    The loaded AsyncLexeWallet instance.

Raises:
    LoadWalletError.NotFound: If no local data exists.
    LoadWalletError.LoadFailed: If local data is corrupted.

Example::

    try:
        wallet = AsyncLexeWallet.load(config, creds)
    except LoadWalletError.NotFound:
        wallet = AsyncLexeWallet.fresh(config, creds)
""")

_set_method_doc(AsyncLexeWallet, "fresh", """\
Create a fresh wallet, deleting any existing local state for this user.

Data for other users and environments is not affected.

Args:
    env_config: Wallet environment configuration.
    credentials: Authentication credentials (see :class:`Credentials`).
    lexe_data_dir: Base data directory (default: ``~/.lexe``).

Returns:
    A new AsyncLexeWallet instance.

Raises:
    FfiError: If wallet creation fails.
""")

_set_method_doc(AsyncLexeWallet, "load_or_fresh", """\
Load an existing wallet, or create a fresh one if none exists.
If you are authenticating with client credentials, this is generally
what you want to use.

Args:
    env_config: Wallet environment configuration.
    credentials: Authentication credentials (see :class:`Credentials`).
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
    seed.write(config)  # Persist seed after signup!
""")

_set_method_doc(AsyncLexeWallet, "provision", """\
Ensures the wallet is provisioned to all recent trusted releases.

Call every time the wallet is loaded to keep the user running
the most up-to-date enclave software.

Args:
    credentials: Authentication credentials (see :class:`Credentials`).

Raises:
    FfiError: If provisioning fails.

Example::

    wallet = AsyncLexeWallet.load_or_fresh(config, creds)
    await wallet.provision(creds)
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
    payer_note: Optional note received from the payer out-of-band via
        LNURL-pay that is stored with this payment. If provided, it must be
        non-empty and <= 200 chars / 512 UTF-8 bytes.

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
        If provided, it must be non-empty and <= 200 chars / 512 UTF-8 bytes.
    payer_note: Optional note that was sent to the receiver via LNURL-pay
        and is visible to them. If provided, it must be non-empty and <= 200
        chars / 512 UTF-8 bytes.

Returns:
    A :class:`PayInvoiceResponse` with the payment index and timestamp.

Raises:
    FfiError: If the invoice is invalid or payment initiation fails.

Example::

    resp = await wallet.pay_invoice(bolt11_string)
    payment = await wallet.wait_for_payment(resp.index)
    print(f"Payment {payment.status}")
""")

_set_method_doc(AsyncLexeWallet, "get_payment", """\
Get a specific payment by its index.

Args:
    index: Payment index string
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
    index: Payment index string.
    note: New note text, or ``None`` to clear.
        If provided, it must be non-empty and <= 200 chars / 512 UTF-8 bytes.

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
List payments from local storage with cursor-based pagination.

Reads from the local database only (no network calls).
Use :meth:`sync_payments` to fetch the latest data from the node
if needed.

Args:
    filter: Which payments to include:
        :attr:`PaymentFilter.ALL`,
        :attr:`PaymentFilter.PENDING`,
        :attr:`PaymentFilter.COMPLETED`,
        :attr:`PaymentFilter.FAILED`, or
        :attr:`PaymentFilter.FINALIZED` (completed or failed).
    order: Sort order (:attr:`Order.DESC` or :attr:`Order.ASC`).
        Defaults to ``DESC`` (newest first).
    limit: Maximum number of payments to return. Defaults to ``100``.
    after: Pagination cursor. Pass ``next_index`` from a previous
        response to get the next page. Defaults to ``None`` (first page).

Returns:
    A :class:`ListPaymentsResponse` with payments and a ``next_index``
    cursor for fetching the next page (``None`` if no more results).

Raises:
    FfiError: If ``after`` is not a valid payment index string.

Example::

    await wallet.sync_payments()

    # First page
    resp = wallet.list_payments(PaymentFilter.ALL)
    for p in resp.payments:
        print(f"{p.index}: {p.amount_sats} sats ({p.status})")

    # Next page
    if resp.next_index is not None:
        resp = wallet.list_payments(PaymentFilter.ALL, after=resp.next_index)
""")

_set_method_doc(AsyncLexeWallet, "clear_payments", """\
Clear all local payment data for this wallet.

Clears the local payment cache only. Remote data on the node is
not affected. Call :meth:`sync_payments` to re-populate.

Raises:
    FfiError: If the local database cannot be cleared.
""")

_set_method_doc(AsyncLexeWallet, "wait_for_payment", """\
Wait for a payment to reach a terminal state (completed or failed).

Polls the node with exponential backoff until the payment finalizes
or the timeout is reached. Defaults to 10 minutes if not specified.
Maximum timeout is 86,400 seconds (24 hours).

Args:
    index: Payment index string.
    timeout_secs: Maximum wait time in seconds. Defaults to ``600``.
        Max: ``86400`` (24 hours).

Returns:
    The finalized :class:`Payment`.

Raises:
    FfiError: If the timeout is exceeded or the node is unreachable.

Example::

    resp = await wallet.pay_invoice(invoice_str)
    payment = await wallet.wait_for_payment(resp.index)
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
Filter for listing payments.

- **ALL** -- Include all payments.
- **PENDING** -- Only pending payments.
- **COMPLETED** -- Only completed payments.
- **FAILED** -- Only failed payments.
- **FINALIZED** -- Only finalized payments (completed or failed).
"""

lexe.Order.__doc__ = """\
Sort order for listing results.

- **ASC** -- Ascending order (oldest first).
- **DESC** -- Descending order (newest first).
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
    index: Unique payment identifier (``<created_at_ms>-<payment_id>``).
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

lexe.ClientPaymentId.__doc__ = """\
A unique, client-generated id for payment types (onchain send,
spontaneous send) that need an extra id for idempotency.
Its primary purpose is to prevent accidental double payments.

Methods:
    generate(): Generate a random ``ClientPaymentId``.
    to_bytes(): Return the 32-byte id.
    to_hex(): Encode the id as a 64-character hex string.

Example::

    payment_id = ClientPaymentId.generate()
    print(payment_id.to_hex())
"""

_set_method_doc(lexe.ClientPaymentId, "generate", """\
Generate a random ``ClientPaymentId``.

Returns:
    A new random ClientPaymentId.
""")

_set_method_doc(lexe.ClientPaymentId, "to_bytes", """\
Return the 32-byte id.

Returns:
    The id as raw bytes.
""")

_set_method_doc(lexe.ClientPaymentId, "to_hex", """\
Encode the id as a 64-character hex string.

Returns:
    The hex-encoded id string.
""")

lexe.PaymentSyncSummary.__doc__ = """\
Summary of a payment sync operation.

Attributes:
    num_new: Number of new payments added to the local DB.
    num_updated: Number of existing payments that were updated.
"""

lexe.ListPaymentsResponse.__doc__ = """\
Response from listing payments.

Attributes:
    payments: Payments in the requested page.
    next_index: Cursor for fetching the next page, or ``None``
        if there are no more results.
"""

# ================ #
# --- Invoices --- #
# ================ #

lexe.CreateInvoiceResponse.__doc__ = """\
Response from creating a Lightning invoice.

Attributes:
    index: Unique payment identifier for this invoice.
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
    index: Unique payment identifier for this payment.
    created_at_ms: When payment was initiated (ms since UNIX epoch).
"""

# ================= #
# --- FfiError  --- #
# ================= #

lexe.SeedFileError.__doc__ = """\
Error type for seedphrase file operations.

Raised by :meth:`RootSeed` file I/O methods (:meth:`~RootSeed.read`,
:meth:`~RootSeed.read_from_path`, :meth:`~RootSeed.write`,
:meth:`~RootSeed.write_to_path`).

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
        wallet = LexeWallet.load(config, creds)
    except LoadWalletError.NotFound:
        wallet = LexeWallet.fresh(config, creds)
    except LoadWalletError.LoadFailed as e:
        print(f"Load failed: {e.message}")
"""

lexe.FfiError.__doc__ = """\
Error type raised by SDK methods.

Catch this to handle Lexe SDK errors.

Methods:
    message(): Returns the error message string.

Example::

    try:
        info = wallet.node_info()
    except FfiError as e:
        print(f"SDK error: {e.message()}")
"""

_set_method_doc(lexe.FfiError, "message", """\
Returns the error message string.

Returns:
    A human-readable description of the error.
""")
