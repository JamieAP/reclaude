from __future__ import annotations

import sys
import time
from pathlib import Path

from .cli_constants import GEMINI_MIN_INTERVAL, GEMINI_RPM_LIMIT
from .log import get_logger

# Gemini rate limiting state
_gemini_client = None
_gemini_last_call: float = 0.0
_gemini_call_times: list[float] = []


def _get_gemini_client():
    """Get or create a Gemini client using the new google.genai SDK."""
    global _gemini_client

    if _gemini_client is None:
        try:
            from google import genai

            # Try to get API key from config file
            api_key_file = Path.home() / ".config" / "gemini-api-key"
            if api_key_file.exists():
                api_key = api_key_file.read_text().strip()
                _gemini_client = genai.Client(api_key=api_key)
            else:
                return None
        except ImportError:
            return None

    return _gemini_client


def _rate_limit_gemini(verbose: bool = False) -> None:
    """Enforce rate limiting for Gemini API calls."""
    global _gemini_last_call, _gemini_call_times
    now = time.time()

    # Clean old timestamps (older than 60s)
    _gemini_call_times = [t for t in _gemini_call_times if now - t < 60]

    # If we've made too many calls in the last minute, wait
    if len(_gemini_call_times) >= GEMINI_RPM_LIMIT:
        oldest = min(_gemini_call_times)
        wait_time = 60 - (now - oldest) + 1
        if wait_time > 0:
            print(
                f"    RPM limit ({len(_gemini_call_times)}/{GEMINI_RPM_LIMIT}): waiting {wait_time:.1f}s...",
                file=sys.stderr,
            )
            time.sleep(wait_time)

    # Ensure minimum interval between calls
    elapsed = now - _gemini_last_call
    if elapsed < GEMINI_MIN_INTERVAL:
        wait = GEMINI_MIN_INTERVAL - elapsed
        if verbose and wait > 1:
            print(f"    Interval wait: {wait:.1f}s...", file=sys.stderr)
        time.sleep(wait)

    _gemini_last_call = time.time()
    _gemini_call_times.append(_gemini_last_call)


def _call_gemini_sdk(
    model_name: str,
    prompt: str,
    data: str,
    verbose: bool = True,
    max_retries: int = 3,
    respect_rate_limit: bool = True,
    temperature: float = 0.3,
    max_output_tokens: int | None = None,
) -> tuple[str, bool]:
    """Call Gemini using Python SDK.

    Args:
        model_name: Model to use (gemini-3-flash-preview or gemini-3-pro-preview)
        prompt: System/instruction prompt
        data: User data to process
        verbose: Print progress to stderr
        max_retries: Max retry attempts
        respect_rate_limit: Whether to enforce rate limiting
        temperature: 0.0-1.0, lower = more deterministic (default 0.3 for summaries)
        max_output_tokens: Max output tokens (default: model default)

    Returns:
        Tuple of (output_text, success_bool)
    """
    from google.genai import types

    client = _get_gemini_client()
    if client is None:
        return "ERROR: Gemini SDK not available (check ~/.config/gemini-api-key)", False

    # Generation config.
    config_kwargs = {
        "temperature": temperature,
        "top_p": 0.95,
        "top_k": 40,
    }
    if max_output_tokens:
        config_kwargs["max_output_tokens"] = max_output_tokens
    gen_config = types.GenerateContentConfig(**config_kwargs)

    for attempt in range(max_retries):
        if respect_rate_limit:
            _rate_limit_gemini(verbose=verbose)

        try:
            call_start = time.time()
            if verbose:
                print(f"    {model_name}...", file=sys.stderr, end="", flush=True)

            full_prompt = f"{prompt}\n\n---\n\n{data}"
            response = client.models.generate_content(
                model=model_name,
                contents=full_prompt,
                config=gen_config,
            )
            call_duration = time.time() - call_start

            if verbose:
                print(f" ({call_duration:.1f}s)", file=sys.stderr)

            if response.text:
                output = response.text.strip()
                # Sanity check for large inputs - warn on expansion
                if len(data) > 500 and len(output) > len(data) * 2:
                    print(
                        f"    Warning: Expansion: {len(data)} -> {len(output)} (expected compression)",
                        file=sys.stderr,
                    )

                # Log successful API call
                log = get_logger()
                log.info(
                    "gemini_call",
                    model=model_name,
                    input_chars=len(data),
                    output_chars=len(output),
                    duration_s=round(call_duration, 2),
                    temperature=temperature,
                    compression_ratio=round(len(output) / len(data), 3) if len(data) > 0 else None,
                    success=True,
                )
                return output, True
            else:
                if attempt < max_retries - 1:
                    print(
                        f"    Warning: Empty response, retrying ({attempt + 1}/{max_retries})...",
                        file=sys.stderr,
                    )
                    time.sleep(2)
                    continue
                get_logger().warning("gemini_call", model=model_name, error="empty_response", success=False)
                return "ERROR: Empty response", False

        except Exception as e:
            error_str = str(e)
            if verbose:
                print(f" ERROR: {error_str[:50]}", file=sys.stderr)
            get_logger().error(
                "gemini_call",
                model=model_name,
                error=error_str[:200],
                attempt=attempt + 1,
                success=False,
            )

            # Check for rate limit errors
            if "429" in error_str or "quota" in error_str.lower() or "rate" in error_str.lower():
                wait = (attempt + 1) * 30
                print(
                    f"    Warning: Rate limited, waiting {wait}s ({attempt + 1}/{max_retries})...",
                    file=sys.stderr,
                )
                time.sleep(wait)
                continue

            if attempt < max_retries - 1:
                print(f"    Warning: Retrying ({attempt + 1}/{max_retries})...", file=sys.stderr)
                time.sleep(3)
                continue

            return f"ERROR: {e}", False

    return "ERROR: Max retries exceeded", False
