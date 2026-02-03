"""Pytest configuration for Lexe SDK tests."""

import atexit
import asyncio
import json
import os
import tempfile
from functools import wraps
from typing import Optional

import pytest

import lexe


_TEMP_DIRS = []


def _cleanup_temp_dirs() -> None:
    for temp_dir in _TEMP_DIRS:
        temp_dir.cleanup()


atexit.register(_cleanup_temp_dirs)


def pytest_configure(config):
    """Register custom markers."""
    config.addinivalue_line(
        "markers",
        "integration: marks tests as integration tests (require gateway/network)"
    )


@pytest.fixture
def prefunded_wallets() -> Optional[dict]:
    """
    Fixture that provides pre-funded wallet info from Rust smoketest.
    Returns None if not running from smoketest context.

    The JSON file contains:
    - gateway_url: URL of the gateway
    - wallets: List of {seed_hex: str} for each pre-funded wallet
    """
    prefunded_file = os.getenv("PREFUNDED_WALLETS_FILE")
    if not prefunded_file or not os.path.exists(prefunded_file):
        return None

    with open(prefunded_file) as f:
        return json.load(f)


def load_prefunded_wallet(
    wallet_info: dict,
    gateway_url: str,
) -> lexe.LexeWallet:
    """
    Load a pre-funded wallet from smoketest-provided info.

    The wallet is already registered on the backend, so we use load_or_fresh
    which will load the existing wallet.

    Args:
        wallet_info: Dict with 'seed_hex' key containing the hex-encoded seed
        gateway_url: URL of the gateway to connect to

    Returns:
        A loaded LexeWallet instance
    """
    seed_bytes = bytes.fromhex(wallet_info["seed_hex"])
    seed = lexe.RootSeed(seed_bytes=seed_bytes)

    config = lexe.WalletEnvConfig.dev(gateway_url=gateway_url)

    # Use a temporary directory for local wallet data
    temp_dir = tempfile.TemporaryDirectory(prefix="lexe_prefunded_")
    _TEMP_DIRS.append(temp_dir)
    data_dir = temp_dir.name

    wallet = lexe.LexeWallet.load_or_fresh(config, seed, data_dir)
    if wallet is None:
        raise RuntimeError("Failed to load pre-funded wallet")

    return wallet


def async_timeout(seconds: float):
    """Decorator to add timeout to async test functions."""
    def decorator(func):
        @wraps(func)
        async def wrapper(*args, **kwargs):
            return await asyncio.wait_for(
                func(*args, **kwargs),
                timeout=seconds,
            )
        return wrapper
    return decorator
