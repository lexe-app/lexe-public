import pytest
import sdk


def test_sum_as_string():
    assert sdk.sum_as_string(1, 1) == "2"
