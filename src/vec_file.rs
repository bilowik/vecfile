use std::io::{Read, Write, Seek, SeekFrom};
use desse::{Desse, DesseSized};
use std::fs::{File, OpenOptions};
use std::marker::PhantomData;


pub struct VecFile<T: Desse + DesseSized> {
    file: File,
    len: u64,
    cap: u64,
    _phantom: PhantomData<*const T>,
}


impl<T: Desse + DesseSized> VecFile<T> {

     
    // Note: At the end of every public method, the file should be seek'd to the pos of where a new
    // element should be written to.
   
    /// Creates a new empty VecFile using a temporary file
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            file: tempfile::tempfile()?,
            len: 0,
            cap: 8,
            _phantom: PhantomData,
        })
    }

    /// This creates a new VecFile that points at a file with the given path. 
    /// NOTE: This truncates the file.
    pub fn new_with_path<P: AsRef<std::path::Path>>(path: P) 
        -> Result<Self, Box<dyn std::error::Error>> {

        Ok(Self {
            file: OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path)?,
            len: 0,
            cap: 8,
            _phantom: PhantomData,
        })
    }


    /// Checks that the given index is a useable index, which it will be as long as 
    /// index * size_of::<T>() + (size_of::<T>() - 1) does not exceed std::u64::MAX
    fn calc_index(&self, index: u64) -> Result<u64, Error> {
        
        // Check that the start index is in range 
        let start_index = index.checked_mul(self.element_size() as u64)
                                                .ok_or(Error::IndexExceedsMaxU64)?;
        // Check that the end index is in range
        start_index.checked_add(self.element_size() as u64 - 1)
                                                .ok_or(Error::IndexExceedsMaxU64)?;

        // Return the first index
        Ok(start_index)
    }

    /// Returns true if the given index is within the current len
    pub fn bounds_check(&self, index: u64) -> bool { 
         index < self.len
    }

    /// Returns true if the current capacity could fit at least one more element
    pub fn capacity_check(&self, index: u64) -> bool {
        index < self.cap
    }

    /// Get the current allocated capacity
    pub fn cap(&self) -> u64 {
        self.cap
    }

    /// Get the current number of elements
    pub fn len(&self) -> u64 {
        self.len
    }

   
    /// Tries to return the element at the given index.
    ///
    /// This will return Err if index is out of range, or if the underlying file is no longer
    /// accessible.
    pub fn try_get(&mut self, index: u64) -> Result<T, Box<dyn std::error::Error>> {
        if !self.bounds_check(index) {
            // Index is out of range
            return Err(Error::OutOfRange(index, self.len).into());
        }

        let offset_index = self.calc_index(index)?;

        
        self.file.seek(SeekFrom::Start(offset_index))?;
        let ret = self.read_at_curr_seek()?;
        self.reset_seek_to_len()?;
        Ok(ret)
    }

    /// Returns the element at the given index.
    ///
    /// This will panic if index is out of range, or if the underlying file is no longer accessible
    pub fn get(&mut self, index: u64) -> T {
        self.try_get(index).unwrap()
    }

    /// Tries to set the element at the given index to value.
    ///
    /// This will return Err if index is out of range, or if the underlying file is no longer
    /// accessible.
    pub fn try_set(&mut self, index: u64, value: &T) -> Result<(), Box<dyn std::error::Error>> {
        if !self.bounds_check(index) {
            // Index is out of range
            return Err(Error::OutOfRange(index, self.len).into());
        }


        let offset_index = self.calc_index(index)?;
        self.file.seek(SeekFrom::Start(offset_index))?;
        self.write_at_curr_seek(value)?;
        self.reset_seek_to_len()?;
        Ok(())
    }

    /// Sets the element at the given index to value.
    ///
    /// This will panic if index is out of range, or if the underlying file is no longer accessible
    pub fn set(&mut self, index: u64, value: &T) {
        self.try_set(index, value).unwrap()
    }

    /// Resizes the len to fit the new_len. If new_len is less than the current len, the elements
    /// are just truncated.
    pub fn resize(&mut self, new_len: u64, value: &T) -> Result<(), Box<dyn std::error::Error>> {
        if new_len > self.len {
            while self.cap < new_len {
                self.expand()?;
            }
            while self.len() < new_len {
                // We could just continually call push here, but we know we don't need to do 
                // expansion checks or bound checks, so this will be faster
                self.write_at_curr_seek(&value)?;
                self.len = self.len + 1;
            }

        }
        self.len = new_len;
        Ok(())

    }

    /// Reserves capacity for at least 
    pub fn reserve(&mut self, additional: u64) -> Result<(), Box<dyn std::error::Error>> {
        let needed_cap = self.len + additional;
        while self.cap < needed_cap {
            self.expand()?;
        }
        Ok(())
    }

    /// Copies all elements from slice to the collection
    pub fn extend_from_slice(&mut self, slice: &[T]) -> Result<(), Box<dyn std::error::Error>> {
        self.calc_index(self.len + slice.len() as u64)?; // Check that the last index doesn't exceed u64
        self.reserve(slice.len() as u64)?;  // Reserve the addtional space
        self.len = self.len + slice.len() as u64; // Add the slice's len to the collections

        // Copy in the slice
        for e in slice {
            self.write_at_curr_seek(e.clone())?;
        }
        
        Ok(())
    }


    pub fn truncate(&mut self, new_len: u64) {
        self.len = new_len;
    }

    fn expand(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.cap = self.cap * 2;
        self.file.set_len(self.cap * self.element_size() as u64)?;
        Ok(())
    }

    fn expand_if_needed(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.len == self.cap {
            self.expand()?
        }
        Ok(())
    }

    fn reset_seek_to_len(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.file.seek(SeekFrom::Start(self.calc_index(self.len)?))?;
        Ok(())
    }


    pub fn try_push(&mut self, value: &T) -> Result<(), Box<dyn std::error::Error>> {
        if self.len < std::u64::MAX {
            self.expand_if_needed()?;
            self.write_at_curr_seek(value)?;
            self.len = self.len + 1;
            Ok(())
        }
        else {
            Err(Error::PushOnFull.into())
        }
    }

    pub fn push(&mut self, value: &T) {
        self.try_push(value).unwrap()
    }

    pub fn try_pop(&mut self) -> Result<T, Box<dyn std::error::Error>> {
        if self.len > 0 {
            // The collection is not empty 
            // Moves back to the last element
            self.file.seek(SeekFrom::Current(-(self.element_size() as i64)))?; 

            let ret = self.read_at_curr_seek()?;
            // The 'cursor' is now where it began, but we just popped the last element, move back
            // one element to point to the end of the collection.
            self.file.seek(SeekFrom::Current(-(self.element_size() as i64)))?; 
            
            self.len = self.len - 1; // Decrement len
            Ok(ret)
        }
        else {
            // The collection is empty, can't pop from an empty collection
            Err(Error::PopOnEmpty.into())
        }
    }

    pub fn pop(&mut self) -> T {
        self.try_pop().unwrap()
    }


    pub fn element_size(&self) -> usize {
        std::mem::size_of::<<T as Desse>::Output>()
    }


    /// Copies the original underlying file into a new file at path.
    /// If a file exists there, it gets truncated.
    pub fn to_named_file<U: AsRef<std::path::Path>>(&mut self, path: U) 
        -> Result<(), Box<dyn std::error::Error>> {

        let mut named_file = std::fs::OpenOptions::new()
                                .read(true)
                                .write(true)
                                .create(true)
                                .truncate(true)
                                .open(path)?;

        std::io::copy(&mut self.file, &mut named_file)?;
        self.file = named_file;
        Ok(())
    }



    fn read_at_curr_seek(&mut self) -> Result<T, Box<dyn std::error::Error>> {
        let element_size = self.element_size();
        let mut buf = Vec::with_capacity(element_size);
        (&mut self.file).take(element_size as u64).read_to_end(&mut buf)?;

        // We know the size of <T as Desse>::Output, and we know it's a u8 array of that
        // size, so even though the compilier doesn't know that, we can use transmute to treat it
        // as such. This should always be safe as long as <T as Desse>::Output is a array of u8.
        Ok(de_from::<T>(&buf)?)
    }


    fn write_at_curr_seek(&mut self, value: &T) -> Result<(), Box<dyn std::error::Error>> {
        let val_ser = value.serialize();
        
        // We know the size of <T as Desse>::Output, and we know it's a u8 array of that
        // size, so even though the compilier doesn't know that, we can use transmute to treat it
        // as such. This should always be safe as long as <T as Desse>::Output is a array of u8.
        unsafe {
            let ptr: * const u8 = std::mem::transmute(&val_ser as * const _);
            let val_ser_recon = std::slice::from_raw_parts(ptr, self.element_size());
            self.file.write_all(val_ser_recon)?; 
        }
        Ok(())
    }

   

}

