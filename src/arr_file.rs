use std::io::{Read, Write, Seek, SeekFrom};
use desse::{Desse, DesseSized};
use std::fs::{File, OpenOptions};
use std::marker::PhantomData;

pub trait ArrFileTrait {
    type Element: 
        DesseSized + 
        Desse<Output: Default + AsRef<[u8]> + AsMut<[u8]> + Into<Box<[u8]>> + From<Box<[u8]>>>;
}

pub struct ArrFile<T: Desse + DesseSized> {
    file: File,
    len: u64,
    cap: u64,
    _phantom: PhantomData<*const T>,
}

impl<T> ArrFileTrait for ArrFile<T>
where T: Desse<Output: Default + AsRef<[u8]> + AsMut<[u8]> + Into<Box<[u8]>> + From<Box<[u8]>>> +
         DesseSized {
    type Element = T;
}

impl<T> ArrFile<T> 
where T: Desse<Output: Default + AsRef<[u8]> + AsMut<[u8]> + Into<Box<[u8]>> + From<Box<[u8]>>> +
         DesseSized {

    const ELEMENT_SIZE: usize = std::mem::size_of::<<T as Desse>::Output>();
     
    // Note: At the end of every public method, the file should be seek'd to the pos of where a new
    // element should be written to.
   
    /// Creates a new empty ArrFile using a temporary file
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            file: tempfile::tempfile()?,
            len: 0,
            cap: 8,
            _phantom: PhantomData,
        })
    }

    /// This creates a new ArrFile that points at a file defined by path. 
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
        let start_index = index.checked_mul(Self::ELEMENT_SIZE as u64)
                                                .ok_or(Error::IndexExceedsMaxU64)?;
        // Check that the end index is in range
        start_index.checked_add(Self::ELEMENT_SIZE as u64 - 1)
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
    pub fn capacity(&self) -> u64 {
        self.cap
    }

    /// Get the current number of elements
    pub fn len(&self) -> u64 {
        self.len
    }

   
    /// Returns the element at the given index
    pub fn get(&mut self, index: u64) -> Result<T, Box<dyn std::error::Error>> {
        if !self.bounds_check(index) {
            // Index is out of range
            return Err(Error::OutOfRange(index, self.len).into());
        }

        let offset_index = self.calc_index(index)?;

        
        self.file.seek(SeekFrom::Start(offset_index))?;
        Ok(self.read_at_curr_seek()?)
    }

    pub fn set(&mut self, index: u64, value: &T) -> Result<(), Box<dyn std::error::Error>> {
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

    /// Resizes the len to fit the new_len. If new_len is less than the current len, the elements
    /// are just truncated.
    pub fn resize(&mut self, new_len: u64, value: T) -> Result<(), Box<dyn std::error::Error>> {
        if new_len > self.len {
            while self.cap < new_len {
                self.expand()?;
            }
            dbg!("");
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
        while needed_cap < self.cap {
            self.expand()?;
        }
        Ok(())
    }

    /// Reserves capacity for exactly addtional elements

    pub fn truncate(&mut self, new_len: u64) {
        self.len = new_len;
    }

    fn expand(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.cap = self.cap * 2;
        self.file.set_len(self.cap)?;
        Ok(())
    }

    fn expand_if_needed(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.len == self.cap {
            self.expand()?
        }
        Ok(())
    }

    fn reset_seek_to_len(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.file.seek(SeekFrom::Start(self.len() * (Self::ELEMENT_SIZE as u64)))?;
        Ok(())
    }


    pub fn push(&mut self, value: &T) -> Result<(), Box<dyn std::error::Error>> {
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

    pub fn pop(&mut self) -> Result<T, Box<dyn std::error::Error>> {
        if self.len > 0 {
            // The collection is not empty 

            // Moves back to the last element
            self.file.seek(SeekFrom::Current(-(Self::ELEMENT_SIZE as i64)))?; 
            let ret = self.read_at_curr_seek()?;

            // The 'cursor' is now where it began, but we just popped the last element, move back
            // one element to point to the end of the collection.
            self.file.seek(SeekFrom::Current(-(Self::ELEMENT_SIZE as i64)))?; 
            
            self.len = self.len - 1; // Decrement len
            Ok(ret)
        }
        else {
            // The collection is empty, can't pop from an empty collection
            Err(Error::PopOnEmpty.into())
        }



    }


    pub fn element_size(&self) -> usize {
        Self::ELEMENT_SIZE
    }

    fn read_at_curr_seek(&mut self) -> Result<T, Box<dyn std::error::Error>> {
        let mut buf: Box<[u8]> = <<Self as ArrFileTrait>::Element as Desse>::Output::default().into();
        self.file.read_exact(&mut buf)?;
        Ok(T::deserialize_from(&mut buf.into()))
    }


    fn write_at_curr_seek(&mut self, value: &T) -> Result<(), Box<dyn std::error::Error>> {
        self.file.write_all(&value.serialize().into())?; 
        Ok(())
    }

   





}


#[derive(Debug)]
pub enum Error {
    OutOfRange(u64, u64),
    IndexExceedsMaxU64,
    PopOnEmpty,
    PushOnFull,
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
        }
    }
}

impl std::error::Error for Error {}







#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop() {
        let num: u64 = 0xff00ff50;
        let num2: u64 = 0xdd00ddf0;
        let num3: u64 = 0xaa00aa10;
        let num4: u64 = 0xbb00bbaa;
        let mut f: ArrFile<u64> = ArrFile::new_with_path("push_pop.bin").unwrap();
        f.push(&num).unwrap();
        dbg!(f.element_size());
        f.push(&num2).unwrap();
        f.push(&num3).unwrap();
        f.push(&num4).unwrap();
        
        assert_eq!(f.pop().unwrap(), num4);
        assert_eq!(f.pop().unwrap(), num3);
        assert_eq!(f.pop().unwrap(), num2);
        assert_eq!(f.pop().unwrap(), num);
    }

    #[test]
    fn set_get() {
        let num: u64 = 123;
        let num2: u64 = 232;
        let num3: u64 = 1101;
        let num4: u64 = 501203;
        
        let mut f: ArrFile<u64> = ArrFile::new_with_path("set_get.bin").unwrap();
        f.resize(32, 0xbbffaacc).unwrap();
    }

}














