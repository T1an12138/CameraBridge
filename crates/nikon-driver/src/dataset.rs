use camera_core::{CameraError, MediaFormat};
use chrono::NaiveDateTime;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceInfoDataset {
    pub operations_supported: Vec<u16>,
    pub manufacturer: String,
    pub model: String,
    pub device_version: String,
    pub serial_number: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectInfoDataset {
    pub storage_id: u32,
    pub object_format: u16,
    pub compressed_size: u64,
    pub thumb_compressed_size: u32,
    pub image_width: u32,
    pub image_height: u32,
    pub filename: String,
    pub captured_at: Option<NaiveDateTime>,
}

impl ObjectInfoDataset {
    pub fn media_format(&self) -> Option<MediaFormat> {
        MediaFormat::from_filename(&self.filename).or(match self.object_format {
            0x3801 => Some(MediaFormat::Jpeg),
            0x3800 | 0xb101 => Some(MediaFormat::Nef),
            _ => None,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], CameraError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| CameraError::protocol("dataset offset overflow"))?;
        if end > self.bytes.len() {
            return Err(CameraError::protocol(format!(
                "truncated PTP dataset at byte {}",
                self.offset
            )));
        }
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, CameraError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u32(&mut self) -> Result<u32, CameraError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn string(&mut self) -> Result<String, CameraError> {
        let count = usize::from(*self.take(1)?.first().expect("one-byte slice"));
        if count == 0 {
            return Ok(String::new());
        }
        let units = self.take(
            count
                .checked_mul(2)
                .ok_or_else(|| CameraError::protocol("PTP string length overflow"))?,
        )?;
        let mut decoded = Vec::with_capacity(count);
        for pair in units.chunks_exact(2) {
            decoded.push(u16::from_le_bytes([pair[0], pair[1]]));
        }
        if decoded.last() != Some(&0) {
            return Err(CameraError::protocol("PTP string is not NUL terminated"));
        }
        decoded.pop();
        String::from_utf16(&decoded).map_err(|_| CameraError::protocol("invalid UTF-16 PTP string"))
    }

    fn u16_array(&mut self) -> Result<Vec<u16>, CameraError> {
        let count = usize::try_from(self.u32()?)
            .map_err(|_| CameraError::protocol("array length overflow"))?;
        (0..count).map(|_| self.u16()).collect()
    }

    fn u32_array(&mut self) -> Result<Vec<u32>, CameraError> {
        let count = usize::try_from(self.u32()?)
            .map_err(|_| CameraError::protocol("array length overflow"))?;
        (0..count).map(|_| self.u32()).collect()
    }
}

pub fn parse_device_info(bytes: &[u8]) -> Result<DeviceInfoDataset, CameraError> {
    let mut reader = Reader::new(bytes);
    let _standard_version = reader.u16()?;
    let _vendor_extension_id = reader.u32()?;
    let _vendor_extension_version = reader.u16()?;
    let _vendor_extension_description = reader.string()?;
    let _functional_mode = reader.u16()?;
    let operations_supported = reader.u16_array()?;
    let _events_supported = reader.u16_array()?;
    let _properties_supported = reader.u16_array()?;
    let _capture_formats = reader.u16_array()?;
    let _image_formats = reader.u16_array()?;
    let manufacturer = reader.string()?;
    let model = reader.string()?;
    let device_version = reader.string()?;
    let serial_number = reader.string()?;
    Ok(DeviceInfoDataset {
        operations_supported,
        manufacturer,
        model,
        device_version,
        serial_number,
    })
}

pub fn parse_storage_ids(bytes: &[u8]) -> Result<Vec<u32>, CameraError> {
    Reader::new(bytes).u32_array()
}

pub fn parse_object_handles(bytes: &[u8]) -> Result<Vec<u32>, CameraError> {
    Reader::new(bytes).u32_array()
}

pub fn parse_object_info(bytes: &[u8]) -> Result<ObjectInfoDataset, CameraError> {
    let mut reader = Reader::new(bytes);
    let storage_id = reader.u32()?;
    let object_format = reader.u16()?;
    let _protection = reader.u16()?;
    let compressed_size = u64::from(reader.u32()?);
    let _thumb_format = reader.u16()?;
    let thumb_compressed_size = reader.u32()?;
    let _thumb_width = reader.u32()?;
    let _thumb_height = reader.u32()?;
    let image_width = reader.u32()?;
    let image_height = reader.u32()?;
    let _image_bit_depth = reader.u32()?;
    let _parent = reader.u32()?;
    let _association_type = reader.u16()?;
    let _association_description = reader.u32()?;
    let _sequence = reader.u32()?;
    let filename = reader.string()?;
    let captured = reader.string()?;
    let _modified = reader.string()?;
    let _keywords = reader.string()?;
    let captured_at = if captured.is_empty() {
        None
    } else {
        NaiveDateTime::parse_from_str(&captured, "%Y%m%dT%H%M%S")
            .or_else(|_| NaiveDateTime::parse_from_str(&captured, "%Y%m%dT%H%M%S.0"))
            .ok()
    };
    Ok(ObjectInfoDataset {
        storage_id,
        object_format,
        compressed_size,
        thumb_compressed_size,
        image_width,
        image_height,
        filename,
        captured_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_string(out: &mut Vec<u8>, value: &str) {
        let units: Vec<_> = value.encode_utf16().collect();
        out.push(u8::try_from(units.len() + 1).unwrap());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out.extend_from_slice(&0_u16.to_le_bytes());
    }

    #[test]
    fn parses_minimal_device_info() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&100_u16.to_le_bytes());
        bytes.extend_from_slice(&6_u32.to_le_bytes());
        bytes.extend_from_slice(&100_u16.to_le_bytes());
        push_string(&mut bytes, "Nikon");
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&0x1001_u16.to_le_bytes());
        for _ in 0..4 {
            bytes.extend_from_slice(&0_u32.to_le_bytes());
        }
        push_string(&mut bytes, "Nikon");
        push_string(&mut bytes, "Z 6_2");
        push_string(&mut bytes, "C 1.70");
        push_string(&mut bytes, "123");
        let info = parse_device_info(&bytes).unwrap();
        assert_eq!(info.model, "Z 6_2");
        assert_eq!(info.operations_supported, vec![0x1001]);
    }
}
