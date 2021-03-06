use std::io::{Read, Write, Seek, SeekFrom};
use desse::{Desse, DesseSized};
use std::fs::{File, OpenOptions};
use std::marker::PhantomData;
use std::cell::Cell;
use std::mem::transmute;

/// A file that can be treated similarly to a Vec. By default the underlying file is a temporary
/// file which is requested from the operating system, but there's options to used path'd files as
/// well.
///
/// Has optionally "shadowing" which syncs 1 or more files which can automatically replace the
/// original in case of IO or other types of errors. Once a shadow is used to replace the original,
/// another one is automatically generated and synced, so if there's 3 shadows there will always be
/// 3. 
///
/// While it's not technically required, the reserve method can be used to reduce file system
/// allocations, which may or may not be expensive depending on the underlying file system.
///
/// The reasoning behind having separate methods for trying and not, is to provide convenience.
/// When shadows are being used, it is relatively safe to call unwrap on try methods, so rather
/// than calling unwrap on most method calls, provided are wrapper methods that simply panic rather
/// than return an Err. 
/// If shadows aren't being used, the try methods may be used to avoid errors.
///
/// Note: Index and IndexMut are not implemented since they require returning references, and we
/// cannot get a reference from a section of a file.
pub struct VecFile<T: Desse + DesseSized> {
    file: Cell<File>, // The underlying file 
    shadows: Cell<Vec<File>>, // The shadows that may exist
    len: u64, // The current number of elements in the file
    cap: u64, // The max number of elements the file can hold at its given allocated lenght
    _phantom: PhantomData<*const T>, // Phantom data for the generic type parameter
}


impl<T: Desse + DesseSized> VecFile<T> {

     
    // Note: At the end of every public method, the file should be seek'd to the pos of where a new
    // element should be written to.
   
    /// Creates a new empty VecFile.
    pub fn new() -> Self {
        Default::default()
    }

