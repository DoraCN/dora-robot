//! DORA bridge node — the communication middleware.
//!
//! Pumps opaque framed bytes between the local DORA dataflow and a pluggable
//! [`tr_transport::Transport`] (Loopback for tests, TCP/UDP, or ZenohTransport —
//! selected by config). It supports `control`, `feedback` and `episode` logical
//! channels with per-channel QoS.
//!
//! ```text
//! operator side   : dataflow `command`  → transport `control`
//!                   transport `feedback` → dataflow `feedback`
//!                   dataflow `episode`   → transport `episode`
//! robot side      : transport `control`  → dataflow `command`
//!                   dataflow `feedback`  → transport `feedback`
//!                   transport `episode` → dataflow `episode`  (→ recorder)
//! ```

use dora_node_api::DoraNode;
use futures::StreamExt;

fn main() -> eyre::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let (_node, mut events) = DoraNode::init_from_env()?;
        // The concrete transport and role (client/server) are selected by env/config.
        // Example: let mut transport = TcpTransport::connect("...")?;
        // In the event loop the bridge:
        //   - on dataflow `command` input → transport.send(Channel::Control, &bytes);
        //   - on dataflow `episode` input → transport.send(Channel::Episode, &bytes);
        //   - polls transport periodically → on frame → node.send_output(...).

        while let Some(_event) = events.next().await {
            // TODO: wire transport pump (J1 for zenoh; loopback for fast-gate).
        }
        Ok(())
    })
}