impl<T: Desse + DesseSized> std::iter::IntoIterator for &VecFile<T> {
    type Item = T;
    type IntoIter = VecFileIterator<T>;

    fn into_iter(self) -> Self::IntoIter {
        let mut file = self.file.try_clone().unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        VecFileIterator {
            file,
            len: self.len,
            counter: 0,
            _phantom: PhantomData,
        }
    }
}


impl<T: Desse + DesseSized> std::convert::TryFrom<Vec<T>> for VecFile<T> {
    type Error = Box<dyn std::error::Error>;
    fn try_from(vec: Vec<T>) -> Result<Self, Self::Error> {
        let mut ret = VecFile::new()?;
        ret.reserve(vec.len() as u64)?;
        ret.extend_from_slice(&vec)?;
        Ok(ret)
    }
}

impl<T: Desse + DesseSized> std::convert::TryInto<Vec<T>> for VecFile<T> {
    type Error = Box<dyn std::error::Error>;
    fn try_into(self) -> Result<Vec<T>, Self::Error> {
        if self.len() > (std::usize::MAX as u64) {
            return Err(Error::LenExceedsUsize(self.len()).into());
        }
        let mut vec = Vec::with_capacity(std::mem::size_of::<T>());

        for value in &self {
            vec.push(value);
        }

        Ok(vec)

    }
}




