#!/usr/bin/env python3
"""Generate samples/long.md (~5,000 lines, 40 sections) for performance testing.

Deterministic: the same output every run. Usage:  python3 gen_long.py [sections]
"""
import random
import sys
from pathlib import Path

SECTIONS = int(sys.argv[1]) if len(sys.argv) > 1 else 40
rng = random.Random(20261002)

TOPICS = [
    "Request Routing", "Cache Invalidation", "Schema Evolution", "Backpressure",
    "Idempotency Keys", "Rate Limiting", "Feature Flags", "Observability",
    "Retry Budgets", "Connection Pooling", "Blue/Green Deploys", "Secrets Rotation",
    "Queue Partitioning", "Data Retention", "Index Design", "Batch Windows",
    "Circuit Breakers", "Config Drift", "Load Shedding", "Snapshot Strategy",
    "Access Control", "Audit Logging", "Webhook Delivery", "Clock Skew",
    "Schema Registry", "Blob Storage", "Search Relevance", "Session Handling",
    "Cost Allocation", "Capacity Planning", "Incident Review", "Chaos Testing",
    "Release Trains", "Dependency Pinning", "Build Caching", "Contract Testing",
    "Pagination", "Time Zones", "Locale Handling", "Graceful Shutdown",
]

SUBJECTS = ["The service", "Each worker", "The scheduler", "A client", "The gateway",
            "Our pipeline", "The control plane", "Every replica", "The batch job", "The proxy"]
VERBS = ["retries", "buffers", "validates", "batches", "rejects", "replays", "compacts",
         "forwards", "signs", "samples", "throttles", "snapshots"]
OBJECTS = ["incoming requests", "stale entries", "the pending queue", "duplicate events",
           "outbound webhooks", "expired tokens", "partial writes", "hot partitions",
           "oversized payloads", "idle connections"]
TAILS = ["before the deadline expires", "when the queue is saturated", "under sustained load",
         "after a leader election", "once the circuit closes again", "during a rolling deploy",
         "without blocking the event loop", "using exponential backoff with jitter",
         "and records a metric for every attempt", "while preserving ordering per key"]

LANGS = ["python", "rust", "typescript", "go", "bash", "sql", "yaml", "json", "toml", "c", "java", "diff"]


def sentence():
    return f"{rng.choice(SUBJECTS)} {rng.choice(VERBS)} {rng.choice(OBJECTS)} {rng.choice(TAILS)}."


def paragraph(n=None):
    n = n or rng.randint(3, 6)
    return " ".join(sentence() for _ in range(n))


def code(lang, i):
    n = i + 1
    samples = {
        "python": f"def handle_{n}(event: dict) -> bool:\n    if event.get('retries', 0) > 3:\n        return False\n    return process(event)",
        "rust": f"fn handle_{n}(ev: &Event) -> Result<(), Error> {{\n    ensure!(ev.retries <= 3, Error::TooManyRetries);\n    process(ev)\n}}",
        "typescript": f"export async function handle{n}(ev: Event): Promise<boolean> {{\n  if (ev.retries > 3) return false;\n  return process(ev);\n}}",
        "go": f"func Handle{n}(ev Event) error {{\n\tif ev.Retries > 3 {{\n\t\treturn ErrTooManyRetries\n\t}}\n\treturn process(ev)\n}}",
        "bash": f"#!/usr/bin/env bash\nset -euo pipefail\nfor shard in $(seq 1 {n}); do\n  ./run-shard.sh \"$shard\"\ndone",
        "sql": f"SELECT shard_id, count(*) AS pending\nFROM jobs_{n}\nWHERE state = 'pending'\nGROUP BY shard_id\nORDER BY pending DESC;",
        "yaml": f"worker_{n}:\n  replicas: {n % 7 + 1}\n  limits:\n    cpu: \"500m\"\n    memory: 512Mi",
        "json": f'{{\n  "id": {n},\n  "state": "pending",\n  "tags": ["a", "b", "c"]\n}}',
        "toml": f"[worker_{n}]\nreplicas = {n % 7 + 1}\nbackoff = \"exponential\"",
        "c": f"int handle_{n}(const event_t *ev) {{\n    if (ev->retries > 3) return -1;\n    return process(ev);\n}}",
        "java": f"boolean handle{n}(Event ev) {{\n    if (ev.retries() > 3) return false;\n    return process(ev);\n}}",
        "diff": f"--- a/config_{n}.toml\n+++ b/config_{n}.toml\n@@ -1,2 +1,2 @@\n-retries = 3\n+retries = {n % 5 + 3}",
    }
    return f"```{lang}\n{samples[lang]}\n```"