    /// This creates a new VecFile that points at a file with the given path. 
    /// NOTE: This truncates the file.
    pub fn new_with_path<P: AsRef<std::path::Path>>(path: P) 
        -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { // TODO: Replace with default
            file: 
                OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path)?.into(),
            shadows: Vec::with_capacity(0).into(),
            len: 0,
            cap: 8,
            _phantom: PhantomData,
        })
    }

    /// Creates a VecFile instance with the given parts
    ///
    /// This is considered unsafe since there's no checks or guarantees that the reconstructed
    /// VecFile has the given len or cap or if the underlying data is valid data for the given type
    /// T.
    pub unsafe fn from_raw_parts(file: File, len: u64, cap: u64) -> Self {
        Self {
            file: file.into(),
            shadows: Vec::with_capacity(0).into(),
            len,
            cap,
            _phantom: PhantomData,
        }
    }
    
  
    /// Makes a deep copy
    ///
    /// This can fail if self doesn't have any shadows and theres a reads/write issue.
    pub fn try_clone(&self) -> Result<Self, Box<dyn std::error::Error>> {
        // This could be done much more efficiently, however it's prefered to have shadow
        // protection in case of read/write issues, so we want to do it with VecFile's methods
        
        let mut clone = Self::default();
        let shadows: &mut Vec<File> = unsafe { transmute(self.shadows.as_ptr()) };

        clone.add_shadows(shadows.len())?;
        clone.reserve(self.len)?; // Should be relatively safe if shadows are in play

        for element in self.into_iter() {
            clone.push(&element);
        }
        Ok(clone)
    }


    /// Adds a number of additional shadows to the VecFile.
    /// 
    /// This does not need to be re-done if a shadow is used to replace the original as its done
    /// automatically.
    pub fn add_shadows(&self, additional_shadows: usize) -> Result<(), Box<dyn std::error::Error>> {
        let shadows: &mut Vec<File> = unsafe { transmute(self.shadows.as_ptr()) };
        if additional_shadows > 0 {
            shadows.reserve(additional_shadows);
            for _ in 0..additional_shadows {
                let new_shadow = self.new_shadow()?;
                shadows.push(new_shadow);
            }
        }
        Ok(())
    }

    /// Removes the given number of shadows.
    pub fn remove_shadows(&mut self, shadow_to_remove: usize) {
        let shadows: &mut Vec<File> = unsafe { transmute(self.shadows.as_ptr()) };
        for _ in 0..shadow_to_remove {
            shadows.pop();
        }
    }

    /// Removes all shadows
    pub fn clear_shadows(&mut self) {
        self.shadows = vec![].into();
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

    /// Returns true if the given index is within the current capacity
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
    pub fn try_get(&self, index: u64) -> Result<T, Box<dyn std::error::Error>> {
        if !self.bounds_check(index) {
            // Index is out of range
            return Err(Error::OutOfRange(index, self.len).into());
        }

        let offset_index = self.calc_index(index)?;
        let file: &mut File = unsafe { transmute(self.file.as_ptr()) }; // Get file from cell
         
        file.seek(SeekFrom::Start(offset_index))?;
        let ret = self.read_at_curr_seek()?;
        self.reset_seek_to_len()?;
        Ok(ret)
    }

    /// Returns the element at the given index.
    ///
    /// This will panic if index is out of range, or if the underlying file is no longer accessible
    pub fn get(&self, index: u64) -> T {
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
        let file: &mut File = unsafe { transmute(self.file.as_ptr()) }; // Get file from cell



        let offset_index = self.calc_index(index)?;
        file.seek(SeekFrom::Start(offset_index))?;
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
    /// are just truncated. The given value is inserted into every added index.
    pub fn resize(&mut self, new_len: u64, value: &T) -> Result<(), Box<dyn std::error::Error>> {
        if new_len > self.len {
            while self.cap < new_len {
                self.expand()?;
            }
            while self.len() < new_len {
                // We could just continually call push here, but we know we don't need to do 
                // expansion checks or bound checks, so this will be faster
                self.write_at_curr_seek(value)?;
                self.len = self.len + 1;
            }

        }
        self.len = new_len;
        Ok(())

    }

    /// Resizes the len to fit the new_len. If new_len is less than the current len, the elements
    /// are just truncated. The value generated by f is inserted into every added index.
    pub fn resize_with<F>(&mut self, new_len: u64, mut f: F) 
        -> Result<(), Box<dyn std::error::Error>> 
        where F: FnMut() -> T   {

        if new_len > self.len {
            while self.cap < new_len {
                self.expand()?;
            }
            while self.len() < new_len {
                // We could just continually call push here, but we know we don't need to do 
                // expansion checks or bound checks, so this will be faster
                self.write_at_curr_seek(&(f()))?;
                self.len = self.len + 1;
            }

        }
        self.len = new_len;
        Ok(())

    }

    /// Reserves capacity for at least 'additional' more elements
    pub fn reserve(&mut self, additional: u64) -> Result<(), Box<dyn std::error::Error>> {
        let needed_cap = self.len + additional;
        while self.cap < needed_cap {
            self.expand()?;
        }
        Ok(())
    }

    /// Tries to copy all elements from slice to the collection
    pub fn try_extend_from_slice(&mut self, slice: &[T]) -> Result<(), Box<dyn std::error::Error>> {
        // Check that the last index doesn't exceed u64
        self.calc_index(self.len + slice.len() as u64)?; 
        
        self.reserve(slice.len() as u64)?;  // Reserve the addtional space
        self.len = self.len + slice.len() as u64; // Add the slice's len to the collections

        // Copy in the slice
        for e in slice {
            self.write_at_curr_seek(&e).unwrap();
        }
        Ok(())
        
    }

    /// Tries to copy all elements from slice to the collection
    pub fn extend_from_slice(&mut self, slice: &[T]) {
        self.try_extend_from_slice(slice).unwrap();
    }



    /// Truncates the collection to new_len, or does nothing if new_len is greater than the
    /// current length
    pub fn truncate(&mut self, new_len: u64) {
        if self.len > new_len {
            self.len = new_len;
        }
    }

    /// Tries to create a VecFile from an iterator
    pub fn try_from_iter<U: IntoIterator<Item=T>>(&mut self, iter: U) 
        -> Result<Self, Box<dyn std::error::Error>> {

        let mut vf = VecFile::new();
        vf.add_shadows(1)?; // To protect against potential read errors.

        for element in iter {
            vf.try_push(&element)?;
        }
        
        vf.remove_shadows(1);
        Ok(vf)
    }

    fn expand(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let file: &mut File = unsafe { transmute(self.file.as_ptr()) }; // Get file from cell
        let shadows: &mut Vec<File> = unsafe { transmute(self.shadows.as_ptr()) };
        self.cap = self.cap * 2;
        let new_file_size = self.cap * (self.element_size() as u64);
        while let Err(_) = file.set_len(new_file_size) {
            self.replace_with_shadow()?;
        }
        for i in 0..shadows.len() {
            match shadows[i].set_len(new_file_size) {
                Ok(_) => (),
                Err(_) => {
                    // This shadow is having write issues, replace it with another shadow.
                    // This new replacement doesn't need to be expanded like the others since it's
                    // a fresh copy of the original which has already been expanded.
                    self.replace_shadow_at_index(i)?;
                }
            }

        }
        Ok(())
    }

    fn expand_if_needed(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.len == self.cap {
            self.expand()?
        }
        Ok(())
    }

    fn reset_seek_to_len(&self) -> Result<(), Box<dyn std::error::Error>> {
        let file: &mut File = unsafe { transmute(self.file.as_ptr()) }; // Get file from cell
        file.seek(SeekFrom::Start(self.calc_index(self.len)?))?;
        Ok(())
    }


    /// Tries to push an element to the end of the collection.
    ///
    /// This will return an Error if the underlying file's len would exceed std::u64::MAX or 
    /// if the underlying file has write issues and no shadows exist.
    pub fn try_push(&mut self, value: &T) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(_) = self.calc_index(self.len) {
            self.expand_if_needed()?;
            self.write_at_curr_seek(value)?;
            self.len = self.len + 1;
            Ok(())
        }
        else {
            Err(Error::PushOnFull.into())
        }
    }

    /// Pushes the element to the end of the collection.
    ///
    /// This will panic if the underlying file's len would exceed std::u64::MAX or 
    /// if the underlying file has write issues and no shadows exist.
    pub fn push(&mut self, value: &T) {
        self.try_push(value).unwrap()
    }

    /// Pops the element at the end of the collection.
    ///
    /// This will return an error if the underlying file has read issues and no shadows exist or
    /// if the list is empty.
    pub fn try_pop(&mut self) -> Result<T, Box<dyn std::error::Error>> {
        if self.len > 0 {
            let file: &mut File = unsafe { transmute(self.file.as_ptr()) }; // Get file from cell
            // The collection is not empty 
            // Moves back to the last element
            file.seek(SeekFrom::Current(-(self.element_size() as i64)))?; 

            let ret = self.read_at_curr_seek()?;
            // The 'cursor' is now where it began, but we just popped the last element, move back
            // one element to point to the end of the collection.
            file.seek(SeekFrom::Current(-(self.element_size() as i64)))?; 
            
            self.len = self.len - 1; // Decrement len
            Ok(ret)
        }
        else {
            // The collection is empty, can't pop from an empty collection
            Err(Error::PopOnEmpty.into())
        }
    }

    /// Pops the element at the end of the collection.
    ///
    /// This will panic if the underlying file has read issues and no shadows exist or the list is
    /// empty.
    pub fn pop(&mut self) -> T {
        // TODO: Change this and try_pop to return Option<T>
        self.try_pop().unwrap()
    }

    /// Returns the size, in bytes, of the element associated with this list.
    pub fn element_size(&self) -> usize {
        std::mem::size_of::<<T as Desse>::Output>()
    }

    pub fn max_capacity() -> u64 {
        std::u64::MAX / std::mem::size_of::<<T as Desse>::Output>() as u64  
    }


    /// Copies the original underlying file into a new file at path.
    /// If a file exists there, it gets truncated.
    pub fn to_named_file<U: AsRef<std::path::Path>>(&mut self, path: U) 
        -> Result<(), Box<dyn std::error::Error>> {

        let file: &mut File = unsafe { transmute(self.file.as_ptr()) }; // Get file from cell
        let mut named_file = std::fs::OpenOptions::new()
                                .read(true)
                                .write(true)
                                .create(true)
                                .truncate(true)
                                .open(path)?;

        std::io::copy(file, &mut named_file)?;
        std::mem::drop(file); // Drop before it becomes a dangling pointer.
        self.file = named_file.into();
        Ok(())
    }

    /// Tries to insert the given element into te specified index and shifts the elements to the 
    /// right.
    ///
    /// This will return an error if index > self.len or if there's an issue with the underlying
    /// file.
    pub fn try_insert(&mut self, index: u64, element: &T) -> Result<(), Box<dyn std::error::Error>> {
        if !self.bounds_check(index) {
            return Err(Error::OutOfRange(index, self.len).into());
        }

        self.expand_if_needed()?; // Expand if the collection is currently full

        self.len = self.len + 1;
        // Increment the len for the shuffle, this also means we don't need to iterate to the len +
        // 1.

        // Shuffle all elements to the right
        for i in (index..self.len).rev() {
            let curr = self.try_get(i - 1)?;
            self.try_set(i, &curr)?;
        }
        
        self.set(index, element); // Insert the new element
        Ok(())
    }

    /// Insert the given element into te specified index and shifts the elements to the 
    /// right.
    ///
    /// This will panic if index > self.len or if there's an issue with the underlying
    /// file.
    pub fn insert(&mut self, index: u64, element: &T) {
        self.try_insert(index, element).unwrap();
    }


    /// Tries to remove the element at the specified index and shfits the elements to the left.
    ///
    /// This will return an error if index > self.len or if there's an issue with the underlying
    /// file.
    pub fn try_remove(&mut self, index: u64) -> Result<T, Box<dyn std::error::Error>> {
        if !self.bounds_check(index) {
            return Err(Error::OutOfRange(index, self.len).into());
        }

        let ret_element = self.try_get(index)?;

        for i in index..(self.len - 1) {
            let curr = self.try_get(i + 1)?;
            self.try_set(i, &curr)?;
        }
        self.len = self.len - 1;
        Ok(ret_element)
    }


    /// Removes the element at the specified index and shfits the elements to the left.
    ///
    /// This will panic if index > self.len or if there's an issue with the underlying
    /// file.
    pub fn remove(&mut self, index: u64) -> T {
        self.try_remove(index).unwrap()
    }




    fn read_at_curr_seek(&self) -> Result<T, Box<dyn std::error::Error>> {
        let file: &mut File = unsafe { transmute(self.file.as_ptr()) }; // Get file from cell
        let element_size = self.element_size();
        let mut buf = Vec::with_capacity(element_size);
        while let Err(_) =  file.take(element_size as u64).read_to_end(&mut buf) {
            // A read error occured for some reason, replace the main file with one of it's
            // shadows
            self.replace_with_shadow()?;
        }
            

        // We know the size of <T as Desse>::Output, and we know it's a u8 array of that
        // size, so even though the compilier doesn't know that, we can use transmute to treat it
        // as such. This should always be safe as long as <T as Desse>::Output is a array of u8.
        Ok(de_from::<T>(&buf)?)
    }


    fn write_at_curr_seek(&mut self, value: &T) -> Result<(), Box<dyn std::error::Error>> {
        let file: &mut File = unsafe { transmute(self.file.as_ptr()) }; // Get file from cell
        let shadows: &mut Vec<File> = unsafe { transmute(self.shadows.as_ptr()) };
        let value_ser = ser_to::<T>(value)?;

        while let Err(_) = file.write_all(value_ser.as_slice()) {
            // The write failed for some reason, replace the main file with one of it's shadows
            self.replace_with_shadow()?;
        }
        for shadow in shadows {
            //TODO if a write fails replace it with a new shadow
            shadow.write_all(value_ser.as_slice())?;
        }
        Ok(())
    }

    fn replace_with_shadow(&self) -> Result<(), Box<dyn std::error::Error>> {
        let shadows: &mut Vec<File> = unsafe { transmute(self.shadows.as_ptr()) };
        if shadows.len() == 0 {
            // This should never happen, but if it does, that means no shadows exist to replace the
            // original file, and this would only be called if the original file is no longer
            // accessible. In such a case, we are in a very bad state, so panic.
            panic!("This is a bug. Shadow replacement shouldn't occur when no shadows have been set");
        }

        self.file.replace(shadows.pop().unwrap());
        self.add_shadows(1)?;
        Ok(())

    }

    fn replace_shadow_at_index(&mut self, index: usize) -> Result<(), Box<dyn std::error::Error>> {
        let shadows: &mut Vec<File> = unsafe { transmute(self.shadows.as_ptr()) };
        let replacement = self.new_shadow()?;
        shadows[index] = replacement;
        Ok(())
    }

    /// Creates a new shadow of the VecFile's file
    fn new_shadow(&self) -> Result<File, Box<dyn std::error::Error>> {
        let file: &mut File = unsafe { transmute(self.file.as_ptr()) }; // Get file from cell
        let shadows: &mut Vec<File> = unsafe { transmute(self.shadows.as_ptr()) };
        // Continually generate temporary files until one passes the read/write test
        let mut shadow = tested_tempfile();

        shadow.set_len(self.cap * self.element_size() as u64)?;
        let mut orig_read_fail_counter = 0;
        let orig_read_fail_counter_max = 5;

        while let Err(_) = file.seek(SeekFrom::Start(0))
                                .and(std::io::copy(file, &mut shadow)) {
            // Either the seek or the copy failed, so test the dest file for writeability.        
           
            if let Ok(_) = rw_test(&mut shadow) {
                // The destination file is passing read/write tests still, so the issue
                // lies with the original file potentially.
                
                orig_read_fail_counter = orig_read_fail_counter + 1;
                if orig_read_fail_counter == orig_read_fail_counter_max {
                    // The original file has failed too many times.
                    if shadows.len() == 0 {
                        // The destination file is ok, so there's an issue with the
                        // original, and with no other shadows, the data is irrecoverable.
                        return Err(Error::IrrecoverableState.into());
                    }
                    else {
                        // Replace the original 
                        self.replace_with_shadow()?;
                    }
                }
                // Seek file back to the beginning. Unwrawpping is safe since we know it passed the
                // rw test
                shadow.seek(SeekFrom::Start(0)).unwrap();
            }

            else {
                // Something happened to the tested temp file between generation and 
                // copying data over. Generate a new one.
                shadow = tested_tempfile();
                orig_read_fail_counter = 0;
            }
        }
        self.reset_seek_to_len()?;
        shadow.seek(SeekFrom::Start(self.calc_index(self.len()).unwrap())).unwrap();
        Ok(shadow)
    }
        

}


