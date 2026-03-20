"""Preprocessing transforms applied to the generated UniFFI bindings.

Converts selected no-arg methods into read-only ``@property`` descriptors
for a more Pythonic API (``obj.attr`` instead of ``obj.attr()``).

Imported by ``__init__.py`` on package load, before ``_docs.py``.
"""

from . import lexe


def _make_property(cls: type, name: str, doc: str) -> None:
    """Convert a no-arg method to a read-only ``@property`` with a docstring.

    UniFFI doesn't generate ``@property`` decorators, so we monkey-patch
    them here for a more Pythonic API (``obj.attr`` instead of
    ``obj.attr()``).
    """
    original = getattr(cls, name)
    setattr(cls, name, property(original, doc=doc))


# --- WalletConfig properties --- #

_make_property(lexe.WalletConfig, "deploy_env", "The configured deployment environment.")
_make_property(lexe.WalletConfig, "network", "The configured Bitcoin network.")
_make_property(lexe.WalletConfig, "use_sgx", "Whether SGX is enabled for this config.")
_make_property(lexe.WalletConfig, "gateway_url", """\
The gateway URL for this environment.

Returns ``None`` for dev configs without a gateway URL override.
""")

# --- Wallet properties --- #

_make_property(lexe.BlockingLexeWallet, "user_pk", """\
The user's hex-encoded ed25519 public key derived from the root seed.
""")

_make_property(lexe.AsyncLexeWallet, "user_pk", """\
The user's hex-encoded ed25519 public key derived from the root seed.
""")
