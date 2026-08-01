#!/usr/bin/env python3
"""Evaluate HybridRoute decision accuracy and latency using a JSONL dataset."""

from __future__ import annotations

import argparse
import json
import statistics
import time
import urllib.error
import urllib.request
from pathlib import Path


def percentile(values: list[float], fraction: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, round((len(ordered) - 1) * fraction)))
    return ordered[index]


def post_json(url: str, payload: dict) -> tuple[dict, float]:
    encoded = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=encoded,
        headers={"content-type": "application/json"},
        method="POST",
    )
    started = time.perf_counter()
    with urllib.request.urlopen(request, timeout=10) as response:
        body = json.loads(response.read().decode("utf-8"))
    return body, (time.perf_counter() - started) * 1000.0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--dataset",
        default="benchmarks/sample-routing.jsonl",
        help="JSONL file containing request and expected_route fields",
    )
    parser.add_argument(
        "--url",
        default="http://localhost:8088/v1/route",
        help="HybridRoute decision endpoint",
    )
    args = parser.parse_args()

    rows = [
        json.loads(line)
        for line in Path(args.dataset).read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    correct = 0
    latencies: list[float] = []
    confusion: dict[tuple[str, str], int] = {}

    for index, row in enumerate(rows, start=1):
        expected = row.pop("expected_route")
        row.setdefault("sticky_key", f"evaluation-{index}")
        try:
            response, latency_ms = post_json(args.url, row)
        except (urllib.error.URLError, TimeoutError) as error:
            raise SystemExit(f"request {index} failed: {error}") from error

        actual = (response.get("selected") or {}).get("route_id", "<no-match>")
        latencies.append(latency_ms)
        correct += int(actual == expected)
        confusion[(expected, actual)] = confusion.get((expected, actual), 0) + 1
        marker = "PASS" if actual == expected else "FAIL"
        print(f"{marker:4} expected={expected:20} actual={actual:20} {latency_ms:8.2f} ms")

    accuracy = correct / len(rows) if rows else 0.0
    print("\nSummary")
    print(f"cases:    {len(rows)}")
    print(f"accuracy: {accuracy:.2%}")
    print(f"p50:      {statistics.median(latencies) if latencies else 0.0:.2f} ms")
    print(f"p95:      {percentile(latencies, 0.95):.2f} ms")
    print(f"p99:      {percentile(latencies, 0.99):.2f} ms")

    errors = [(pair, count) for pair, count in confusion.items() if pair[0] != pair[1]]
    if errors:
        print("\nMisroutes")
        for (expected, actual), count in sorted(errors):
            print(f"{expected} -> {actual}: {count}")
    return 0 if correct == len(rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
