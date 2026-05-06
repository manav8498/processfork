#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Model clients for PFBench.

Each client is a callable ``model(prompt: str, max_tokens: int) -> str``.
The harness loads one by name via :func:`load`.
"""

from __future__ import annotations

import os
from typing import Callable

ModelFn = Callable[[str, int], str]


def echo_model(prompt: str, max_tokens: int) -> str:
    """Trivial reference — returns the prompt verbatim. Self-test only."""
    del max_tokens
    return prompt


def openai_model(model_name: str = "gpt-4o") -> ModelFn:
    """OpenAI chat-completion client. Requires ``OPENAI_API_KEY``.

    Use::

        load("openai:gpt-4o")            # vanilla GPT-4o
        load("openai:gpt-4o-mini")       # cheaper variant for pilots
    """
    try:
        from openai import OpenAI  # type: ignore[import-not-found]
    except ImportError as e:
        raise RuntimeError(
            "openai package not installed; pip install openai>=1.0"
        ) from e

    api_key = os.environ.get("OPENAI_API_KEY")
    if not api_key:
        raise RuntimeError("OPENAI_API_KEY not set")
    client = OpenAI(api_key=api_key)

    def call(prompt: str, max_tokens: int) -> str:
        resp = client.chat.completions.create(
            model=model_name,
            messages=[{"role": "user", "content": prompt}],
            max_tokens=max_tokens,
            temperature=0.7,
        )
        return resp.choices[0].message.content or ""

    return call


def anthropic_model(model_name: str = "claude-opus-4-7") -> ModelFn:
    """Anthropic messages client. Requires ``ANTHROPIC_API_KEY``.

    Use::

        load("anthropic:claude-opus-4-7")
        load("anthropic:claude-haiku-4-5-20251001")
    """
    try:
        import anthropic  # type: ignore[import-not-found]
    except ImportError as e:
        raise RuntimeError(
            "anthropic package not installed; pip install anthropic>=0.34"
        ) from e

    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        raise RuntimeError("ANTHROPIC_API_KEY not set")
    client = anthropic.Anthropic(api_key=api_key)

    def call(prompt: str, max_tokens: int) -> str:
        resp = client.messages.create(
            model=model_name,
            max_tokens=max_tokens,
            messages=[{"role": "user", "content": prompt}],
        )
        # First content block is text for our prompt shape.
        block = resp.content[0]
        return getattr(block, "text", "") or ""

    return call


def load(spec: str) -> ModelFn:
    """Resolve ``"echo"``, ``"openai:gpt-4o"``, or
    ``"anthropic:claude-opus-4-7"`` into a callable."""
    if spec == "echo":
        return echo_model
    if spec.startswith("openai:"):
        return openai_model(spec.removeprefix("openai:"))
    if spec.startswith("anthropic:"):
        return anthropic_model(spec.removeprefix("anthropic:"))
    raise ValueError(
        f"unknown model spec {spec!r}; expected 'echo', "
        "'openai:<model>', or 'anthropic:<model>'"
    )
