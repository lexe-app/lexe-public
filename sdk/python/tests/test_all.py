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

def test_wallet_load_nonexistent():
    """Test loading nonexistent wallet returns None"""
    with tempfile.TemporaryDirectory() as temp_dir:
        config = create_dev_config()
        seed = create_test_root_seed()

        wallet = lexe.try_load_wallet(config, seed, temp_dir)
        assert wallet is None
