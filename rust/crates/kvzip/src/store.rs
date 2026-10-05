use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    hash::{BuildHasher, RandomState},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    cache::ByteCache,
    codec::{decode_group, GroupEncoder},
    error::{Error, Result},
    format::{encode_record, parse_record, Record, HEADER},
};

pub const DEFAULT_MAX_SEGMENT_BYTES: u64 = 32 * 1024 * 1024;
const MIN_MAX_SEGMENT_BYTES: u64 = 1024;
/// GitHub rejects files above 100 MiB; staying under 100 MB holds for either reading of the
/// unit.
pub const MAX_MAX_SEGMENT_BYTES: u64 = 100_000_000;
const SEGMENT_EXTENSION: &str = "kvz";
/// Names the segment this directory's own writer appended to last, and its length then. The
/// file is git-ignored, so a clone or another worktree never has it.
const MARKER_NAME: &str = ".kvzip-writer";
const HEADER_LEN: u64 = HEADER.len() as u64;
/// A group stops taking values once it holds this many bytes of them, which bounds what `get`
/// decodes for one value.
const GROUP_RAW_TARGET: usize = 1 << 20;
/// Every group is coded against the first bytes of its segment's values, up to this many.
const BASE_MAX: usize = 1 << 20;
const CACHE_BYTES: usize = 64 << 20;

#[derive(Clone, Debug)]
pub struct Options {
    /// No segment file grows beyond this many bytes.
    pub max_segment_bytes: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_segment_bytes: DEFAULT_MAX_SEGMENT_BYTES,
        }
    }
}

/// A directory of segments. All methods take `&self` and may be called from several threads;
/// several stores, in one process or many, may use the same directory at once.
pub struct Store {
    dir: PathBuf,
    max_segment_bytes: u64,
    // Lock order: `writer`, then `state`, then `cache`.
    writer: Mutex<Option<Writer>>,
    state: RwLock<State>,
    cache: Mutex<ByteCache<CacheKey>>,
}

/// Decoded values of one group, or (`None`) the base of a segment.
type CacheKey = (usize, Option<usize>);

#[derive(Default)]
struct State {
    segments: Vec<Segment>,
    /// The segment whose name sorts last.
    newest: Option<usize>,
    index: HashMap<Box<[u8]>, Location>,
}

struct Segment {
    name: String,
    /// End of the last valid record read so far; 0 until the header has been validated.
    scanned: u64,
    /// Bytes of values in the records read so far.
    raw_total: usize,
    groups: Vec<Group>,
    /// Bytes of values in the last group.
    group_raw: usize,
}

struct Group {
    offset: u64,
    /// The group is coded against this many leading bytes of the segment's values.
    base_len: usize,
}

struct Location {
    segment: usize,
    group: usize,
    /// End of the record in the segment, which orders records of one segment.
    end: u64,
    raw_offset: usize,
    raw_len: usize,
}

struct Writer {
    /// `None` until the first record creates the segment, so a record that fits nowhere leaves
    /// no file behind.
    open: Option<(usize, File)>,
    len: u64,
    encoder: GroupEncoder,
    group_open: bool,
    group_raw: usize,
    base: Vec<u8>,
    marker: Option<File>,
}

impl Store {
    pub fn open(dir: impl AsRef<Path>, options: Options) -> Result<Self> {
        if !(MIN_MAX_SEGMENT_BYTES..=MAX_MAX_SEGMENT_BYTES).contains(&options.max_segment_bytes) {
            return Err(Error::InvalidOptions(format!(
                "max_segment_bytes must be between {MIN_MAX_SEGMENT_BYTES} and \
                 {MAX_MAX_SEGMENT_BYTES}"
            )));
        }
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        let store = Self {
            dir,
            max_segment_bytes: options.max_segment_bytes,
            writer: Mutex::new(None),
            state: RwLock::new(State::default()),
            cache: Mutex::new(ByteCache::new(CACHE_BYTES)),
        };
        store.refresh()?;
        Ok(store)
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let state = self.state.read().expect("state lock is not poisoned");
        let Some(location) = state.index.get(key) else {
            return Ok(None);
        };
        let raw = self.group_raw(&state, location)?;
        Ok(Some(
            raw[location.raw_offset..][..location.raw_len].to_vec(),
        ))
    }

