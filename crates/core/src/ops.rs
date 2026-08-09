//! Business-level operations built on the abstract [`crate::backend::Backend`].
//!
//! These implement the historical workflow: erase → write → verify, and the
//! standalone read/verify operations. No hardware knowledge lives here.

use crate::backend::Backend;
use crate::error::Result;
use crate::memory::{MemoryImage, PROGRAM_WORDS};

/// Story of a program (write) combined with an automatic read-back verify.
pub fn program(backend: &mut dyn Backend, image: &MemoryImage) -> Result<bool> {
    backend.program(image)
}

/// Read the full microcontroller state into an image.
pub fn read(backend: &mut dyn Backend) -> Result<MemoryImage> {
    let mut img = MemoryImage::new();
    // Backend exposes a slice write; default to program size.
    let mut prog = vec![0u16; PROGRAM_WORDS];
    backend.read_program(&mut prog)?;
    img.program = prog;
    img.data = backend.read_data()?;
    Ok(img)
}

/// Independently verify an image against the device.
pub fn verify(backend: &mut dyn Backend, image: &MemoryImage) -> Result<bool> {
    let mut read_back = vec![0u16; PROGRAM_WORDS];
    backend.read_program(&mut read_back)?;
    let img_read = MemoryImage {
        program: read_back,
        ..image.clone()
    };
    let data_ok = backend.read_data()? == image.data;
    Ok(img_read.program == image.program && data_ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::DeviceId;

    struct MemoryBackend {
        image: MemoryImage,
    }

    impl Backend for MemoryBackend {
        fn identify(&mut self) -> Result<DeviceId> {
            Ok(DeviceId {
                name: "mock".into(),
                revision: None,
            })
        }
        fn erase(&mut self) -> Result<()> {
            self.image = MemoryImage::new();
            Ok(())
        }
        fn write_program(&mut self, words: &[u16]) -> Result<()> {
            self.image.program[..words.len().min(PROGRAM_WORDS)].copy_from_slice(words);
            Ok(())
        }
        fn write_data(&mut self, bytes: &[u8; 64]) -> Result<()> {
            self.image.data = *bytes;
            Ok(())
        }
        fn read_program(&mut self, out: &mut [u16]) -> Result<()> {
            out.copy_from_slice(&self.image.program[..out.len()]);
            Ok(())
        }
        fn read_data(&mut self) -> Result<[u8; 64]> {
            Ok(self.image.data)
        }
        fn program(&mut self, image: &MemoryImage) -> Result<bool> {
            self.write_program(&image.program)?;
            self.write_data(&image.data)?;
            Ok(true)
        }
    }

    #[test]
    fn read_roundtrip_with_mock() {
        let mut backend = MemoryBackend {
            image: MemoryImage::new(),
        };
        let mut expected = MemoryImage::new();
        expected.program[7] = 0xABCD;
        backend.program(&expected).unwrap();
        let img = read(&mut backend).unwrap();
        assert_eq!(img.program[7], 0xABCD);
    }

    #[test]
    fn verify_matches() {
        let mut backend = MemoryBackend {
            image: MemoryImage::new(),
        };
        let mut img = MemoryImage::new();
        img.program[3] = 0xFF00;
        backend.program(&img).unwrap();
        // An empty image does NOT match.
        assert!(!verify(&mut backend, &MemoryImage::new()).unwrap());
        assert!(verify(&mut backend, &img).unwrap());
    }
}
