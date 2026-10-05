# Segment format (version 1)

Committed caches hold segments forever, so a store must keep reading every segment a released
version wrote. A change to a rule a reader depends on needs a new version byte and a reader for
the old one; the rules that only writers follow say so. A segment whose header is not the one
below contributes no records and is never appended to.

## Directory

A store is a directory of segment files named `<time>-<random>.kvz`:

- `<time>`: 16 lowercase hex digits, the creation time in microseconds since the Unix epoch,
  raised when necessary so that the name sorts after every segment the creating store knows.
- `<random>`: 8 lowercase hex digits.

Entries with another extension are not segments, and neither is an entry that is not a regular
file. A store never follows a symbolic link in its directory.

## Segment

A segment is the 8-byte header `kvzip\0\0\x01` (the last byte is the version) followed by
records:

| Field          | Encoding                                        |
| -------------- | ----------------------------------------------- |
| kind           | 1 byte: `1` starts a group, `0` continues it    |
| key length     | varint                                          |
| key            | bytes                                           |
| value length   | varint                                          |
| payload length | varint                                          |
| payload        | bytes                                           |
| CRC-32         | 4 bytes little-endian, of all the fields before |

A varint is an unsigned LEB128 integer of at most 64 bits. The first record of a segment starts
a group.

A reader stops at the first record that is incomplete or fails its checksum, and at a record
that continues a group when no group has started. The bytes from there on are either a record
still being written or the remains of a crashed write; a store never appends to a segment that
ends in such bytes.

## Groups

The payloads of a group, concatenated in order, form one zstd frame that is never closed. Its
content is the concatenation of the group's values. The writer flushes the frame after every
value (`ZSTD_e_flush`), so the payloads up to a record decode to exactly the values up to it.

The frame of a group is coded against a prefix (`ZSTD_CCtx_refPrefix`): the first `BASE_MAX`
(1 MiB) bytes of the values stored in the segment before the group, or all of them when there
are fewer. Decoding a group therefore needs the groups at the start of its segment, up to the
one in which the values reach `BASE_MAX`, and no other.

Frames use a window of 4 MiB (`windowLog` 22); a decoder must allow it and rejects a larger one.

Where groups start is the writer's choice, which readers take from the kind byte and which may
change without a new version: a store starts a group when it begins to append to a segment and
when the current group holds at least `GROUP_RAW_TARGET` (1 MiB) of values.

## Which record of a key is current

The record in the segment whose name sorts last; within a segment, the record at the highest
offset.

## Which segment a store appends to

No reader depends on the rules of this section: they only keep writers from changing a segment
on two branches, and may change without a new version.

A segment is only ever appended to, by the store that holds an exclusive advisory lock (`flock`)
on it. A store starts appending to an existing segment only when all of these hold; otherwise it
creates one. It stops appending to a segment once it sees one whose name sorts later.

- The segment's name sorts last in the directory.
- `.kvzip-writer` names the segment and its current length. The file holds
  `<segment name> <length as 20 decimal digits>\n`. A store rewrites it before every record
  with the length the segment will have, and does not write the record when that fails.
- The segment ends in a valid record, is shorter than the limit, and its lock is free.

A store lists `/.kvzip-writer` in the directory's `.gitignore` before it creates the file, so the
file never travels through git. A segment is therefore extended only in the directory that
created it and only from the state that directory left it in: all versions of a segment that
ever exist are prefixes of one another, and two branches cannot both change one segment.

`put` fails when `.gitignore` or `.kvzip-writer` is a symbolic link, without writing a record.
