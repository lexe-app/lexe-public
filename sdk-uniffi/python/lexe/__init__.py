from .lexe import *  # noqa: F403

__doc__ = lexe.__doc__

# Re-export BlockingLexeWallet as the default `LexeWallet`.
# Sync is the natural default for Python scripts.
from .lexe import BlockingLexeWallet as LexeWallet  # noqa: E402, F811

LexeWallet.__name__ = "LexeWallet"
LexeWallet.__qualname__ = "LexeWallet"
LexeWallet.__module__ = "lexe"

__all__ = [*lexe.__all__, "LexeWallet"] if hasattr(lexe, "__all__") else ["LexeWallet"]

# Convert selected no-arg methods into @property descriptors.
from . import _preprocess as _preprocess  # noqa: F401, E402


# Apply Python-specific docstring enrichments over the
# language-agnostic UniFFI-generated docstrings.
from . import _docs as _docs  # noqa: F401, E402
