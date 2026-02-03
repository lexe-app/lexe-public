import os
import tempfile
import pytest
import lexe
from conftest import load_prefunded_wallet


# --- Helper functions --- #

def create_test_root_seed() -> lexe.RootSeed:
    """Create a test root seed (insecure, for testing only)"""
    seed_bytes = b"test_seed_for_python_sdk_1234567"
    return lexe.RootSeed(seed_bytes=seed_bytes)


def create_dev_config() -> lexe.WalletEnvConfig:
    """Create a dev environment config for testing"""
    gateway_url = os.getenv("GATEWAY_URL")
    if gateway_url:
        return lexe.WalletEnvConfig.dev(gateway_url=gateway_url)
    return lexe.WalletEnvConfig.dev()


# --- Unit tests --- #

def test_enums():
    """Test enum types are properly exposed"""
    # DeployEnv
    assert lexe.DeployEnv.DEV
    assert lexe.DeployEnv.STAGING
    assert lexe.DeployEnv.PROD

    # LxNetwork
    assert lexe.LxNetwork.MAINNET
    assert lexe.LxNetwork.TESTNET4
    assert lexe.LxNetwork.REGTEST
    assert lexe.LxNetwork.TESTNET3
    assert lexe.LxNetwork.SIGNET

    # PaymentDirection
    assert lexe.PaymentDirection.INBOUND
    assert lexe.PaymentDirection.OUTBOUND
    assert lexe.PaymentDirection.INFO

    # PaymentRail
    assert lexe.PaymentRail.ONCHAIN
    assert lexe.PaymentRail.INVOICE
    assert lexe.PaymentRail.OFFER
    assert lexe.PaymentRail.SPONTANEOUS
    assert lexe.PaymentRail.WAIVED_FEE
    assert lexe.PaymentRail.UNKNOWN

    # PaymentStatus
    assert lexe.PaymentStatus.PENDING
    assert lexe.PaymentStatus.COMPLETED
    assert lexe.PaymentStatus.FAILED

    # ConfirmationPriority
    assert lexe.ConfirmationPriority.HIGH
    assert lexe.ConfirmationPriority.NORMAL
    assert lexe.ConfirmationPriority.BACKGROUND

    # PaymentFilter
    assert lexe.PaymentFilter.ALL
    assert lexe.PaymentFilter.PENDING
    assert lexe.PaymentFilter.FINALIZED


def test_root_seed_creation():
    """Test RootSeed can be created"""
    seed = create_test_root_seed()
    assert len(seed.seed_bytes) == 32


def test_wallet_env_config():
    """Test WalletEnvConfig can be created"""
    config = create_dev_config()
    assert config.deploy_env() == lexe.DeployEnv.DEV
    assert config.network() == lexe.LxNetwork.REGTEST
    assert config.use_sgx() == False


# --- Wallet tests --- #

@pytest.mark.integration
def test_wallet_fresh():
    """Test creating a fresh wallet (requires gateway)"""
    with tempfile.TemporaryDirectory() as temp_dir:
        config = create_dev_config()
        seed = create_test_root_seed()

        wallet = lexe.LexeWallet.fresh(config, seed, temp_dir)
        assert wallet is not None


def test_wallet_load_nonexistent():
    """Test loading nonexistent wallet returns None"""
    with tempfile.TemporaryDirectory() as temp_dir:
        config = create_dev_config()
        seed = create_test_root_seed()

        wallet = lexe.try_load_wallet(config, seed, temp_dir)
        assert wallet is None


@pytest.mark.integration
def test_wallet_fresh_and_load():
    """Test creating fresh wallet and then loading it (requires gateway)"""
    with tempfile.TemporaryDirectory() as temp_dir:
        config = create_dev_config()
        seed = create_test_root_seed()

        # Create fresh wallet
        wallet1 = lexe.LexeWallet.fresh(config, seed, temp_dir)
        assert wallet1 is not None

        # Load the same wallet
        wallet2 = lexe.try_load_wallet(config, seed, temp_dir)
        assert wallet2 is not None


@pytest.mark.integration
def test_wallet_load_or_fresh():
    """Test load_or_fresh creates wallet if not found (requires gateway)"""
    with tempfile.TemporaryDirectory() as temp_dir:
        config = create_dev_config()
        seed = create_test_root_seed()

        wallet = lexe.LexeWallet.load_or_fresh(config, seed, temp_dir)
        assert wallet is not None


# --- Integration tests (require regtest) --- #

