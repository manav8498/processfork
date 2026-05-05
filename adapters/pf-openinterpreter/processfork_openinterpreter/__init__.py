# SPDX-License-Identifier: MIT
"""ProcessFork ↔ OpenInterpreter integration."""

from __future__ import annotations

from .wrapper import WrappedInterpreter, wrap_interpreter

__all__ = ["WrappedInterpreter", "wrap_interpreter"]
