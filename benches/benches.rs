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
        vf.reserve(LEN as u64).unwrap();

        b.iter(|| {
            for val in buf.iter() {
                vf.push(val);
            }
        });

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
            let mut vf_clone = vf.clone();
            for _ in 0u64..((LEN - 1) as u64) {
                vf_clone.pop();
            }
        })


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
            for i in 0u64..((LEN - 1) as u64) {
                vf.get(i);
            }
        });
    }

    #[bench]
    fn set(b: &mut Bencher) {
        const LEN: usize = 1024;
        let buf: [u8; LEN] = unsafe { [std::mem::MaybeUninit::uninit().assume_init(); LEN] };
        let mut vf = VecFile::new();
        vf.resize(LEN as u64, &0u8);

        b.iter(|| {
            for (i, val) in buf.iter().enumerate() {
                vf.set(i as u64, &val);
            }
        });
    }

}
        