@pytest.mark.integration
@pytest.mark.asyncio
async def test_wallet_node_info():
    """Test getting node info from a provisioned wallet"""
    with tempfile.TemporaryDirectory() as temp_dir:
        config = create_dev_config()
        seed = create_test_root_seed()

        wallet = lexe.LexeWallet.fresh(config, seed, temp_dir)

        # Sign up and provision
        await wallet.signup_and_provision(seed, None)

        # Get node info
        info = await wallet.node_info()

        assert info.version != ""
        assert info.user_pk != ""
        assert info.node_pk != ""
        assert info.balance_sats >= 0
        assert info.lightning_balance_sats >= 0
        assert info.onchain_balance_sats >= 0


@pytest.mark.integration
@pytest.mark.asyncio
async def test_create_and_pay_invoice(prefunded_wallets):
    """Test creating and paying invoices between two pre-funded wallets."""
    # Test constants
    poll_timeout_secs = 120
    test_invoice_amount_sats = 1000
    test_invoice_expiration_secs = 3600

    if prefunded_wallets is None:
        pytest.skip("Requires pre-funded wallets from Rust smoketest")

    if len(prefunded_wallets["wallets"]) < 2:
        pytest.skip("Requires at least 2 pre-funded wallets")

    gateway_url = prefunded_wallets["gateway_url"]

    # Load pre-funded wallets
    wallet1 = load_prefunded_wallet(
        prefunded_wallets["wallets"][0],
        gateway_url,
    )
    wallet2 = load_prefunded_wallet(
        prefunded_wallets["wallets"][1],
        gateway_url,
    )

    # Verify wallets have Lightning balance
    info1 = await wallet1.node_info()
    info2 = await wallet2.node_info()
    assert info1.lightning_balance_sats > 0, \
        "Wallet 1 has no Lightning balance: " \
        f"{info1.lightning_balance_sats}"
    assert info2.lightning_balance_sats > 0, \
        "Wallet 2 has no Lightning balance: " \
        f"{info2.lightning_balance_sats}"

    # Create invoice on wallet2
    create_resp = await wallet2.create_invoice(
        expiration_secs=test_invoice_expiration_secs,
        amount_sats=test_invoice_amount_sats,
        description="Test payment from Python SDK"
    )

    assert create_resp.invoice != ""
    assert create_resp.amount_sats == test_invoice_amount_sats
    assert create_resp.payment_index != ""
    assert create_resp.created_at_ms > 0
    assert create_resp.expires_at_ms >= create_resp.created_at_ms

    # Pay invoice from wallet1
    pay_resp = await wallet1.pay_invoice(
        invoice=create_resp.invoice,
        fallback_amount_sats=None,
        note="Paying test invoice from Python SDK"
    )
    assert pay_resp.payment_index != ""
    assert pay_resp.created_at_ms > 0

    # Wait for payment to complete using SDK polling method
    payer_payment = await wallet1.wait_for_payment_completion(
        payment_index=pay_resp.payment_index,
        timeout_secs=poll_timeout_secs,
    )

    assert payer_payment.status == lexe.PaymentStatus.COMPLETED, \
        f"Payment status is {payer_payment.status}, expected COMPLETED"
    assert payer_payment.amount_sats == test_invoice_amount_sats, (
        "Payment amount is "
        f"{payer_payment.amount_sats}, expected {test_invoice_amount_sats}"
    )


@pytest.mark.integration
@pytest.mark.asyncio
async def test_payment_sync():
    """Test syncing payments from node"""
    with tempfile.TemporaryDirectory() as temp_dir:
        config = create_dev_config()
        seed = create_test_root_seed()

        wallet = lexe.LexeWallet.fresh(config, seed, temp_dir)
        await wallet.signup_and_provision(seed, None)

        # Sync payments
        summary = await wallet.sync_payments()

        assert summary.num_new >= 0
        assert summary.num_updated >= 0


@pytest.mark.integration
@pytest.mark.asyncio
async def test_update_payment_note():
    """Test updating a payment note"""
    with tempfile.TemporaryDirectory() as temp_dir:
        config = create_dev_config()
        seed = create_test_root_seed()

        wallet = lexe.LexeWallet.fresh(config, seed, temp_dir)
        await wallet.signup_and_provision(seed, None)

        # Create an invoice to get a payment index
        create_resp = await wallet.create_invoice(
            expiration_secs=3600,
            amount_sats=1000,
            description="Test"
        )

        # Sync payments from node to local storage before updating notes.
        await wallet.sync_payments()

        # Update the payment note
        await wallet.update_payment_note(
            payment_index=create_resp.payment_index,
            note="Updated note for test payment"
        )

        # Verify the note was updated
        payment = await wallet.get_payment(create_resp.payment_index)
        assert payment is not None
        assert payment.note == "Updated note for test payment"


