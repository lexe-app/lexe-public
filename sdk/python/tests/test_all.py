import pytest
import lexe

def test_add():
    assert lexe.add(1, 2) == 3

def test_hex_encode():
    assert lexe.hex_encode(b"hello") == "68656c6c6f"
