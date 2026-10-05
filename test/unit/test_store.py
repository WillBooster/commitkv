import os
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest

from kvzip import Store


def test_values_written_as_str_and_bytes_survive_reopening(tmp_path: Path) -> None:
    binary = bytes([0, 255, 128, 10])
    store = Store(tmp_path)
    store.put("text", "こんにちは")
    store.put(binary, binary)
    store.put("text", "replaced")

    reopened = Store(str(tmp_path))
    assert reopened.get("text") == b"replaced"
    assert reopened.get(binary) == binary
    assert reopened.get("missing") is None
    assert "text" in reopened and b"text" in reopened and "missing" not in reopened
    assert len(reopened) == 2
    assert sorted(reopened.keys()) == [binary, b"text"]


def test_a_store_sees_what_another_store_wrote_after_refresh(tmp_path: Path) -> None:
    reader = Store(tmp_path)
    Store(tmp_path).put("key", "value")
    assert reader.get("key") is None
    reader.refresh()
    assert reader.get("key") == b"value"


def test_threads_share_a_store(tmp_path: Path) -> None:
    store = Store(tmp_path)

    def write(thread: int) -> None:
        for i in range(200):
            store.put(f"{thread}-{i}", f"value {thread} {i}")

    with ThreadPoolExecutor(max_workers=8) as executor:
        list(executor.map(write, range(8)))
    reopened = Store(tmp_path)
    assert len(reopened) == 1600
    assert reopened.get("7-199") == b"value 7 199"


def test_a_record_that_does_not_fit_is_rejected_and_no_segment_exceeds_the_limit(
    tmp_path: Path,
) -> None:
    max_segment_bytes = 4096
    store = Store(tmp_path, max_segment_bytes=max_segment_bytes)
    with pytest.raises(ValueError, match="segment"):
        store.put("huge", os.urandom(max_segment_bytes))
    for i in range(20):
        store.put(f"key-{i}", os.urandom(1000))
    sizes = [path.stat().st_size for path in tmp_path.glob("*.kvz")]
    assert len(sizes) > 1
    assert max(sizes) <= max_segment_bytes
    with pytest.raises(ValueError, match="max_segment_bytes"):
        Store(tmp_path, max_segment_bytes=1)
