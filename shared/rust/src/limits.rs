//! Lengths written inside a file that comes from outside are never trusted.
//!
//! A count or a size read from a foreign file (an MP4 table, a Matroska element, a zip entry, a PNG header) is only what
//! the file says. Used as it stands for `vec![0; n]` or `Vec::with_capacity(n)`, a few bytes claiming four billion
//! samples ask for a hundred gigabytes, and a failed allocation ends the whole program (`handle_alloc_error` aborts;
//! `catch_unwind` can't stop it). So every parser goes through here: a declared length is first held to what the file
//! actually has, and memory is asked for in a way that can say no.

use std::io::{self, Read};

/// The most a declared length can honestly be: no more than the bytes the file still has from here.
pub fn cap(declared: u64, available: u64) -> u64 {
    declared.min(available)
}

/// Whether `count` items of at least `each` bytes apiece can really be in `available` bytes.
pub fn fits(count: u64, each: u64, available: u64) -> bool {
    count.checked_mul(each.max(1)).map(|n| n <= available).unwrap_or(false)
}

fn no_memory(len: u64) -> io::Error {
    io::Error::new(io::ErrorKind::OutOfMemory, format!("not enough memory for {len} bytes"))
}

/// An empty list with room for `n` items, or an error when that much memory can't be had.
pub fn room<T>(n: usize) -> io::Result<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| no_memory((n as u64).saturating_mul(std::mem::size_of::<T>() as u64)))?;
    Ok(v)
}

/// `len` zero bytes, or an error when that much memory can't be had.
pub fn zeroed(len: u64) -> io::Result<Vec<u8>> {
    let n = usize::try_from(len).map_err(|_| no_memory(len))?;
    let mut v = room::<u8>(n)?;
    v.resize(n, 0);
    Ok(v)
}

/// Reads a part whose length the file declares. A part longer than what is left of the file (`available`) is damage,
/// not a reason to ask for the memory: it is refused before anything is allocated.
pub fn read_declared<R: Read + ?Sized>(r: &mut R, declared: u64, available: u64) -> io::Result<Vec<u8>> {
    if declared > available {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a part says it is {declared} bytes, but only {available} are left"),
        ));
    }
    let mut v = zeroed(declared)?;
    r.read_exact(&mut v)?;
    Ok(v)
}

/// Reads at most `limit` bytes of a stream whose real length isn't known in advance (a compressed entry), growing as the
/// bytes really arrive rather than trusting a declared size up front. More than `limit` is an error.
pub fn read_up_to<R: Read + ?Sized>(r: &mut R, limit: u64) -> io::Result<Vec<u8>> {
    let mut v = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            return Ok(v);
        }
        if v.len() as u64 + n as u64 > limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("more than {limit} bytes")));
        }
        v.try_reserve(n).map_err(|_| no_memory(v.len() as u64 + n as u64))?;
        v.extend_from_slice(&buf[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_length_is_held_to_the_file() {
        assert_eq!(cap(u64::MAX, 100), 100);
        assert_eq!(cap(10, 100), 10);
        assert!(fits(25, 4, 100));
        assert!(!fits(26, 4, 100));
        assert!(!fits(u64::MAX, 32, u64::MAX));
        assert!(fits(0, 0, 0));
    }

    #[test]
    fn memory_that_cant_be_had_is_an_error_not_an_abort() {
        assert_eq!(zeroed(u64::MAX).unwrap_err().kind(), io::ErrorKind::OutOfMemory);
        assert_eq!(room::<[u8; 32]>(usize::MAX / 2).unwrap_err().kind(), io::ErrorKind::OutOfMemory);
        assert_eq!(zeroed(3).unwrap(), vec![0, 0, 0]);
    }

    #[test]
    fn a_part_longer_than_the_file_is_refused_before_allocating() {
        let mut r: &[u8] = &[1, 2, 3, 4];
        assert_eq!(read_declared(&mut r, 0xFFFF_FFFF_FFFF, 4).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let mut r: &[u8] = &[1, 2, 3, 4];
        assert_eq!(read_declared(&mut r, 3, 4).unwrap(), vec![1, 2, 3]);
        let mut r: &[u8] = &[7; 10];
        assert!(read_up_to(&mut r, 9).is_err());
        let mut r: &[u8] = &[7; 10];
        assert_eq!(read_up_to(&mut r, 10).unwrap().len(), 10);
    }
}