/// Implements PartialEq for any type whose reference implements IntoIterator<Item=&T>
/// Unfortunately, because of orphaning rules, this cannot be implemented as U == VecFile
/// as well, so VecFile will always have to come first in equalities. 
impl<T, U> PartialEq<U> for VecFile<T> 
where T: Desse + DesseSized + PartialEq + Eq,
      for<'a> &'a U: IntoIterator<Item=&'a T> {

    fn eq(&self, other: &U) -> bool {
        self.into_iter().zip(other.into_iter()).all(|(e1, e2)| &e1 == e2)
    }
}
impl<T> PartialEq for VecFile<T> 
where T: Desse + DesseSized + std::fmt::Debug + PartialEq + Eq {
    fn eq(&self, other: &Self) -> bool {
        self.into_iter().zip(other.into_iter()).all(|(e1, e2)| dbg!(e1) == dbg!(e2))
    }
}


impl<T: Desse + DesseSized> Default for VecFile<T> {
    fn default() -> Self {
        Self {
            file: tested_tempfile().into(),
            shadows: Vec::with_capacity(0).into(), // Wait to allocate, since most won't use shadows
            len: 0,
            cap: 8,
            _phantom: PhantomData,
        }
    }
}


impl<T: Desse + DesseSized> Clone for VecFile<T> {
    fn clone(&self) -> Self {
        self.try_clone().unwrap()
    }
}