def table(i):
    rows = ["| Metric | Baseline | Current | Delta | Status |", "| :--- | ---: | ---: | ---: | :---: |"]
    for r in range(rng.randint(8, 12)):
        base = rng.randint(50, 900)
        cur = base + rng.randint(-120, 120)
        delta = cur - base
        status = "✅" if delta <= 0 else "⚠️"
        rows.append(f"| `metric_{i}_{r}` | {base} ms | {cur} ms | {delta:+d} ms | {status} |")
    return "\n".join(rows)


def bullets():
    items = [f"- **{rng.choice(VERBS).title()}:** {sentence()}" for _ in range(rng.randint(4, 7))]
    # one nested pair
    items.insert(2, f"  - {sentence()}")
    items.insert(3, f"    - {sentence()}")
    return "\n".join(items)


def numbered():
    return "\n".join(f"{k}. {sentence()}" for k in range(1, rng.randint(5, 8)))


def tasks():
    return "\n".join(f"- [{'x' if rng.random() < 0.5 else ' '}] {sentence()}" for _ in range(rng.randint(3, 5)))


def section(i):
    title = TOPICS[i % len(TOPICS)]
    out = [f"## {i + 1}. {title}", "", paragraph(), ""]
    out += [f"> [!NOTE]\n> {sentence()}", ""] if i % 5 == 0 else [f"> {sentence()}", ""]
    for j in range(4):
        out += [f"### {i + 1}.{j + 1} {rng.choice(OBJECTS).title()} and {rng.choice(OBJECTS).title()}", ""]
        out += [paragraph(), ""]
        out += [bullets() if j % 2 == 0 else numbered(), ""]
        out += [paragraph(2), ""]
        if j == 0:
            out += [code(LANGS[(i + j) % len(LANGS)], i), ""]
        if j == 1:
            out += [table(i), ""]
        if j == 2:
            out += [tasks(), ""]
            out += [code(LANGS[(i + 5) % len(LANGS)], i + 100), ""]
        if j == 3:
            out += [table(i + 50), "", code(LANGS[(i + 9) % len(LANGS)], i + 200), ""]
        out += [paragraph(), ""]
    out += ["---", ""]
    return out


lines = ["---", "title: Long Document (performance test)", "generated: true", "---", "",
         "# Long Document for Performance Testing", "",
         f"This file is generated by `gen_long.py` and contains {SECTIONS} sections of mixed content. "
         "Scroll, search, jump through the outline, and resize the window to check responsiveness.", ""]
lines += ["## Contents", ""]
lines += [f"- [{i + 1}. {TOPICS[i % len(TOPICS)]}](#{i + 1}-{TOPICS[i % len(TOPICS)].lower().replace('/', '').replace(' ', '-')})"
          for i in range(SECTIONS)]
lines += [""]
for i in range(SECTIONS):
    lines += section(i)

target = Path(__file__).resolve().parent / "long.md"
target.write_text("\n".join(lines).rstrip("\n") + "\n", encoding="utf-8")
print(f"wrote {target} ({target.read_text(encoding='utf-8').count(chr(10))} lines, {SECTIONS} sections)")
