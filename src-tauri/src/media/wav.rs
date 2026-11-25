//! Minimal WAV reading/slicing for the 16 kHz mono PCM files CaptionNest extracts.
//! Slices are copied straight from disk, so long videos never load fully into memory.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WavInfo {
    pub data_offset: u64,
    pub data_len: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits: u16,
}

impl WavInfo {
    fn bytes_per_ms(&self) -> f64 {
        self.sample_rate as f64 * self.channels as f64 * (self.bits as f64 / 8.0) / 1000.0
    }

    #[cfg(test)]
    pub fn duration_ms(&self) -> i64 {
        (self.data_len as f64 / self.bytes_per_ms()) as i64
    }

    fn frame_bytes(&self) -> u64 {
        self.channels as u64 * (self.bits as u64 / 8)
    }
}

/// A 16 kHz mono 16-bit WAV of `ms` milliseconds of silence (for checking API keys).
pub fn silence(ms: u32) -> Vec<u8> {
    let samples = 16 * ms;
    let mut wav = Vec::with_capacity(44 + samples as usize * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&16_000u32.to_le_bytes());
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(samples * 2).to_le_bytes());
    wav.resize(44 + samples as usize * 2, 0);
    wav
}

/// Finds the format and data chunks (skipping LIST and other chunks FFmpeg may write).
pub fn read_info(path: &Path) -> io::Result<WavInfo> {
    let mut f = File::open(path)?;
    let mut header = [0u8; 12];
    f.read_exact(&mut header)?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a WAV file"));
    }
    let (mut rate, mut channels, mut bits) = (0u32, 0u16, 0u16);
    loop {
        let mut ch = [0u8; 8];
        f.read_exact(&mut ch)?;
        let size = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]) as u64;
        match &ch[0..4] {
            b"fmt " => {
                let mut fmt = vec![0u8; size as usize];
                f.read_exact(&mut fmt)?;
                channels = u16::from_le_bytes([fmt[2], fmt[3]]);
                rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                bits = u16::from_le_bytes([fmt[14], fmt[15]]);
                if size % 2 == 1 {
                    f.seek(SeekFrom::Current(1))?;
                }
            }
            b"data" => {
                let offset = f.stream_position()?;
                let file_len = f.metadata()?.len();
                // FFmpeg writes 0xFFFFFFFF or a placeholder when streaming; trust the file size.
                let len = if size == 0 || size == u32::MAX as u64 || offset + size > file_len { file_len - offset } else { size };
                if rate == 0 || channels == 0 || bits == 0 {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "WAV format chunk missing"));
                }
                return Ok(WavInfo { data_offset: offset, data_len: len, sample_rate: rate, channels, bits });
            }
            _ => {
                f.seek(SeekFrom::Current((size + size % 2) as i64))?;
            }
        }
    }
}

/// Writes `[start_ms, end_ms)` of `src` as its own WAV file.
pub fn write_slice(src: &Path, info: &WavInfo, start_ms: i64, end_ms: i64, dst: &Path) -> io::Result<()> {
    let bpm = info.bytes_per_ms();
    let frame = info.frame_bytes();
    let align = |ms: i64| -> u64 { ((ms.max(0) as f64 * bpm) as u64 / frame * frame).min(info.data_len) };
    let (a, b) = (align(start_ms), align(end_ms));
    let len = b.saturating_sub(a);
    let mut f = File::open(src)?;
    f.seek(SeekFrom::Start(info.data_offset + a))?;
    let mut out = io::BufWriter::new(File::create(dst)?);
    write_header(&mut out, info, len as u32)?;
    io::copy(&mut f.take(len), &mut out)?;
    out.flush()
}

fn write_header(w: &mut impl Write, info: &WavInfo, data_len: u32) -> io::Result<()> {
    let block_align = info.channels * info.bits / 8;
    let byte_rate = info.sample_rate * block_align as u32;
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_len).to_le_bytes())?;
    w.write_all(b"WAVEfmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?;
    w.write_all(&info.channels.to_le_bytes())?;
    w.write_all(&info.sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&block_align.to_le_bytes())?;
    w.write_all(&info.bits.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_wav(path: &Path, seconds: u32, with_list_chunk: bool) {
        let info = WavInfo { data_offset: 0, data_len: 0, sample_rate: 16_000, channels: 1, bits: 16 };
        let samples: Vec<u8> = (0..16_000 * seconds).flat_map(|i| ((i % 1000) as i16).to_le_bytes()).collect();
        let mut f = File::create(path).unwrap();
        if with_list_chunk {
            // RIFF header, fmt, LIST, data — like FFmpeg's output.
            let list = b"INFOISFT\x0e\x00\x00\x00Lavf61.7.100\x00\x00";
            let total = 4 + (8 + 16) + (8 + list.len() as u32) + (8 + samples.len() as u32);
            f.write_all(b"RIFF").unwrap();
            f.write_all(&total.to_le_bytes()).unwrap();
            f.write_all(b"WAVEfmt ").unwrap();
            f.write_all(&16u32.to_le_bytes()).unwrap();
            f.write_all(&[1, 0, 1, 0]).unwrap();
            f.write_all(&16_000u32.to_le_bytes()).unwrap();
            f.write_all(&32_000u32.to_le_bytes()).unwrap();
            f.write_all(&[2, 0, 16, 0]).unwrap();
            f.write_all(b"LIST").unwrap();
            f.write_all(&(list.len() as u32).to_le_bytes()).unwrap();
            f.write_all(list).unwrap();
            f.write_all(b"data").unwrap();
            f.write_all(&(samples.len() as u32).to_le_bytes()).unwrap();
        } else {
            write_header(&mut f, &info, samples.len() as u32).unwrap();
        }
        f.write_all(&samples).unwrap();
    }

    #[test]
    fn reads_ffmpeg_style_header_with_list_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        make_wav(&p, 3, true);
        let info = read_info(&p).unwrap();
        assert_eq!((info.sample_rate, info.channels, info.bits), (16_000, 1, 16));
        assert_eq!(info.duration_ms(), 3_000);
    }

    #[test]
    fn slices_exact_sample_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        make_wav(&p, 4, false);
        let info = read_info(&p).unwrap();
        let out = dir.path().join("b.wav");
        write_slice(&p, &info, 1_000, 2_500, &out).unwrap();
        let sliced = read_info(&out).unwrap();
        assert_eq!(sliced.duration_ms(), 1_500);
        // First sample of the slice is sample 16000 of the source.
        let mut f = File::open(&out).unwrap();
        f.seek(SeekFrom::Start(sliced.data_offset)).unwrap();
        let mut two = [0u8; 2];
        f.read_exact(&mut two).unwrap();
        assert_eq!(i16::from_le_bytes(two), (16_000 % 1000) as i16);
    }

    #[test]
    fn slice_past_end_is_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        make_wav(&p, 2, false);
        let info = read_info(&p).unwrap();
        let out = dir.path().join("b.wav");
        write_slice(&p, &info, 1_500, 9_000, &out).unwrap();
        assert_eq!(read_info(&out).unwrap().duration_ms(), 500);
    }
}
