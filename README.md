![Verify](https://github.com/kalamay/vmap-rs/workflows/Verify/badge.svg)

# vmap-rs
A cross-platform library for fast and safe memory-mapped IO in Rust

Take a look at the [Documentation](https://docs.rs/vmap/) for details!

This library defines a convenient API for reading and writing to files
using the hosts virtual memory system. The design of the API strives to
both minimize the frequency of mapping system calls while still retaining
safe access. Critically, it never attempts the own the `File` object used
for mapping. That is, it never clones it or in any way retains it. While
this has some implications for the API (i.e. [`.flush()`]), it cannot cause
bugs outside of this library through `File`'s leaky abstraction when cloned
and then closed.

The [`Map`] and [`MapMut`] types are primary means for allocating virtual
memory regions, both for a file and anonymously. Generally, the
[`Map::with_options()`] and [`MapMut::with_options()`] are used to specify
the mapping requirements. See [`Options`] for more information.

The [`MapMut`] type maintains interior mutability for the mapped memory,
while the [`Map`] is read-only. However, it is possible to convert between
these types ([`.into_map_mut()`] and [`.into_map()`]) assuming the proper
[`Options`] are specified.

Additionally, a variety of buffer implementations are provided in the
[`vmap::io`] module. The [`Ring`] and [`InfiniteRing`] use circular memory
address allocations using cross-platform optimizations to minimize excess
resources where possible. The [`BufReader`] and [`BufWriter`] implement
buffered I/O using a [`Ring`] as a backing layer.

# Examples

```rust
use vmap::Map;
use std::{fs, str};

let path = "example";

// Write some test data
fs::write(&path, b"this is a test")?;

// Map the first 4 bytes
let (map, file) = Map::with_options().len(4).open(&path)?;
assert_eq!(Ok("this"), str::from_utf8(&map[..]));

// Reuse the file to map a different region
let map = Map::with_options().offset(10).len(4).map(&file)?;
assert_eq!(Ok("test"), str::from_utf8(&map[..]));
```

If opened properly, the [`Map`] can be moved into a [`MapMut`] and modifications
to the underlying file can be performed:

```rust
use vmap::Map;
use std::{fs, str};

let path = "example";

// Write some test data
fs::write(&path, b"this is a test")?;

// Open with write permissions so the Map can be converted into a MapMut
let (map, file) = Map::with_options().write().len(14).open(&path)?;
assert_eq!(Ok("this is a test"), str::from_utf8(&map[..]));

// Move the Map into a MapMut
// ... we could have started with MapMut::with_options()
let mut map = map.into_map_mut()?;
map[..4].clone_from_slice(b"that");

// Flush the changes to disk synchronously
map.flush(&file, Flush::Sync)?;

// Move the MapMut back into a Map
let map = map.into_map()?;
assert_eq!(Ok("that is a test"), str::from_utf8(&map[..]));
```

## Ring Buffer

The [`vmap`] library contains a [`Ring`] that constructs a circular memory
allocation where values can wrap from around from the end of the buffer back
to the beginning with sequential memory addresses. The [`InfiniteRing`] is
similar, however it allows writes to overwrite reads.

```rust
use vmap::io::{Ring, SeqWrite};
use std::io::{BufRead, Read, Write};

let mut buf = Ring::new(4000).unwrap();
let mut i = 1;

// Fill up the buffer with lines.
while buf.write_len() > 20 {
    write!(&mut buf, "this is test line {}\n", i)?;
    i += 1;
}

// No more space is available.
assert!(write!(&mut buf, "this is test line {}\n", i).is_err());

let mut line = String::new();

// Read the first line written.
let len = buf.read_line(&mut line)?;
assert_eq!(line, "this is test line 1\n");

line.clear();

// Read the second line written.
let len = buf.read_line(&mut line)?;
assert_eq!(line, "this is test line 2\n");

// Now there is enough space to write more.
write!(&mut buf, "this is test line {}\n", i)?;
```

[`.flush()`]: https://docs.rs/vmap/0.6.1/vmap/struct.MapMut.html#method.flush
[`.into_map()`]: https://docs.rs/vmap/0.6.1/vmap/struct.MapMut.html#method.into_map
[`.into_map_mut()`]: https://docs.rs/vmap/0.6.1/vmap/struct.Map.html#method.into_map_mut
[`BufReader`]: https://docs.rs/vmap/0.6.1/vmap/io/struct.BufReader.html
[`BufWriter`]: https://docs.rs/vmap/0.6.1/vmap/io/struct.BufWriter.html
[`InfiniteRing`]: https://docs.rs/vmap/0.6.1/vmap/io/struct.InfiniteRing.html
[`Map::with_options()`]: https://docs.rs/vmap/0.6.1/vmap/struct.Map.html#method.with_options
[`MapMut::with_options()`]: https://docs.rs/vmap/0.6.1/vmap/struct.MapMut.html#method.with_options
[`MapMut`]: https://docs.rs/vmap/0.6.1/vmap/struct.MapMut.html
[`Map`]: https://docs.rs/vmap/0.6.1/vmap/struct.Map.html
[`Options`]: https://docs.rs/vmap/0.6.1/vmap/struct.Options.html
[`Ring`]: https://docs.rs/vmap/0.6.1/vmap/io/struct.Ring.html
[`vmap::io`]: https://docs.rs/vmap/0.6.1/vmap/io/index.html
[`vmap`]: https://docs.rs/vmap/
