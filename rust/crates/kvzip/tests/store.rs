use std::{fs, path::Path, thread};

use kvzip::{Error, Options, Store};

const SMALL_SEGMENT: u64 = 64 * 1024;

#[test]
fn values_survive_reopening_and_the_latest_value_of_a_key_wins() {
    let dir = tempfile::tempdir().unwrap();
    let large = noise(1, 3 << 20);
    {
        let store = open(dir.path());
        for i in 0..2000 {
            store.put(&key(i), &document(i)).unwrap();
        }
        store.put(b"empty", b"").unwrap();
        store.put(b"large", &large).unwrap();
        store.put(&key(7), b"replaced in the first run").unwrap();
        assert_eq!(
            store.get(&key(7)).unwrap().unwrap(),
            b"replaced in the first run"
        );
    }
    {
        let store = open(dir.path());
        store.put(&key(8), b"replaced in the second run").unwrap();
    }
    let store = open(dir.path());
    assert_eq!(store.len(), 2002);
    assert_eq!(store.get(b"empty").unwrap().unwrap(), b"");
    assert_eq!(store.get(b"large").unwrap().unwrap(), large);
    assert_eq!(
        store.get(&key(7)).unwrap().unwrap(),
        b"replaced in the first run"
    );
    assert_eq!(
        store.get(&key(8)).unwrap().unwrap(),
        b"replaced in the second run"
    );
    for i in (0..2000).filter(|i| ![7, 8].contains(i)) {
        assert_eq!(store.get(&key(i)).unwrap().unwrap(), document(i));
    }
    assert_eq!(store.get(b"missing").unwrap(), None);
    assert!(store.contains(&key(0)) && !store.contains(b"missing"));
    let mut keys = store.keys();
    keys.sort();
    assert_eq!(keys.len(), 2002);
    assert!(keys.binary_search(&b"large".to_vec()).is_ok());
}

#[test]
fn no_segment_exceeds_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let options = Options {
        max_segment_bytes: SMALL_SEGMENT,
    };
    let store = Store::open(dir.path(), options.clone()).unwrap();
    // Incompressible values of every size up to just under the limit.
    let lengths: Vec<usize> = (0..300)
        .map(|i| (i * 211) % (SMALL_SEGMENT as usize - 100))
        .collect();
    for (i, &len) in lengths.iter().enumerate() {
        store.put(&key(i), &noise(i as u64, len)).unwrap();
    }
    let sizes = segment_sizes(dir.path());
    assert!(sizes.len() > 100, "values this large need many segments");
    assert!(sizes.iter().all(|&size| size <= SMALL_SEGMENT), "{sizes:?}");
    let reopened = Store::open(dir.path(), options).unwrap();
    for (i, &len) in lengths.iter().enumerate() {
        assert_eq!(
            reopened.get(&key(i)).unwrap().unwrap(),
            noise(i as u64, len)
        );
    }
}

#[test]
fn a_record_larger_than_a_segment_is_rejected_without_leaving_a_trace() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(
        dir.path(),
        Options {
            max_segment_bytes: SMALL_SEGMENT,
        },
    )
    .unwrap();
    store.put(b"before", b"kept").unwrap();
    let error = store
        .put(b"huge", &noise(0, SMALL_SEGMENT as usize))
        .unwrap_err();
    assert!(matches!(error, Error::RecordTooLarge { .. }), "{error}");
    store.put(b"after", b"kept too").unwrap();
    assert_eq!(store.get(b"huge").unwrap(), None);
    assert_eq!(store.get(b"before").unwrap().unwrap(), b"kept");
    assert_eq!(store.get(b"after").unwrap().unwrap(), b"kept too");
    assert!(segment_sizes(dir.path())
        .iter()
        .all(|&size| size <= SMALL_SEGMENT));
}

#[test]
fn threads_and_stores_sharing_a_directory_lose_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let options = Options {
        max_segment_bytes: SMALL_SEGMENT,
    };
    let stores = [
        Store::open(dir.path(), options.clone()).unwrap(),
        Store::open(dir.path(), options.clone()).unwrap(),
    ];
    thread::scope(|scope| {
        for (s, store) in stores.iter().enumerate() {
            for t in 0..4 {
                scope.spawn(move || {
                    for i in 0..300 {
                        let id = s * 10_000 + t * 1000 + i;
                        store.put(&key(id), &document(id)).unwrap();
                        assert_eq!(store.get(&key(id)).unwrap().unwrap(), document(id));
                    }
                });
            }
        }
    });
    assert!(segment_sizes(dir.path())
        .iter()
        .all(|&size| size <= SMALL_SEGMENT));
    stores[0].refresh().unwrap();
    for store in [&stores[0], &Store::open(dir.path(), options).unwrap()] {
        assert_eq!(store.len(), 2400);
        for s in 0..2 {
            for t in 0..4 {
                for i in 0..300 {
                    let id = s * 10_000 + t * 1000 + i;
                    assert_eq!(store.get(&key(id)).unwrap().unwrap(), document(id));
                }
            }
        }
    }
}

#[test]
fn a_torn_tail_hides_only_the_record_it_belongs_to() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = open(dir.path());
        for i in 0..50 {
            store.put(&key(i), &document(i)).unwrap();
        }
    }
    let segment = fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = fs::read(&segment).unwrap();
    fs::write(&segment, &bytes[..bytes.len() - 3]).unwrap();
    {
        let store = open(dir.path());
        assert_eq!(store.len(), 49);
        assert_eq!(store.get(&key(49)).unwrap(), None);
        store.put(&key(49), &document(49)).unwrap();
    }
    let store = open(dir.path());
    for i in 0..50 {
        assert_eq!(store.get(&key(i)).unwrap().unwrap(), document(i));
    }
}

#[test]
fn similar_values_share_context_within_and_across_runs() {
    let raw: usize = (0..600).map(|i| document(i).len()).sum();

    let one_run = tempfile::tempdir().unwrap();
    let store = open(one_run.path());
    for i in 0..600 {
        store.put(&key(i), &document(i)).unwrap();
    }
    let stored: u64 = segment_sizes(one_run.path()).iter().sum();
    assert!(
        stored * 5 < raw as u64,
        "one run stored {stored} of {raw} bytes"
    );

    let many_runs = tempfile::tempdir().unwrap();
    for i in 0..600 {
        open(many_runs.path()).put(&key(i), &document(i)).unwrap();
    }
    let sizes = segment_sizes(many_runs.path());
    assert_eq!(sizes.len(), 1, "runs continue the newest segment");
    assert!(
        sizes[0] * 5 < raw as u64,
        "many runs stored {} of {raw} bytes",
        sizes[0]
    );
}

fn open(dir: &Path) -> Store {
    Store::open(dir, Options::default()).unwrap()
}

fn segment_sizes(dir: &Path) -> Vec<u64> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .collect()
}

fn key(id: usize) -> Vec<u8> {
    format!("key-{id}").into_bytes()
}

/// A value that shares a long template with every other document and differs in its details,
/// like the prompts of one application.
fn document(id: usize) -> Vec<u8> {
    let details = noise(id as u64, 96)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "You are a support agent. Answer the question using only the reference material below, \
         cite the section you used, and reply in the language of the question.\n\
         ## Reference\n{}\n## Question {id}\n{details}\n",
        "The warranty covers manufacturing defects for twenty-four months from delivery. "
            .repeat(8),
    )
    .into_bytes()
}

/// Deterministic incompressible bytes.
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 32) as u8
        })
        .collect()
}
