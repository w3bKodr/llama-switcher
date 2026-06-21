#!/usr/bin/env python3
"""Dependency-free CLI for the Llama Switcher Hermes skill."""

import argparse
import json
import os
import sys
import urllib.error
import urllib.request


BASE_URL = os.environ.get(
    "LLAMA_SWITCHER_BASE_URL", "http://127.0.0.1:47891"
).rstrip("/")
TOKEN = os.environ.get("LLAMA_SWITCHER_API_TOKEN", "")


def request(path, method="GET", body=None):
    if not TOKEN:
        raise RuntimeError(
            "LLAMA_SWITCHER_API_TOKEN is not configured. Reinstall the skill "
            "from Llama Switcher > Agent Control."
        )

    data = json.dumps(body).encode("utf-8") if body is not None else None
    req = urllib.request.Request(
        BASE_URL + path,
        data=data,
        method=method,
        headers={
            "Authorization": "Bearer " + TOKEN,
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        try:
            detail = json.loads(detail).get("error", detail)
        except json.JSONDecodeError:
            pass
        raise RuntimeError(
            "Llama Switcher API error {}: {}".format(error.code, detail)
        ) from error
    except urllib.error.URLError as error:
        raise RuntimeError(
            "Llama Switcher is not reachable at {}. Start the tray app first."
            .format(BASE_URL)
        ) from error


def main():
    parser = argparse.ArgumentParser(description="Control Llama Switcher")
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ("status", "profiles", "rescan", "restart", "stop", "open-dashboard"):
        sub.add_parser(command)
    alias = sub.add_parser("switch-alias")
    alias.add_argument("alias")
    name = sub.add_parser("switch-name")
    name.add_argument("model")
    name.add_argument("feature")
    args = parser.parse_args()

    calls = {
        "status": ("/status", "GET", None),
        "profiles": ("/profiles", "GET", None),
        "rescan": ("/rescan", "POST", None),
        "restart": ("/restart", "POST", None),
        "stop": ("/stop", "POST", None),
        "open-dashboard": ("/open-dashboard", "POST", None),
        "switch-alias": (
            "/switch-by-alias", "POST", {"alias": getattr(args, "alias", None)}
        ),
        "switch-name": (
            "/switch-by-name",
            "POST",
            {
                "model": getattr(args, "model", None),
                "feature": getattr(args, "feature", None),
            },
        ),
    }
    print(json.dumps(request(*calls[args.command]), indent=2))


if __name__ == "__main__":
    try:
        main()
    except RuntimeError as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)

