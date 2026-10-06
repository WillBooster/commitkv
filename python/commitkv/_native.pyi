from os import PathLike
from typing import final

@final
class Store:
    """A directory of compressed segments, none of which exceeds ``max_segment_bytes``
    (32 MiB by default; ``ValueError`` unless between 1,024 and 100,000,000).

    A ``str`` key or value stands for its UTF-8 encoding. A store may be shared by threads,
    and several stores, in one process or many, may use the same directory at once.

    Every method may raise ``OSError`` for an I/O failure and ``RuntimeError`` for a corrupt
    store.
    """

    def __init__(
        self, directory: str | PathLike[str], *, max_segment_bytes: int = ...
    ) -> None: ...
    def get(self, key: str | bytes) -> bytes | None: ...
    def put(self, key: str | bytes, value: str | bytes) -> None:
        """Stores a value, replacing an earlier one.

        Raises ``ValueError`` when the record does not fit in a segment.
        """

    def __contains__(self, key: str | bytes) -> bool: ...
    def __len__(self) -> int: ...
    def keys(self) -> list[bytes]: ...
    def refresh(self) -> None:
        """Picks up records that other stores wrote to the directory."""
