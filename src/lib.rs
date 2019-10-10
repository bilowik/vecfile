/// A Vec-type collection that sits in a file vs in memory. Has many of the same operations as 
/// Vec, and has some optional protections against Read/Write issues with the underlying file via
/// 'shadows.' Can be iterated over, can be easily cloned, and converted to and from Vec. 
///
/// ## Example
/// ```
/// use vecfile::vec_file::*;
///
/// fn main() {
///     let mut vf = VecFile::new();
///     vf.push(&10u8);
///     vf.push(&210u8);
///     assert_eq!(vf.pop(), 210);
/// }
/// ```
pub mod vec_file;
pub use crate::vec_file::*;
