# training — dora-robot ⇄ LeRobot v3

Isolated Python subproject. It is the **downstream consumer** of the Rust/DORA
realtime teleoperation core and never runs on the control loop.

| Piece                  | What it does                                                              |
|------------------------|--------------------------------------------------------------------------|
| `tr_lerobot.recorder`  | DORA **Python node**: dataflow → **LeRobotDataset v3** via lerobot's writer |
| `tr_lerobot.schema`    | v3 feature schema (`action`, `observation.state`, `observation.images.*`) |
| `tr_lerobot.validate`  | loads the produced dataset back through `LeRobotDataset` (conformance gate) |
| `tr_lerobot.train`     | thin wrapper around lerobot's training entrypoint                         |

## Why a Python node (Option B)

LeRobotDataset **v3** stores many episodes per Parquet/MP4 file, resolves episode
boundaries through `meta/episodes/*` offset tables, and requires a final
`finalize()` to write parquet footers. Re-implementing that in Rust would be
fragile and chase a moving upstream format. Instead the recorder drives
**lerobot's own writer**, so v3 conformance is guaranteed by construction.

Inside one machine's DORA dataflow, nodes exchange Apache Arrow natively, so the
recorder consumes already-decoded arrays. The custom `tr-transport`/`Codec` is
only used on the *inter-machine* bridge hop.

> lerobot is **pinned here** (see `requirements.txt`) — it is **not** vendored
> into the Rust workspace as a git submodule.

## Setup

```sh
cd training
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt     # pinned lerobot (v3), dora-rs, pyarrow, numpy
pip install -e .                    # exposes tr-lerobot-validate / tr-lerobot-train
```

## Record (via DORA)

The recorder is wired into `../dataflows/record.yml`. From the project root:

```sh
dora up && dora start dataflows/record.yml
```

## Validate (conformance gate)

```sh
python -m tr_lerobot.validate --repo-id local/teleop --root ./datasets
```

## Train

```sh
python -m tr_lerobot.train --repo-id local/teleop --root ./datasets --policy act
```

> The exact `LeRobotDataset.create` / `add_frame` signatures and the training CLI
> flags are annotated with `NOTE:` in the source — confirm them against the
> pinned lerobot version before first use.