impl<T: Desse + DesseSized + PartialEq + Eq + std::fmt::Debug> VecFile<T> { 
    pub fn confirm_shadow_equivalence(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        
        let file: &mut File = unsafe { transmute(self.file.as_ptr()) }; // Get file from cell
        let shadows: &mut Vec<File> = unsafe { transmute(self.shadows.as_ptr()) };

        if shadows.len() > 1 {
            // Compare the first two shadows
            for i in 0..shadows.len() - 1 {
                unsafe {
                    let s1 = VecFile::<T>::from_raw_parts(shadows[i].try_clone().unwrap(),
                                                         self.len,
                                                         self.cap);
                    let s2 = VecFile::<T>::from_raw_parts(shadows[i + 1].try_clone().unwrap(),
                                                         self.len,
                                                         self.cap);

                    // Iterate through both the current shadow and next shadow and check that all
                    // elements are equal
                    if !s1.into_iter()
                             .zip(s2.into_iter())
                             .all(|(e1, e2)| e1 == e2) {
                        return Ok(false);
                    }
                }
            }
        }

        // All shadows are equivalent, or there's only one shadow.
        // Compare the first shadow to the main original
        unsafe {
            let orig = VecFile::<T>::from_raw_parts(file.try_clone().unwrap(),
                                                   self.len,
                                                   self.cap);
            let s1 = VecFile::from_raw_parts(shadows[0].try_clone().unwrap(),
                                                self.len,
                                                self.cap);


            let ret = orig.into_iter()
                            .zip(s1.into_iter())
                            .all(|(e1, e2)| e1 == e2);
            self.reset_seek_to_len()?;
            Ok(ret)
        }
    }
} 


        