    /// Stores `value` under `key`, replacing every value of the key this store has seen. Among
    /// records of a key that stores wrote without seeing each other's, which one wins is
    /// unspecified but the same for every reader.
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut writer = self.lock_writer();
        let mut adopt = true;
        loop {
            // A record in a segment that is no longer the newest would lose to the records of
            // its key that this store has seen in newer segments.
            let superseded = writer
                .as_ref()
                .and_then(|writer| writer.open.as_ref())
                .is_some_and(|(segment, _)| Some(*segment) != self.read_state().newest);
            if superseded {
                *writer = None;
                adopt = false;
            }
            if writer.is_none() {
                *writer = Some(self.open_writer(adopt)?);
            }
            let current = writer.as_mut().expect("the writer was just opened");
            let new_group = !current.group_open || current.group_raw >= GROUP_RAW_TARGET;
            if new_group {
                current
                    .encoder
                    .start_group(Arc::from(current.base.as_slice()));
            }
            let mut payload = Vec::new();
            current.encoder.push(value, &mut payload);
            let bytes = encode_record(new_group, key, value.len() as u64, &payload);
            let record_bytes = bytes.len() as u64;
            if current.len + record_bytes > self.max_segment_bytes {
                // The encoder has advanced past a record that is not written, so this writer
                // cannot continue its segment either way.
                let empty = current.len == HEADER_LEN;
                *writer = None;
                if empty {
                    return Err(Error::RecordTooLarge {
                        record_bytes: HEADER_LEN + record_bytes,
                        max_segment_bytes: self.max_segment_bytes,
                    });
                }
                adopt = false;
                continue;
            }
            let record = Record {
                new_group,
                key,
                raw_len: value.len() as u64,
                payload: &payload,
                len: bytes.len(),
            };
            let appended = self.append(current, &record, &bytes, value);
            if appended.is_err() {
                // A partial record may be on disk: leave the segment to readers, which stop
                // at it, and continue in a new one.
                *writer = None;
            }
            return appended;
        }
    }

    pub fn contains(&self, key: &[u8]) -> bool {
        self.read_state().index.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.read_state().index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn keys(&self) -> Vec<Vec<u8>> {
        let state = self.read_state();
        state.index.keys().map(|key| key.to_vec()).collect()
    }

    /// Picks up records that other stores wrote to the directory since the last call.
    pub fn refresh(&self) -> Result<()> {
        // Holding the writer keeps `put` from appending a record while it is being scanned,
        // which would apply it twice.
        let _writer = self.lock_writer();
        let mut state = self.state.write().expect("state lock is not poisoned");
        self.refresh_state(&mut state)
    }

    fn lock_writer(&self) -> MutexGuard<'_, Option<Writer>> {
        self.writer.lock().expect("writer lock is not poisoned")
    }

    fn read_state(&self) -> std::sync::RwLockReadGuard<'_, State> {
        self.state.read().expect("state lock is not poisoned")
    }

    fn lock_cache(&self) -> MutexGuard<'_, ByteCache<CacheKey>> {
        self.cache.lock().expect("cache lock is not poisoned")
    }

    fn refresh_state(&self, state: &mut State) -> Result<()> {
        let known: HashSet<&str> = state.segments.iter().map(|s| s.name.as_str()).collect();
        let mut added = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let path = entry?.path();
            let is_segment = path.extension().is_some_and(|ext| ext == SEGMENT_EXTENSION);
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                if is_segment && !known.contains(name) {
                    added.push(name.to_owned());
                }
            }
        }
        added.sort();
        for name in added {
            state.add_segment(Segment::new(name));
        }
        for index in 0..state.segments.len() {
            self.scan(state, index)?;
        }
        Ok(())
    }

    /// Reads the records appended to a segment since it was last scanned. A record that is
    /// incomplete or fails its checksum ends the scan: it is either still being written or the
    /// tail a crashed writer left, and the next scan retries from it.
    fn scan(&self, state: &mut State, index: usize) -> Result<()> {
        let segment = &state.segments[index];
        let mut file = File::open(self.dir.join(&segment.name))?;
        let len = file.metadata()?.len();
        if len <= segment.scanned.max(HEADER_LEN) {
            return Ok(());
        }
        let start = segment.scanned;
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(len - start).read_to_end(&mut bytes)?;
        let mut pos = 0;
        if start == 0 {
            if !bytes.starts_with(&HEADER) {
                return Ok(());
            }
            pos = HEADER.len();
            state.segments[index].scanned = HEADER_LEN;
        }
        while let Some(record) = parse_record(&bytes[pos..]) {
            if !state.apply(index, &record) {
                break;
            }
            pos += record.len;
        }
        Ok(())
    }

    fn group_raw(&self, state: &State, location: &Location) -> Result<Arc<[u8]>> {
        let key = (location.segment, Some(location.group));
        let needed = location.raw_offset + location.raw_len;
        // A cached group that has grown since it was decoded is too short for its new values.
        if let Some(raw) = self
            .lock_cache()
            .get(&key)
            .filter(|raw| raw.len() >= needed)
        {
            return Ok(raw);
        }
        let segment = &state.segments[location.segment];
        let group = &segment.groups[location.group];
        let base = self.base(state, location.segment, group.base_len)?;
        let end = segment.group_end(location.group);
        let bytes = read_range(&self.dir.join(&segment.name), group.offset, end)?;
        let raw: Arc<[u8]> = decode_records(&base[..group.base_len], &bytes)?.into();
        self.lock_cache().insert(key, raw.clone());
        Ok(raw)
    }

    /// The first `len` bytes of the values of a segment, at most `BASE_MAX`.
    fn base(&self, state: &State, segment_index: usize, len: usize) -> Result<Arc<[u8]>> {
        if len == 0 {
            return Ok(Arc::from([]));
        }
        let key = (segment_index, None);
        if let Some(base) = self.lock_cache().get(&key).filter(|base| base.len() >= len) {
            return Ok(base);
        }
        let segment = &state.segments[segment_index];
        // Groups that start once the base is full are coded against it and are not part of it.
        let group_count = segment
            .groups
            .iter()
            .take_while(|group| group.base_len < BASE_MAX)
            .count();
        let start = segment.groups[0].offset;
        let end = segment.group_end(group_count - 1);
        let bytes = read_range(&self.dir.join(&segment.name), start, end)?;
        let mut base = Vec::new();
        for index in 0..group_count {
            let range = (segment.groups[index].offset - start) as usize
                ..(segment.group_end(index) - start) as usize;
            let raw = decode_records(&base[..segment.groups[index].base_len], &bytes[range])?;
            base.extend_from_slice(&raw[..raw.len().min(BASE_MAX - base.len())]);
        }
        let base: Arc<[u8]> = base.into();
        self.lock_cache().insert(key, base.clone());
        Ok(base)
    }

    /// Continues the newest segment when `adopt` is set, the marker says that this directory's
    /// own writer left it at its current length, and no other store is writing to it; otherwise
    /// the first record will start a new one.
    ///
    /// The marker keeps every segment's history linear: a segment that arrived through git (a
    /// clone, another worktree, a checkout of another branch) is never appended to, so two
    /// branches cannot both change one segment. Only the newest segment is continued so that,
    /// across runs, a later record of a key always sorts after an earlier one.
    fn open_writer(&self, adopt: bool) -> Result<Writer> {
        let mut writer = Writer {
            open: None,
            len: HEADER_LEN,
            encoder: GroupEncoder::new(),
            group_open: false,
            group_raw: 0,
            base: Vec::new(),
            marker: None,
        };
        if !adopt {
            return Ok(writer);
        }
        let mut state = self.state.write().expect("state lock is not poisoned");
        self.refresh_state(&mut state)?;
        let Some(index) = state.newest else {
            return Ok(writer);
        };
        let segment = &state.segments[index];
        let written_here = self
            .read_marker()
            .is_some_and(|(name, len)| name == segment.name && len == segment.scanned);
        if !written_here || segment.scanned >= self.max_segment_bytes {
            return Ok(writer);
        }
        let file = OpenOptions::new()
            .append(true)
            .open(self.dir.join(&segment.name))?;
        // The length differs when the segment ends in an invalid record or another store
        // appended to it between the scan and the lock.
        if file.try_lock().is_ok() && file.metadata()?.len() == segment.scanned {
            let base_len = segment.raw_total.min(BASE_MAX);
            writer.base = self.base(&state, index, base_len)?[..base_len].to_vec();
            writer.len = segment.scanned;
            writer.open = Some((index, file));
        }
        Ok(writer)
    }

    fn append(
        &self,
        writer: &mut Writer,
        record: &Record,
        bytes: &[u8],
        value: &[u8],
    ) -> Result<()> {
        let mut state = self.state.write().expect("state lock is not poisoned");
        let segment = match &mut writer.open {
            Some((segment, file)) => {
                file.write_all(bytes)?;
                *segment
            }
            None => {
                let (name, mut file) = self.create_segment(&state)?;
                file.write_all(&[&HEADER, bytes].concat())?;
                let mut segment = Segment::new(name);
                segment.scanned = HEADER_LEN;
                let index = state.add_segment(segment);
                writer.open = Some((index, file));
                index
            }
        };
        let applied = state.apply(segment, record);
        assert!(applied, "a writer starts every segment with a new group");
        writer.len += bytes.len() as u64;
        writer.group_raw = value.len()
            + if record.new_group {
                0
            } else {
                writer.group_raw
            };
        writer.group_open = true;
        if writer.marker.is_none() {
            writer.marker = self.open_marker();
        }
        if let Some(mut marker) = writer.marker.as_ref() {
            // The text has a fixed length, so one write replaces it. A failure only leaves a
            // marker that matches no segment, which makes the next writer start a new one.
            let text = format!("{} {:020}\n", state.segments[segment].name, writer.len);
            let _ = marker
                .seek(SeekFrom::Start(0))
                .and_then(|_| marker.write_all(text.as_bytes()));
        }
        let base_room = BASE_MAX - writer.base.len();
        writer
            .base
            .extend_from_slice(&value[..value.len().min(base_room)]);
        Ok(())
    }

    fn read_marker(&self) -> Option<(String, u64)> {
        let text = fs::read_to_string(self.dir.join(MARKER_NAME)).ok()?;
        let (name, len) = text.trim_end().split_once(' ')?;
        Some((name.to_owned(), len.parse().ok()?))
    }

    /// Opens the marker after making sure git ignores it: a committed marker would let other
    /// checkouts append to the segment it names. `None` when either step fails, which only
    /// costs the next writer a new segment.
    fn open_marker(&self) -> Option<File> {
        let ignore_path = self.dir.join(".gitignore");
        let ignore_line = format!("/{MARKER_NAME}");
        let ignored = match fs::read_to_string(&ignore_path) {
            Ok(ignored) => ignored,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(_) => return None,
        };
        if !ignored.lines().any(|line| line == ignore_line) {
            let separator = if ignored.is_empty() || ignored.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            OpenOptions::new()
                .append(true)
                .create(true)
                .open(&ignore_path)
                .and_then(|mut file| {
                    file.write_all(format!("{separator}{ignore_line}\n").as_bytes())
                })
                .ok()?;
        }
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(self.dir.join(MARKER_NAME))
            .ok()
    }

    /// Creates a segment whose name sorts after every known one and locks it against other
    /// writers. The name starts with the creation time so that names order segments by age.
    fn create_segment(&self, state: &State) -> Result<(String, File)> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_micros() as u64);
        let newest = state
            .segments
            .iter()
            .filter_map(|segment| u64::from_str_radix(segment.name.get(..16)?, 16).ok())
            .max();
        let time = newest.map_or(now, |newest| now.max(newest.saturating_add(1)));
        loop {
            let random = RandomState::new().hash_one(time) as u32;
            let name = format!("{time:016x}-{random:08x}.{SEGMENT_EXTENSION}");
            let created = OpenOptions::new()
                .append(true)
                .create_new(true)
                .open(self.dir.join(&name));
            match created {
                Ok(file) => {
                    file.try_lock()
                        .map_err(|error| Error::Io(io::Error::other(error.to_string())))?;
                    return Ok((name, file));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl State {
    fn add_segment(&mut self, segment: Segment) -> usize {
        let index = self.segments.len();
        if self
            .newest
            .is_none_or(|newest| self.segments[newest].name < segment.name)
        {
            self.newest = Some(index);
        }
        self.segments.push(segment);
        index
    }

    /// Adds a record that follows the scanned part of its segment; `false` when the record
    /// cannot be placed, which leaves the state unchanged.
    fn apply(&mut self, segment_index: usize, record: &Record) -> bool {
        let segment = &mut self.segments[segment_index];
        let Ok(raw_len) = usize::try_from(record.raw_len) else {
            return false;
        };
        if record.new_group {
            segment.groups.push(Group {
                offset: segment.scanned,
                base_len: segment.raw_total.min(BASE_MAX),
            });
            segment.group_raw = 0;
        } else if segment.groups.is_empty() {
            return false;
        }
        let location = Location {
            segment: segment_index,
            group: segment.groups.len() - 1,
            end: segment.scanned + record.len as u64,
            raw_offset: segment.group_raw,
            raw_len,
        };
        segment.scanned = location.end;
        segment.group_raw += raw_len;
        segment.raw_total += raw_len;
        // The newest record of a key wins: segment names order by creation time and offsets
        // order within a segment.
        let newer = self.index.get(record.key).is_none_or(|current| {
            (&self.segments[current.segment].name, current.end)
                < (&self.segments[segment_index].name, location.end)
        });
        if newer {
            self.index.insert(record.key.into(), location);
        }
        true
    }
}

impl Segment {
    fn new(name: String) -> Self {
        Self {
            name,
            scanned: 0,
            raw_total: 0,
            groups: Vec::new(),
            group_raw: 0,
        }
    }

    fn group_end(&self, group: usize) -> u64 {
        self.groups
            .get(group + 1)
            .map_or(self.scanned, |next| next.offset)
    }
}

fn read_range(path: &Path, start: u64, end: u64) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; (end - start) as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Decodes the values of the records that make up one group.
fn decode_records(prefix: &[u8], bytes: &[u8]) -> Result<Vec<u8>> {
    let mut payloads = Vec::new();
    let mut raw_len = 0;
    let mut pos = 0;
    while pos < bytes.len() {
        let record = parse_record(&bytes[pos..])
            .ok_or_else(|| Error::Corrupt("a record changed after it was read".into()))?;
        payloads.push(record.payload);
        raw_len += record.raw_len as usize;
        pos += record.len;
    }
    decode_group(prefix, payloads.into_iter(), raw_len)
}
