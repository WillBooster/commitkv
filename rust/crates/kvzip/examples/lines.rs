//! Stores every line of a file as a value and reports the stored size and the throughput:
//! `cargo run --release --example lines -- <file> [<max records>]`.

use std::{env, fs, time::Instant};

use kvzip::{Options, Store};

fn main() {
    let mut args = env::args().skip(1);
    let path = args.next().expect("usage: lines <file> [<max records>]");
    let limit = args
        .next()
        .map_or(usize::MAX, |limit| limit.parse().expect("a record count"));
    let content = fs::read(path).expect("the file is readable");
    assert!(!content.is_empty(), "the file holds no line");
    let content = content.strip_suffix(b"\n").unwrap_or(&content);
    let lines: Vec<&[u8]> = content.split(|&byte| byte == b'\n').take(limit).collect();
    let raw: usize = lines.iter().map(|line| line.len()).sum();
    let dir = tempfile::tempdir().expect("a temporary directory");

    let store = Store::open(dir.path(), Options::default()).expect("the store opens");
    let started = Instant::now();
    for (i, line) in lines.iter().enumerate() {
        store.put(&i.to_le_bytes(), line).expect("put succeeds");
    }
    let put_seconds = started.elapsed().as_secs_f64();
    drop(store);

    let stored: u64 = fs::read_dir(dir.path())
        .expect("the directory is readable")
        .filter(|entry| {
            let path = entry.as_ref().expect("an entry").path();
            path.extension().is_some_and(|extension| extension == "kvz")
        })
        .map(|entry| entry.expect("an entry").metadata().expect("metadata").len())
        .sum();
    let started = Instant::now();
    let store = Store::open(dir.path(), Options::default()).expect("the store reopens");
    let open_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    // A stride coprime to the record count visits every record in a scattered order.
    let stride = (1..)
        .map(|i| lines.len() / 2 + i)
        .find(|&s| gcd(s, lines.len()) == 1)
        .expect("a stride");
    for i in 0..lines.len() {
        let index = i * stride % lines.len();
        assert_eq!(
            store
                .get(&index.to_le_bytes())
                .expect("get succeeds")
                .as_deref(),
            Some(lines[index])
        );
    }
    let get_seconds = started.elapsed().as_secs_f64();

    let megabytes = raw as f64 / 1e6;
    println!(
        "records: {}, raw: {megabytes:.1} MB, stored: {:.2}%",
        lines.len(),
        100.0 * stored as f64 / raw as f64
    );
    println!(
        "put: {:.0} MB/s ({:.0} us/record), open: {:.3} s, scattered get: {:.0} us/record",
        megabytes / put_seconds,
        1e6 * put_seconds / lines.len() as f64,
        open_seconds,
        1e6 * get_seconds / lines.len() as f64,
    );
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}
