#![feature(test)]
#[cfg(test)]
mod tests {
    extern crate test;
    use vecfile::*;
    use test::Bencher;

    #[bench]
    fn push(b: &mut Bencher) {
        const LEN: usize = 1024;
        let buf: [u8; LEN] = unsafe { [std::mem::MaybeUninit::uninit().assume_init(); LEN] };
        let mut vf = VecFile::new();

        b.iter(|| {
            for val in buf.iter() {
                vf.push(val);
            }
        });
    }
}
        
