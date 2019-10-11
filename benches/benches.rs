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

        for val in &vf {
            dbg!(val);
        }

    }

    #[bench]
    fn pop(b: &mut Bencher) {
        const LEN: usize = 1024;
        let buf: [u8; LEN] = unsafe { [std::mem::MaybeUninit::uninit().assume_init(); LEN] };
        let mut vf = VecFile::new();
        for val in buf.iter() {
            vf.push(val);
        }

        b.iter(|| {
            for _ in 0u64..(vf.len() - 1) {
                vf.pop();
            }
        });
    }

    #[bench]
    fn read(b: &mut Bencher) {
        const LEN: usize = 1024;
        let buf: [u8; LEN] = unsafe { [std::mem::MaybeUninit::uninit().assume_init(); LEN] };
        let mut vf = VecFile::new();
        for val in buf.iter() {
            vf.push(val);
        }

        b.iter(|| {
            for i in 0u64..vf.len() {
                vf.get(i);
            }
        });
    }
}
        