pub struct VecFileIterator<T: Desse + DesseSized> {
    file: File,
    len: u64,
    counter: u64,
    _phantom: PhantomData<T>,
}

impl<T: Desse + DesseSized> std::iter::Iterator for VecFileIterator<T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.counter < self.len {
            self.counter = self.counter + 1;
            let mut buf = Vec::with_capacity(std::mem::size_of::<T>());
            (&mut self.file).take(std::mem::size_of::<T>() as u64).read_to_end(&mut buf).unwrap();
            Some(de_from(buf.as_slice()).unwrap())
        }
        else {
            None
        }
    }
}
        







#[derive(Debug)]
pub enum Error {
    OutOfRange(u64, u64),
    IndexExceedsMaxU64,
    PopOnEmpty,
    PushOnFull,
    LenExceedsUsize(u64),
    InequalSizeForDe(usize, usize),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::OutOfRange(index, len) =>
                write!(f, "Index out of range: Index: {}; Length: {}", index, len),
            Error::IndexExceedsMaxU64 => 
                write!(f, "Index out of range of possible u64 values"),
            Error::PopOnEmpty => 
                write!(f, "Collection is empty, no elements to pop"),
            Error::PushOnFull => 
                write!(f, "Collection is full, no elements can be pushed"),
            Error::LenExceedsUsize(len) =>
                write!(f, "Cannot convert to Vec, len: {} > std::usize::MAX", len),
            Error::InequalSizeForDe(size_given, size_required) => 
                write!(f, "Deserialize failure, size's must be equal. Given: {}; Required: {}",
                       size_given,
                       size_required
                       ),
        }
    }
}

impl std::error::Error for Error {}


