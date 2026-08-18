#!/usr/bin/env python3
"""Compatibility entry point for the PHY6252 USB-UART flasher."""

from phyflash.cli import main


if __name__ == "__main__":
    raise SystemExit(main())
