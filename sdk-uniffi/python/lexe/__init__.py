from .lexe import *

__doc__ = lexe.__doc__
if hasattr(lexe, "__all__"):
    __all__ = lexe.__all__

# Apply Python-specific docstring enrichments over the
# language-agnostic UniFFI-generated docstrings.
from . import _docs as _docs  # noqa: F401, E402
