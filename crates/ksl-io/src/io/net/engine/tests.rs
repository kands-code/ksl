//! 读模式的长度上限测试.

use super::{READ_CHUNK, ReadMode};

#[test]
fn read_lengths_follow_mode_limits() {
    assert_eq!(ReadMode::Read.read_len(10, 0), 10);
    assert_eq!(ReadMode::Read.read_len(READ_CHUNK * 2, 0), READ_CHUNK);
    assert_eq!(ReadMode::Exact.read_len(100, 40), 60);
    assert_eq!(ReadMode::Exact.read_len(READ_CHUNK * 2, 0), READ_CHUNK);
    assert_eq!(ReadMode::All.read_len(10, 10), 1);
    assert_eq!(ReadMode::Line.read_len(10, 0), 11);
    assert_eq!(ReadMode::Line.read_len(10, 4), 7);
    assert_eq!(ReadMode::All.read_len(READ_CHUNK * 2, 0), READ_CHUNK);
}