impl<T: Desse + DesseSized> std::iter::IntoIterator for &VecFile<T> {
    type Item = T;
    type IntoIter = VecFileIterator<T>;

    fn into_iter(self) -> Self::IntoIter {
        let file: &mut File = unsafe { transmute(self.file.as_ptr()) }; // Get file from cell
        let mut file = file.try_clone().unwrap();
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
        let mut ret = VecFile::new();
        ret.reserve(vec.len() as u64)?;
        ret.try_extend_from_slice(&vec)?;
        Ok(ret)
    }
}

impl<T: Desse + DesseSized> std::convert::TryInto<Vec<T>> for VecFile<T> {
    type Error = Box<dyn std::error::Error>;
    fn try_into(self) -> Result<Vec<T>, Self::Error> {
        if self.len() > (std::usize::MAX as u64) {
            return Err(Error::LenExceedsUsize(self.len()).into());
        }

        Ok((&self).into_iter().collect())
    }
}



impl<T: Desse + DesseSized> std::iter::FromIterator<T> for VecFile<T> {
    fn from_iter<I: IntoIterator<Item=T>>(iter: I) -> Self {
        let mut vf = VecFile::new();
        vf.add_shadows(1).unwrap(); // To protect against potential read errors.

        for element in iter {
            vf.push(&element);
        }
        
        vf.remove_shadows(1);
        vf
    }
    
}


