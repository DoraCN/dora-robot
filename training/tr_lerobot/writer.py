"""`EpisodeWriter` — the data-handoff seam between this project and lerobot.

The recorder hands frames + episode decisions to an ``EpisodeWriter``; the
production implementation wraps lerobot's v3 dataset writer. This isolates **our
handoff logic** from **lerobot's persistence**, and lets tests inject a *spy*
writer so the recorder logic is testable with **zero torch**.

Persistence / encoding / v3 layout is **lerobot's domain — not this project**
(see `docs/specs/001-so101-teleop-record/spec.md` §5/§6).
"""

from __future__ import annotations

from typing import Protocol, runtime_checkable

from .schema import DatasetSpec


@runtime_checkable
class EpisodeWriter(Protocol):
    """Seam: this project only *calls* these; lerobot does the actual writing."""

    def add_frame(self, frame: dict) -> None:
        """Hand one frame (already in lerobot's expected shape) to the writer."""

    def save_episode(self) -> None:
        """Keep the buffered episode (operator marked **success**)."""

    def discard(self) -> None:
        """Drop the buffered episode (operator marked **fail/rerecord**)."""

    def finalize(self) -> None:
        """Flush buffered metadata + close (mandatory in v3)."""


class LerobotEpisodeWriter:
    """Production `EpisodeWriter` backed by lerobot's `LeRobotDataset` v3 writer.

    lerobot is imported **lazily** so this module imports / py_compiles without
    torch; only constructing this class pulls lerobot.
    """

    def __init__(
        self,
        spec: DatasetSpec,
        repo_id: str,
        root: str,
        robot_type: str = "generic",
        *,
        use_videos: bool = True,
        streaming_encoding: bool = True,
        encoder_threads: int | None = None,
        encoder_queue: int = 30,
        batch_encoding: int = 1,
    ) -> None:
        from lerobot.datasets import LeRobotDataset  # noqa: PLC0415

        # ── Date-based archive: create if new, load if exists ──
        dataset_root = f"{root}/{repo_id}"
        try:
            self._dataset = LeRobotDataset.create(
                repo_id=repo_id,
                fps=spec.fps,
                root=root,
                robot_type=robot_type,
                features=spec.features(),
                use_videos=use_videos,
                streaming_encoding=streaming_encoding,
                encoder_threads=encoder_threads,
                encoder_queue_maxsize=encoder_queue,
                batch_encoding_size=batch_encoding,
            )
        except FileExistsError:
            from pathlib import Path
            self._dataset = LeRobotDataset(
                repo_id=repo_id,
                root=root,
            )

    def add_frame(self, frame: dict) -> None:
        self._dataset.add_frame(frame)

    def save_episode(self) -> None:
        self._dataset.save_episode()

    def discard(self) -> None:
        # lerobot's own "drop the in-progress take" (rerecord model).
        self._dataset.clear_episode_buffer()

    def finalize(self) -> None:
        self._dataset.finalize()