pub(crate) fn de_from<T: Desse + DesseSized>(buf: &[u8]) -> Result<T, Box<dyn std::error::Error>> {

    if buf.len() != std::mem::size_of::<T>() {
        return Err(Error::InequalSizeForDe(buf.len(), std::mem::size_of::<T>()).into());
    }

    // We know the size of <T as Desse>::Output, and we know it's a u8 array of that
    // size, so even though the compilier doesn't know that, we can use transmute to treat it
    // as such. This should always be safe as long as <T as Desse>::Output is a array of u8.
    unsafe {
        Ok(T::deserialize_from(std::mem::transmute(buf.as_ptr())))
   }
}




#[allow(unused_variables)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryInto;

    #[test]
    fn push_pop() {
        let num: u64 = 0xff00ff50;
        let num2: u64 = 0xdd00ddf0;
        let num3: u64 = 0xaa00aa10;
        let num4: u64 = 0xbb00bbaa;
        let mut f: VecFile<u64> = VecFile::new_with_path("push_pop.bin").unwrap();
        f.push(&num);
        f.push(&num2);
        f.push(&num3);
        f.push(&num4);
        
        assert_eq!(f.pop(), num4);
        assert_eq!(f.pop(), num3);
        assert_eq!(f.pop(), num2);
        assert_eq!(f.pop(), num);
    }

    #[test]
    fn set_get() {
        let num: u16 = 0x1111;
        let num2: u16 = 0xbbbb;
        let num3: u16 = 0x8888;
        let num4: u16 = 0x9999;
        let num5: u16 = 0xffff;
        let num6: u16 = 0x5555;
        
        let mut f: VecFile<u16> = VecFile::new_with_path("set_get.bin").unwrap();
        f.resize(32, &num5).unwrap();
        f.set(1, &num);
        f.set(3, &num2);
        f.set(6, &num3);
        f.set(13, &num4);
        f.push(&num6);
        
        assert_eq!(f.get(1), num);
        assert_eq!(f.get(3), num2);
        assert_eq!(f.get(5), num5);
        assert_eq!(f.get(6), num3);
        assert_eq!(f.get(13), num4);
        assert_eq!(f.pop(), num6);

    }

    #[test]
    fn slices() {
        let mut f: VecFile<u16> = VecFile::new().unwrap();
        let slice = [0x1111, 0x3333, 0x2222, 0xffff, 0xdddd];
        f.extend_from_slice(&slice).unwrap();
        
        assert_eq!(f.get(0), 0x1111);
        assert_eq!(f.get(3), 0xffff);
        assert_eq!(f.get(2), 0x2222);
        assert_eq!(f.get(4), 0xdddd);
        assert_eq!(f.get(1), 0x3333);
    }

    #[test]
    fn iterator() {
        let orig_values: Vec<u16> = vec![0x2222, 0xffff, 0xdddd, 0xaaaa, 0x8888];
        let f: VecFile<u16> = orig_values.clone().try_into().unwrap();
        
        for (orig, arr_file) in orig_values.into_iter().zip(f.into_iter()) {
            assert_eq!(orig, arr_file);
        }
    }


    #[test]
    #[should_panic]
    fn index_out_of_bounds() {
        let mut f: VecFile<u16> = vec![0x2222, 0xffff, 0xdddd, 0xaaaa].try_into().unwrap();
        f.get(4);
    }

    #[test]
    #[should_panic]
    fn pop_on_empty() {
        let mut f: VecFile<u16> = vec![].try_into().unwrap();
        f.pop();
    }

    #[test]
    fn try_from_into() {
        let orig_vec: Vec<u16> = vec![0x1111, 0x2222, 0x3333, 0x4444, 0x5555];
        let orig_f: VecFile<u16> = orig_vec.clone().try_into().unwrap();
        let vec: Vec<u16> = orig_f.try_into().unwrap();
    
        assert_eq!(orig_vec.len(), vec.len());
        for i in 0..orig_vec.len() {
            assert_eq!(orig_vec[i], vec[i]);
        }
    }



}