impl<T: Desse + DesseSized + std::fmt::Debug> std::fmt::Debug for VecFile<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut values_string = self
                            .into_iter()
                            .fold(String::new(), |acc, val| acc + &format!("{:?}, ", val));
        values_string.truncate(values_string.len() - 2); // truncates the last ',' and space

        write!(f, "[{}]", values_string)
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
           
            // We re-seek on every iteration to prevent a bug where when iterating over two
            // VecFiles at the same time, where both VecFiles are the same instance, the iterations
            // would conflict. 
            // Seeking every iteration sets the cursor to the right position even if another
            // iterator over the same VecFile exists.
            self.file.seek(SeekFrom::Start(self.counter * (std::mem::size_of::<T>() as u64))).unwrap();


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
    RWTestFailedNotEqual([u8; 4], [u8; 4]),
    IrrecoverableState,
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
            Error::RWTestFailedNotEqual(buf_in, buf_out) => 
                write!(f, "Read/Write test failed on file, output != input: {:?} != {:?}",
                       buf_in,
                       buf_out
                       ),
            Error::IrrecoverableState => 
                write!(f,
        "No available shadows for replacement and the main file is in an irrecoverable state")
        }
    }
}

impl std::error::Error for Error {}


// utility functions

pub(crate) fn de_from<T: Desse + DesseSized>(buf: &[u8]) -> Result<T, Box<dyn std::error::Error>> {
    let se_size = std::mem::size_of::<<T as Desse>::Output>();
    if buf.len() != se_size {
        return Err(Error::InequalSizeForDe(buf.len(), se_size).into());
    }

    // We know the size of <T as Desse>::Output, and we know it's a u8 array of that
    // size, so even though the compilier doesn't know that, we can use transmute to treat it
    // as such. This should always be safe as long as <T as Desse>::Output is a array of u8.
    unsafe {
        Ok(T::deserialize_from(std::mem::transmute(buf.as_ptr())))
   }
}

