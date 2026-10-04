use std::fs::File;
use std::io;
use std::ops::Range;
use std::os::unix::fs::FileExt;
use std::path::Path;

pub const SIDE: u64 = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Signature {
    size: u64,
    head: Vec<u8>,
    tail: Vec<u8>,
}

impl Signature {
    pub fn new(size: u64, head: impl Into<Vec<u8>>, tail: impl Into<Vec<u8>>) -> Self {
        Self {
            size,
            head: head.into(),
            tail: tail.into(),
        }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn ranges(size: u64) -> [Range<u64>; 2] {
        [0..size.min(SIDE), size.saturating_sub(SIDE)..size]
    }

    pub fn read_size(size: u64) -> u64 {
        Self::ranges(size)
            .iter()
            .map(|range| range.end - range.start)
            .sum()
    }

    pub fn of_file(path: &Path) -> io::Result<Option<Self>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let size = file.metadata()?.len();
        let read = |range: Range<u64>| -> io::Result<Vec<u8>> {
            let mut bytes = vec![0; (range.end - range.start) as usize];
            file.read_exact_at(&mut bytes, range.start)?;
            Ok(bytes)
        };
        let [head, tail] = Self::ranges(size);
        Ok(Some(Self::new(size, read(head)?, read(tail)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("file");
        std::fs::write(&path, bytes).expect("write the file");
        (dir, path)
    }

    fn of_bytes(bytes: &[u8]) -> Signature {
        let [head, tail] = Signature::ranges(bytes.len() as u64);
        let slice = |range: Range<u64>| &bytes[range.start as usize..range.end as usize];
        Signature::new(bytes.len() as u64, slice(head), slice(tail))
    }

    #[test]
    fn the_ranges_are_the_first_and_last_side_of_a_long_file() {
        assert_eq!(Signature::ranges(5000), [0..1024, 3976..5000]);
    }

    #[test]
    fn the_ranges_of_a_short_file_each_cover_all_of_it() {
        assert_eq!(Signature::ranges(10), [0..10, 0..10]);
    }

    #[test]
    fn a_long_file_reads_a_side_from_each_end() {
        assert_eq!(Signature::read_size(5000), 2048);
    }

    #[test]
    fn a_short_file_reads_all_of_itself_from_each_end() {
        assert_eq!(Signature::read_size(10), 20);
    }

    #[test]
    fn a_file_signs_as_its_bytes_do() {
        let bytes: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let (_dir, path) = written(&bytes);

        assert_eq!(
            Signature::of_file(&path).expect("read"),
            Some(of_bytes(&bytes))
        );
    }

    #[test]
    fn a_change_inside_the_tail_changes_the_signature() {
        let bytes = vec![0u8; 5000];
        let mut changed = bytes.clone();
        changed[4990] = 1;

        assert_ne!(of_bytes(&bytes), of_bytes(&changed));
    }

    #[test]
    fn a_missing_file_has_no_signature() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            Signature::of_file(&dir.path().join("absent")).expect("read"),
            None
        );
    }
}
