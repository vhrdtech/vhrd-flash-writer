pub trait MemExt<T> {
    fn kb(self) -> T;

    fn mb(self) -> T;
}

impl MemExt<u32> for u32 {
    fn kb(self) -> u32 {
        self * 1024u32
    }

    fn mb(self) -> u32 {
        self * 1_048_576u32
    }
}

impl MemExt<usize> for usize {
    fn kb(self) -> usize {
        self * 1024usize
    }

    fn mb(self) -> usize {
        self * 1_048_576usize
    }
}


