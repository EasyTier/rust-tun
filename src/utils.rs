use std::io::IoSliceMut;
use std::ops::Deref;

pub(crate) trait SliceExt {
    fn size(&self) -> usize;
}

impl<T, B> SliceExt for [T]
where
    T: Deref<Target = [B]>,
{
    #[inline]
    fn size(&self) -> usize {
        self.iter().map(|b| b.len()).sum()
    }
}

#[allow(unused)]
pub(crate) trait DataExt {
    fn put(&self, buf: &mut [u8]) -> usize;
    fn putv(&self, bufs: &mut [IoSliceMut<'_>]) -> usize;
}

impl<T: AsRef<[u8]> + ?Sized> DataExt for T {
    fn put(&self, buf: &mut [u8]) -> usize {
        let slice = self.as_ref();
        let len = slice.len().min(buf.len());
        buf[..len].copy_from_slice(&slice[..len]);
        len
    }

    fn putv(&self, bufs: &mut [IoSliceMut<'_>]) -> usize {
        let slice = self.as_ref();
        let mut offset = 0;
        let mut remaining = slice.len();
        for buf in bufs.iter_mut() {
            if remaining == 0 {
                break;
            }
            let len = remaining.min(buf.len());
            if len == 0 {
                continue;
            }
            buf[..len].copy_from_slice(&slice[offset..offset + len]);
            offset += len;
            remaining -= len;
        }
        offset
    }
}
