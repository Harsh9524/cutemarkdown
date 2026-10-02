# Billing Event Store: Technical Design

![build](https://img.shields.io/badge/build-passing-brightgreen)

Companion to the [Migration Plan](./ai-report.md): schemas, interfaces and deployment. Jump back to the [rollback plan](./ai-report.md#rollback-plan) at any time.

## Overview

Commands enter through the API, are validated by an aggregate, and are appended to the event log. Projectors turn the log into read models.

![Request flow: API, events, Kafka, views](./img/diagram.png)

*Figure 1. Happy-path request flow.* The next image is intentionally missing, so the viewer should show a placeholder rather than a blank gap:

![Missing architecture diagram](./img/missing.png)

## Data Model

The `events` table is the single source of truth.

```sql
CREATE TABLE billing.events (
    event_id uuid PRIMARY KEY, stream_id uuid NOT NULL,
    version int NOT NULL, payload jsonb NOT NULL, UNIQUE (stream_id, version)
);
```

An event as it appears on the wire:

```json
{
  "event_id": "0b6f0c1e-7a3d-4c1e-9a55-2f1d3e7b8a90",
  "type": "InvoiceFinalized",
  "data": { "total": 12900, "currency": "EUR", "paid": false, "note": null }
}
```

## Core Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event { InvoiceDrafted { id: Uuid }, InvoiceFinalized { id: Uuid, total: Money } }
```

```python
def replay(events: Iterable[Event]) -> Invoice:
    return reduce(lambda state, e: state.apply(e), events, Invoice.empty())
```

```typescript
export interface Envelope<T extends string, D> { readonly type: T; readonly data: D }
```

```go
func (s *Store) Append(ctx context.Context, id uuid.UUID, expected int, evs []Event) error {
	if expected != s.version(id) { return ErrConcurrency }
	return s.write(ctx, id, evs)
}
```

```c
int append_event(store_t *s, const event_t *ev) {
    return (!s || !ev) ? -EINVAL : ring_push(&s->ring, ev, sizeof(*ev));
}
```

```java
public record InvoiceFinalized(UUID id, long totalCents) implements Event {
    public InvoiceFinalized { Objects.requireNonNull(id); }
}
```

## Operations

```bash
#!/usr/bin/env bash
set -euo pipefail
billing-ctl replay --tenant "${TENANT:?}" --from "$(date -u -d '1 day ago' +%FT%TZ)"
```

```sh
for t in acme globex initech; do billing-ctl status --tenant "$t" || exit 1; done
```

```powershell
Get-Content .\tenants.txt | ForEach-Object { & billing-ctl.exe status --tenant $_ }
```

## Configuration

```yaml
projector:
  workers: 16
  topics: [billing.events.v1]
```

```toml
[projector]
workers = 16
topics = ["billing.events.v1"]
```

```dockerfile
FROM rust:1.82-slim AS build
COPY . /src
RUN cargo build --release --manifest-path /src/Cargo.toml
```

```diff
--- a/config/pool.toml
+++ b/config/pool.toml
@@ -1,3 +1,3 @@
-max_connections = 20
+max_connections = 48
 min_connections = 4
```

## Dashboard Tile

```html
<section class="tile tile--ok"><h3>Parity</h3><p><strong>99.998%</strong> match</p></section>
```

```css
.tile--ok { border-left: 4px solid #16a34a; background: color-mix(in srgb, #16a34a 8%, white); }
```

## Plain Text and Overflow

A block with no language, such as a log excerpt:

```
2026-10-02T09:14:07Z INFO  projector  caught up stream=7c1e lag_ms=12
2026-10-02T09:14:08Z WARN  projector  slow batch size=500 took_ms=910
```

And one very long line (about 200 characters), to test horizontal scrolling inside a code block:

```text
BILLING_EVENT_STREAM_URL=postgres://billing_writer:not-a-real-password@billing-db-primary.internal.example.com:5432/billing?sslmode=verify-full&application_name=projector&connect_timeout=10&tz=UTC&a=1
```

---

Back to the [Migration Plan](./ai-report.md) · Jump to [Data Model](#data-model)