pub(crate) fn ser_to<T: Desse + DesseSized>(value: &T) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let se_size = std::mem::size_of::<<T as Desse>::Output>();
    let val_ser = value.serialize();
    
    // We know the size of <T as Desse>::Output, and we know it's a u8 array of that
    // size, so even though the compilier doesn't know that, we can use transmute to treat it
    // as such. This should always be safe as long as <T as Desse>::Output is a array of u8.
    unsafe {
        let ptr: * const u8 = std::mem::transmute(&val_ser as * const _);
        Ok(std::slice::from_raw_parts(ptr, se_size).to_vec())
    }


}

/// Tests reading and writing to the specified file, and returns it if it passes
pub(crate) fn rw_test(file: &mut File) -> Result<(), Box<dyn std::error::Error>> {
    let buf_in = [0, 3, 6, 1];
    let mut buf_out = [0, 0, 0, 0];
    file.write_all(&buf_in)?;
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut buf_out)?;
    file.seek(SeekFrom::Start(0))?; // Reset seek to the beginning
    if buf_in == buf_out {
        Ok(())
    }
    else {
        Err(Error::RWTestFailedNotEqual(buf_in, buf_out).into())
    }

}

/// Continually generates tempfiles until one passes the rw test.
/// They will pass the first time a vast majority of the time, but in case of some underlying OS
/// error, we can grab a new one and test and so on.
pub(crate) fn tested_tempfile() -> File {
    loop {
        if let Ok(mut file) = tempfile::tempfile() {
            if let Ok(_) = rw_test(&mut file) {
                file.set_len(0).unwrap();
                break file;
            }
        }
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
        let mut f: VecFile<u16> = VecFile::new();
        let slice = [123, 456, 789, 987, 654];
        f.extend_from_slice(&slice);

        
        assert_eq!(f.get(0), 123);
        assert_eq!(f.get(3), 987);
        assert_eq!(f.get(2), 789);
        assert_eq!(f.get(4), 654);
        assert_eq!(f.get(1), 456);
    }

    #[test]
    fn iterator() {
        let orig_values: Vec<u16> = vec![0x2222, 0xffff, 0xdddd, 0xaaaa, 0x8888];
        let mut f: VecFile<u16> = VecFile::new();
        f.extend_from_slice(orig_values.as_slice());
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
    #[test]
    fn shadows() {
        //let mut f: VecFile<u16> = vec![1, 2, 1, 2, 122, 155].try_into().unwrap();
        let mut f: VecFile<u16> = VecFile::new();
        f.add_shadows(3).unwrap();
        f.push(&1);
        f.push(&2);
        f.push(&3);
        assert!(f.confirm_shadow_equivalence().unwrap());
        f.push(&4);
        assert!(f.confirm_shadow_equivalence().unwrap());
    }

    #[test]
    fn insert_remove() {
        let orig = vec![12u32, 8, 4, 0, 4, 9, 1, 0];
        let mut vec = orig.clone();
        let mut vecf: VecFile<_> = vec.clone().try_into().unwrap();
        assert_eq!(vec.remove(1), vecf.remove(1));
        assert_eq!(vec, vecf.into_iter().collect::<Vec<_>>());
        assert_eq!(vec.remove(3), vecf.remove(3));
        assert_eq!(vec, vecf.into_iter().collect::<Vec<_>>());
        assert_eq!(vec.insert(3, 123), vecf.insert(3, &123));
        assert_eq!(vec, vecf.into_iter().collect::<Vec<_>>());

        let mut vec = orig.clone();
        let mut vecf: VecFile<_> = vec.clone().try_into().unwrap();
        vecf.insert(7, &100);
        vec.insert(7, 100);
        assert_eq!(vecf, vec);

    }

    #[test]
    fn rounded_test() {
        let mut vec = vec![12u32, 8, 4, 0, 4, 9, 1, 0];
        let mut vecf: VecFile<_> = vec.clone().try_into().unwrap();
        let vec_other = vec![512u32, 434, 2830, 3940, 32432];
        vecf.add_shadows(2).unwrap();
        vecf.push(&18);
        vecf.push(&12);

        assert_eq!(vecf.len(), (vec.len() as u64) + 2);
        vecf.pop();
        assert_eq!(vecf.len(), (vec.len() as u64) + 1);
        vecf.pop();
        assert_eq!(vec, vecf.into_iter().collect::<Vec<u32>>());
        assert!(vecf.confirm_shadow_equivalence().unwrap());
        vecf.clear_shadows();
        vecf.add_shadows(5).unwrap();
        assert!(vecf.confirm_shadow_equivalence().unwrap());
        vecf.push(&8);
        assert!(vecf.confirm_shadow_equivalence().unwrap());
        vecf.remove_shadows(3);
        assert!(vecf.confirm_shadow_equivalence().unwrap());
        vec.push(8);
        assert_eq!(vec, vecf.into_iter().collect::<Vec<u32>>());
        assert!(vecf.confirm_shadow_equivalence().unwrap());
        vec.extend_from_slice(vec_other.as_ref());
        vecf.extend_from_slice(vec_other.as_ref());
        assert_eq!(vec, vecf.into_iter().collect::<Vec<u32>>());
        assert!(vecf.confirm_shadow_equivalence().unwrap());
        vec.pop();
        vecf.pop();
        assert_eq!(vec, vecf.into_iter().collect::<Vec<u32>>());
        assert!(vecf.confirm_shadow_equivalence().unwrap());
        vec.resize(10, 0);
        vecf.resize(10, &0).unwrap();
        assert_eq!(vec, vecf.into_iter().collect::<Vec<u32>>());
        assert!(vecf.confirm_shadow_equivalence().unwrap());
        vec.truncate(3);
        vecf.truncate(3);
        let vecf2 = vecf.try_clone().unwrap();
        assert_eq!(vecf, vec);
        assert_eq!(vec, vecf.into_iter().collect::<Vec<u32>>());
        assert!(vecf.confirm_shadow_equivalence().unwrap());
        vec.truncate(0);
        vecf.truncate(0);
        assert_eq!(vec, vecf.into_iter().collect::<Vec<u32>>());
        assert!(vecf.confirm_shadow_equivalence().unwrap());
    }

    #[test]
    fn clone() {
        let vf: VecFile<u8> = (vec![5u8, 1, 3, 9]).try_into().unwrap();
        let vf_clone = vf.clone();
        assert_eq!(vf, vf_clone);
    }






}













