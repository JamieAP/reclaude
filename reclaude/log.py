"""
Unified structured logging for reclaude.

Outputs JSON Lines to ~/.reclaude/reclaude.jsonl for all important events.
"""

from __future__ import annotations

import logging
from pathlib import Path

import structlog

LOG_DIR = Path.home() / ".reclaude"
JSONL_LOG = LOG_DIR / "reclaude.jsonl"

# Module-level logger instance
_logger: structlog.BoundLogger | None = None


def setup_logging() -> structlog.BoundLogger:
    """Configure structlog for JSON logging."""
    LOG_DIR.mkdir(parents=True, exist_ok=True)

    # File handler for JSON logs
    file_handler = logging.FileHandler(JSONL_LOG, mode="a")
    file_handler.setLevel(logging.INFO)

    # Configure stdlib logging (avoid duplicate handlers)
    root = logging.getLogger()
    if not any(isinstance(h, logging.FileHandler) and h.baseFilename == str(JSONL_LOG) for h in root.handlers):
        root.addHandler(file_handler)
        root.setLevel(logging.INFO)

    # Configure structlog
    structlog.configure(
        processors=[
            structlog.processors.TimeStamper(fmt="iso"),
            structlog.processors.add_log_level,
            structlog.processors.JSONRenderer(),
        ],
        wrapper_class=structlog.BoundLogger,
        context_class=dict,
        logger_factory=structlog.PrintLoggerFactory(file=file_handler.stream),
        cache_logger_on_first_use=True,
    )

    return structlog.get_logger()


def get_logger() -> structlog.BoundLogger:
    """Get or create the global structured logger."""
    global _logger
    if _logger is None:
        _logger = setup_logging()
    return _logger


def log_event(event: str, **kwargs) -> None:
    """Log a structured event. Convenience wrapper."""
    get_logger().info(event, **kwargs)