@pytest.mark.integration
@pytest.mark.asyncio
async def test_list_payments():
    """Test listing payments from local storage"""
    with tempfile.TemporaryDirectory() as temp_dir:
        config = create_dev_config()
        seed = create_test_root_seed()

        wallet = lexe.LexeWallet.fresh(config, seed, temp_dir)
        await wallet.signup_and_provision(seed, None)

        # Create an invoice to have at least one payment
        await wallet.create_invoice(
            expiration_secs=3600,
            amount_sats=1000,
            description="Test"
        )

        # Sync payments from node to local storage
        await wallet.sync_payments()

        # List all payments
        response = wallet.list_payments(
            filter=lexe.PaymentFilter.ALL,
            offset=0,
            limit=10
        )

        assert response.total_count >= 1
        assert len(response.payments) >= 1

        # Test with Pending filter
        pending_response = wallet.list_payments(
            filter=lexe.PaymentFilter.PENDING,
            offset=0,
            limit=10
        )
        assert pending_response.total_count >= 0


@pytest.mark.integration
@pytest.mark.asyncio
async def test_ensure_provisioned():
    """Test ensuring wallet is provisioned to latest enclave"""
    with tempfile.TemporaryDirectory() as temp_dir:
        config = create_dev_config()
        seed = create_test_root_seed()

        wallet = lexe.LexeWallet.fresh(config, seed, temp_dir)

        # First signup and provision
        await wallet.signup_and_provision(seed, None)

        # Now call ensure_provisioned - should succeed since we just provisioned
        await wallet.ensure_provisioned(seed)

        # Verify wallet still works after ensure_provisioned
        info = await wallet.node_info()
        assert info.version != ""
        assert info.user_pk != ""


# --- Error case tests --- #

@pytest.mark.integration
@pytest.mark.asyncio
async def test_pay_invalid_invoice_error(prefunded_wallets):
    """Test that paying a malformed BOLT11 invoice returns an FfiError"""
    if prefunded_wallets is None:
        pytest.skip("Requires pre-funded wallets from Rust smoketest")

    gateway_url = prefunded_wallets["gateway_url"]
    wallet = load_prefunded_wallet(prefunded_wallets["wallets"][0], gateway_url)

    with pytest.raises(lexe.FfiError) as exc_info:
        await wallet.pay_invoice(
            invoice="lnbc1invalid",
            fallback_amount_sats=None,
            note=None
        )

    # Verify error message is specific about the invalid invoice.
    error_msg = exc_info.value.message().lower()
    assert "invalid invoice" in error_msg


@pytest.mark.integration
@pytest.mark.asyncio
async def test_get_payment_invalid_format(prefunded_wallets):
    """Test that getting a payment with invalid format raises FfiError"""
    if prefunded_wallets is None:
        pytest.skip("Requires pre-funded wallets from Rust smoketest")

    gateway_url = prefunded_wallets["gateway_url"]
    wallet = load_prefunded_wallet(prefunded_wallets["wallets"][0], gateway_url)

    # Invalid format payment index (not "<timestamp>-ln_<hex32>" format)
    invalid_payment_index = "fake_payment_id"

    # Should raise FfiError for invalid format
    with pytest.raises(lexe.FfiError) as exc_info:
        await wallet.get_payment(invalid_payment_index)

    # Verify error message is specific about the payment index.
    error_msg = exc_info.value.message().lower()
    assert "invalid payment_index" in error_msg


@pytest.mark.integration
@pytest.mark.asyncio
async def test_get_payment_valid_format_nonexistent(prefunded_wallets):
    """Test that getting a nonexistent payment with valid format returns None"""
    if prefunded_wallets is None:
        pytest.skip("Requires pre-funded wallets from Rust smoketest")

    gateway_url = prefunded_wallets["gateway_url"]
    wallet = load_prefunded_wallet(prefunded_wallets["wallets"][0], gateway_url)

    # Valid format: <timestamp>-ln_<hex32> (timestamp + 32-byte hex ID)
    valid_format_payment_index = f"1234567890-ln_{'0' * 64}"

    # Should return None for nonexistent but valid format
    payment = await wallet.get_payment(valid_format_payment_index)
    assert payment is None
