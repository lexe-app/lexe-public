"""Unit tests for the Lexe Python SDK.

These tests don't require a gateway or network connection.
"""

import tempfile

import lexe

from conftest import create_dev_config, create_test_root_seed


def test_enums():
    """Test enum types are properly exposed."""
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
    """Test RootSeed can be created."""
    seed = create_test_root_seed()
    assert len(seed.seed_bytes) == 32


def test_wallet_env_config():
    """Test WalletEnvConfig can be created."""
    config = create_dev_config()
    assert config.deploy_env() == lexe.DeployEnv.DEV
    assert config.network() == lexe.LxNetwork.REGTEST
    assert config.use_sgx() == False


def test_default_lexe_data_dir():
    """Test default_lexe_data_dir returns a path."""
    data_dir = lexe.default_lexe_data_dir()
    assert data_dir is not None
    assert len(data_dir) > 0
    assert data_dir.endswith(".lexe")


def test_seedphrase_path():
    """Test seedphrase_path returns environment-specific paths."""
    with tempfile.TemporaryDirectory() as temp_dir:
        # Mainnet config should return seedphrase.txt
        mainnet_config = lexe.WalletEnvConfig.mainnet()
        mainnet_path = mainnet_config.seedphrase_path(temp_dir)
        assert mainnet_path.endswith("seedphrase.txt")
        assert "prod" not in mainnet_path

        # Regtest config should return seedphrase.<env>.txt
        regtest_config = lexe.WalletEnvConfig.regtest()
        regtest_path = regtest_config.seedphrase_path(temp_dir)
        assert "seedphrase." in regtest_path
        assert regtest_path != mainnet_path


def test_init_logger():
    """Test init_logger can be called without error."""
    # Just verify it doesn't crash - logger is global state
    lexe.init_logger("warn")


def test_wallet_load_nonexistent():
    """Test loading nonexistent wallet returns None."""
    with tempfile.TemporaryDirectory() as temp_dir:
        config = create_dev_config()
        seed = create_test_root_seed()

        wallet = lexe.try_load_wallet(config, seed, temp_dir)
        assert wallet is None
