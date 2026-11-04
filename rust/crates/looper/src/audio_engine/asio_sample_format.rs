//! ASIO PCM boundary. The shared renderer remains interleaved float stereo.

#[derive(Clone, Copy, Debug)]
pub(super) struct SampleLayout {
    width: usize,
    bits: u32,
    floating: bool,
    big_endian: bool,
}

impl SampleLayout {
    pub fn from_asio(sample_type: i32) -> Option<Self> {
        let (width, bits, floating, big_endian) = match sample_type {
            0 => (2, 16, false, true),
            16 => (2, 16, false, false),
            1 => (3, 24, false, true),
            17 => (3, 24, false, false),
            2 => (4, 32, false, true),
            18 => (4, 32, false, false),
            3 => (4, 32, true, true),
            19 => (4, 32, true, false),
            4 => (8, 64, true, true),
            20 => (8, 64, true, false),
            8..=11 => (4, [16, 18, 20, 24][(sample_type - 8) as usize], false, true),
            24..=27 => (
                4,
                [16, 18, 20, 24][(sample_type - 24) as usize],
                false,
                false,
            ),
            _ => return None, // DSD is not PCM and must never be misinterpreted.
        };
        Some(Self {
            width,
            bits,
            floating,
            big_endian,
        })
    }

    fn encode(self, value: f32) -> [u8; 8] {
        let value = if value.is_finite() { value } else { 0.0 };
        let mut bytes = [0; 8];
        if self.floating {
            if self.bits == 32 {
                let encoded = if self.big_endian {
                    value.to_be_bytes()
                } else {
                    value.to_le_bytes()
                };
                bytes[..4].copy_from_slice(&encoded);
            } else {
                bytes = if self.big_endian {
                    f64::from(value).to_be_bytes()
                } else {
                    f64::from(value).to_le_bytes()
                };
            }
        } else {
            let scale = (1u64 << (self.bits - 1)) as f64;
            let integer = (f64::from(value.clamp(-1.0, 1.0)) * scale)
                .round()
                .clamp(-scale, scale - 1.0) as i32;
            // SDK Int32{LSB,MSB}{16,18,20,24} stores signed low-N bits,
            // sign-extended in a 32-bit container. Byte order is independent.
            let encoded = if self.big_endian {
                integer.to_be_bytes()
            } else {
                integer.to_le_bytes()
            };
            let source = if self.big_endian {
                &encoded[4 - self.width..]
            } else {
                &encoded[..self.width]
            };
            bytes[..self.width].copy_from_slice(source);
        }
        bytes
    }

    /// The selected driver's two planes are valid, nonoverlapping, writable for
    /// frames*width bytes until callback return. Caller proves this before Start.
    pub unsafe fn write_channel(self, stereo: &[f32], channel: usize, output: *mut u8) {
        for (frame, pair) in stereo.chunks_exact(2).enumerate() {
            let bytes = self.encode(pair[channel]);
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    output.add(frame * self.width),
                    self.width,
                )
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_endpoints_sign_extension_rounding_and_byte_order() {
        for (code, bits) in [(24, 16), (25, 18), (26, 20), (27, 24), (18, 32)] {
            let layout = SampleLayout::from_asio(code).unwrap();
            let scale = 1i64 << (bits - 1);
            for (value, expected) in [
                (-2.0, -scale),
                (-1.0, -scale),
                (0.0, 0),
                (1.0, scale - 1),
                (2.0, scale - 1),
                (f32::NAN, 0),
            ] {
                let encoded = layout.encode(value);
                assert_eq!(
                    i32::from_le_bytes(encoded[..4].try_into().unwrap()) as i64,
                    expected
                );
                let big = SampleLayout::from_asio(code - 16).unwrap().encode(value);
                assert_eq!(
                    i32::from_be_bytes(big[..4].try_into().unwrap()) as i64,
                    expected
                );
            }
            assert_eq!(
                i32::from_le_bytes(layout.encode(-0.5)[..4].try_into().unwrap()) as i64,
                -scale / 2
            );
        }
        assert_eq!(
            &SampleLayout::from_asio(16).unwrap().encode(-1.0)[..2],
            &[0, 128]
        );
        assert_eq!(
            &SampleLayout::from_asio(1).unwrap().encode(-1.0)[..3],
            &[128, 0, 0]
        );
        assert!(SampleLayout::from_asio(32).is_none());
    }

    #[test]
    fn selected_planes_use_independent_formats_without_touching_guards() {
        let mut left = [0xa5; 12];
        let mut right = [0x5a; 20];
        let stereo = [0.5, -0.5, -1.0, 1.0];
        unsafe {
            SampleLayout::from_asio(16).unwrap().write_channel(
                &stereo,
                0,
                left.as_mut_ptr().add(4),
            );
            SampleLayout::from_asio(20).unwrap().write_channel(
                &stereo,
                1,
                right.as_mut_ptr().add(2),
            );
        }
        assert_eq!(&left[..4], &[0xa5; 4]);
        assert_eq!(&left[8..], &[0xa5; 4]);
        assert_eq!(&left[4..8], &[0, 64, 0, 128]);
        assert_eq!(&right[..2], &[0x5a; 2]);
        assert_eq!(&right[18..], &[0x5a; 2]);
        assert_eq!(f64::from_le_bytes(right[2..10].try_into().unwrap()), -0.5);
        assert_eq!(f64::from_le_bytes(right[10..18].try_into().unwrap()), 1.0);
    }
}
