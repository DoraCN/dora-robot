# dora-node-api Spike（E0）

- **status**: done — dependency fetchable, compiles, API surface confirmed from **cached source** (`dora-node-api-0.5.0`).
- **crate**: `dora-node-api` v0.5.0, locked **357 packages** (fetchable from crates.io).

## Confirmed API (for E1/E2/E3 wiring)

### Initialization (`node/mod.rs:115`)

```rust
use dora_node_api::DoraNode;

let (mut node, mut events) = DoraNode::init_from_env()?;  // ?? eyre::Result
```

- `init_from_env()` returns `eyre::Result<(DoraNode, EventStream)>`.
- `eyre::Report` is the error type (transitive dep of dora-node-api; no need to add `eyre` separately).
- `init_from_env_force()` also available (skips interactive fallback).

### Event loop (`EventStream`, `Stream` impl)

```rust
use futures::StreamExt;

while let Some(event) = events.next().await {
    // EventStream implements `futures::Stream`
}
```

- `EventStream` implements `futures::Stream`, so `.next().await` works (needs `futures::StreamExt` in scope).
- Synchronous fallback: `events.recv()` → `Option<Event>`, `events.recv_timeout(Duration)`.

### `Event` variants (`event_stream/event.rs:17`)

```rust
pub enum Event {
    Input {
        id: DataId,          // input ID from the dataflow yaml
        metadata: Metadata,  // key-value metadata (contains timestamp etc.)
        data: ArrowData,     // the actual payload
    },
    InputClosed { id: DataId },
    Stop,   // the dataflow is shutting down — exit the loop
    // ... other variants (errors, etc.)
}
```

- `DataId` = re-exported string-like ID type.
- `Metadata` supports `metadata.get("key")` / `TryFrom<HashMap>` etc.
- `ArrowData` is dora's Arrow wrapper; can be converted to concrete `arrow::array::Array` types.

### Sending outputs (`node/mod.rs:585`)

```rust
use arrow::array::Array;

node.send_output(
    "output_id".into(),           // DataId (accepts &str / String)
    Default::default(),            // MetadataParameters (can be empty)
    data,                          // impl arrow::array::Array
)?;
```

- `send_output(id, params, data: impl Array)` — for Arrow arrays (zero-copy into shared memory).
- `send_output_bytes(id, params, len, &[u8])` — for raw byte buffers.
- `send_output_raw`, `send_output_sample` — lower-level, for custom formats.

### Typical node pattern (template for E1/E2/E3)

```rust
use dora_node_api::{DoraNode, Event, EventStream};
use futures::StreamExt;
use arrow::array::Float32Array;

fn main() -> eyre::Result<()> {
    let (mut node, mut events) = DoraNode::init_from_env()?;

    while let Some(event) = events.next().await {
        match event {
            Event::Input { id, data, .. } => {
                // decode & process `data` ...
                // send output:
                let out: Float32Array = /* ... */;
                node.send_output("result".into(), Default::default(), out)?;
            }
            Event::Stop => break,
            _ => {}
        }
    }
    Ok(())
}
```

## Dependencies to add for E1/E2/E3

- `dora-node-api = "0.5.0"`
- `futures = "0.3"` (for `StreamExt`)
- `arrow = "59"` (for concrete array types)
- `eyre = "0.6"` (for error handling in node mains)

## Notes

- `dora-node-api@0.5.0` uses **arrow 59** — pin versions to match.
- The node binary name must match the `path` in the dataflow yaml (or use `cargo run` with args).
- Metadata from input events includes the `timestamp` (meta key `"open_timestamp"` or similar — verify with the running runtime).
- `ArrowData` conversion details: `arrow::array::Array::to_data()` to construct, `arrow::array::make_array(data)` to reconstruct — verify exact API against the running runtime.
