use byteorder::{LittleEndian, WriteBytesExt};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

#[derive(Debug)]
pub enum Group {
    Group1,
    Group2,
}

#[derive(Debug)]
pub struct IceFile {
    pub filename: String,
    pub file_ext: Option<String>,
    pub data: Vec<u8>,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct IceFlags {
    pub encrypted: bool,
    pub compressed: bool,
    pub oodle: bool,
    pub vita: bool,
}

struct IceInfo {
    crc32: u32,
    flags: u32,
    filesize: u32,
}

#[derive(Debug, Default)]
struct IceGroup {
    group1: IceGroupEntry,
    group2: IceGroupEntry,
    group1_size: u32,
    group2_size: u32,
    key: u32,
}

#[derive(Debug, Default)]
struct IceGroupEntry {
    original_size: u32,
    data_size: u32,
    filecount: u32,
    crc32: u32,
}

impl IceInfo {
    fn new() -> Self {
        Self {
            crc32: 0,
            flags: 0,
            filesize: 0,
        }
    }

    fn write(self, writer: &mut impl Write) -> Result<(), std::io::Error> {
        writer.write_u32::<LittleEndian>(0xFF)?;
        writer.write_u32::<LittleEndian>(self.crc32)?;
        writer.write_u32::<LittleEndian>(self.flags)?;
        writer.write_u32::<LittleEndian>(self.filesize)?;
        Ok(())
    }
}

impl IceGroup {
    fn write(&self, writer: &mut impl Write) -> Result<(), std::io::Error> {
        self.group1.write(writer)?;
        self.group2.write(writer)?;
        writer.write_u32::<LittleEndian>(self.group1_size)?;
        writer.write_u32::<LittleEndian>(self.group2_size)?;
        writer.write_u32::<LittleEndian>(self.key)?;
        writer.write_u32::<LittleEndian>(0)?;
        Ok(())
    }
} //impl IceGroup

impl IceGroupEntry {
    fn write(&self, writer: &mut impl Write) -> Result<(), std::io::Error> {
        writer.write_u32::<LittleEndian>(self.original_size)?;
        writer.write_u32::<LittleEndian>(self.data_size)?;
        writer.write_u32::<LittleEndian>(self.filecount)?;
        writer.write_u32::<LittleEndian>(self.crc32)?;
        Ok(())
    }
}

fn write_header(writer: &mut impl Write) -> Result<(), std::io::Error> {
    writer.write_all(b"ICE\0")?;
    writer.write_u32::<LittleEndian>(0)?;
    writer.write_u32::<LittleEndian>(4)?;
    writer.write_u32::<LittleEndian>(0x80)?;
    Ok(())
}

#[derive(Debug, Default)]
pub struct IceGroupInfo {
    pub fullsize: u32,
    pub filecount: u32,
}
#[derive(Debug, Default)]
pub struct IceFileInfo {
    pub filename: String,
    pub file_extension: String,
    pub file_size: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Default)]
struct IceFileHeader {
    file_ext: String,
    data_size: u32,
    filename: String,
}
impl IceFileHeader {
    fn write(&self, writer: &mut impl Write) -> Result<u32, std::io::Error> {
        let filename_size = self.filename.chars().filter(char::is_ascii).count();
        writer.write_all(&write_utf8(&self.file_ext, 4))?;
        let alligned_filename_size = align(self.filename.len() + 1, 0x10);
        let data_offset = alligned_filename_size + 0x40; // size of header
        let mut full_size = data_offset + self.data_size as usize;
        if self.file_ext.contains("dds") {
            full_size += self.data_size as usize % 0x10;
        }
        let padding = (align(full_size, 0x10) - full_size) as u32;
        writer.write_u32::<LittleEndian>(full_size as u32 + padding)?;
        writer.write_u32::<LittleEndian>(self.data_size)?;
        writer.write_u32::<LittleEndian>(data_offset as u32)?;
        writer.write_u32::<LittleEndian>(filename_size as u32 + 1)?;
        //flags
        writer.write_u32::<LittleEndian>(0)?;
        for _ in 0..10 {
            writer.write_u32::<LittleEndian>(0)?;
        }
        writer.write_all(&write_utf8(&self.filename, alligned_filename_size))?;
        Ok(padding)
    }
}

fn write_utf8(string: &str, len: usize) -> Vec<u8> {
    string
        .chars()
        .take(len)
        .filter(char::is_ascii)
        .map(|c| c as u8)
        .chain([0].into_iter().cycle())
        .take(len)
        .collect()
}

#[derive(Debug)]
struct GroupWrite {
    buffer: Cursor<Vec<u8>>,
    file_size: usize,
    header: Vec<IceFileHeader>,
}

#[derive(Debug)]
pub struct IceWriter<W: Write> {
    writer: W,
    group1: GroupWrite,
    group2: GroupWrite,
    current_group: Group,
}
impl<W: Write> IceWriter<W> {
    pub fn new(writer: W) -> Result<Self, std::io::Error> {
        let group1 = GroupWrite {
            buffer: Cursor::new(vec![0u8; 0]),
            file_size: 0,
            header: vec![],
        };
        let group2 = GroupWrite {
            buffer: Cursor::new(vec![0u8; 0]),
            file_size: 0,
            header: vec![],
        };
        Ok(Self {
            writer,
            group1,
            group2,
            current_group: Group::Group1,
        })
    }
    pub fn load_group(&mut self, group: Group) {
        self.current_group = group;
    }
    pub fn new_file(&mut self, file: IceFileInfo) -> Result<(), std::io::Error> {
        let group = match self.current_group {
            Group::Group1 => &mut self.group1,
            Group::Group2 => &mut self.group2,
        };
        if let Some(x) = group.header.last_mut() {
            x.data_size = group.file_size as u32;
        }
        group.file_size = 0;
        group.header.push(IceFileHeader {
            file_ext: file.file_extension,
            filename: file.filename,
            ..Default::default()
        });
        if !file.data.is_empty() {
            self.write_all(&file.data[..])?;
        }
        Ok(())
    }
    fn finalize(&mut self) -> Result<(), std::io::Error> {
        if let Some(x) = self.group1.header.last_mut() {
            x.data_size = self.group1.file_size as u32;
        }
        if let Some(x) = self.group2.header.last_mut() {
            x.data_size = self.group2.file_size as u32;
        }
        self.group1.buffer.seek(SeekFrom::Start(0))?;
        self.group2.buffer.seek(SeekFrom::Start(0))?;
        let mut group_info = IceGroup::default();
        let mut group1_buf = Cursor::new(vec![0u8; 0]);
        let mut group2_buf = Cursor::new(vec![0u8; 0]);

        for file in &self.group1.header {
            let padding = file.write(&mut group1_buf)?;
            std::io::copy(
                &mut std::io::Read::by_ref(&mut self.group1.buffer).take(file.data_size as u64),
                &mut group1_buf,
            )?;
            group1_buf.write_all(&vec![0u8; padding as usize])?;
            group_info.group1.filecount += 1;
        }
        self.group1.buffer = Cursor::new(vec![0u8; 0]);
        for file in &self.group2.header {
            let padding = file.write(&mut group2_buf)?;
            std::io::copy(
                &mut std::io::Read::by_ref(&mut self.group2.buffer).take(file.data_size as u64),
                &mut group2_buf,
            )?;
            group2_buf.write_all(&vec![0u8; padding as usize])?;
            group_info.group2.filecount += 1;
        }
        self.group2.buffer = Cursor::new(vec![0u8; 0]);
        group_info.group1.original_size = group1_buf.stream_position()? as u32;
        group_info.group2.original_size = group2_buf.stream_position()? as u32;

        let mut total_group = Cursor::new(vec![0u8; 0]);
        group1_buf.seek(SeekFrom::Start(0))?;
        let last_pos = total_group.stream_position()?;
        std::io::copy(&mut group1_buf, &mut total_group)?;
        let size = (total_group.stream_position()? - last_pos) as u32;
        group_info.group1_size = size;
        group_info.group1.data_size = 0;
        drop(group1_buf);
        group2_buf.seek(SeekFrom::Start(0))?;
        let last_pos = total_group.stream_position()?;
        std::io::copy(&mut group2_buf, &mut total_group)?;
        let size = (total_group.stream_position()? - last_pos) as u32;
        group_info.group2_size = size;
        group_info.group2.data_size = 0;
        drop(group2_buf);
        total_group.seek(SeekFrom::Start(0))?;

        let mut ice_info = IceInfo::new();
        ice_info.flags = 0;
        // ice header + ice info + enc table + groups header
        ice_info.filesize =
            0x10 + 0x10 + 0x100 + 0x30 + group_info.group1_size + group_info.group2_size;

        group_info.group1_size = 0;
        group_info.group2_size = 0;

        let t = [0u8; 0x100];
        let mut group_header_data: Vec<u8> = Vec::with_capacity(0x30);
        let mut crc_hasher = crc32fast::Hasher::new();
        group_info.group1.crc32 = calc_crc(
            &mut std::io::Read::by_ref(&mut total_group).take(group_info.group1_size as u64),
            &mut crc_hasher,
        )?;
        group_info.group2.crc32 = calc_crc(&mut total_group, &mut crc_hasher)?;
        ice_info.crc32 = crc_hasher.finalize();
        total_group.seek(SeekFrom::Start(0))?;

        group_info.write(&mut group_header_data)?;
        write_header(&mut self.writer)?;
        ice_info.write(&mut self.writer)?;
        self.writer.write_all(&t)?;
        self.writer.write_all(&group_header_data)?;
        std::io::copy(&mut total_group, &mut self.writer)?;
        Ok(())
    }
    pub fn into_inner(mut self) -> Result<W, (W, std::io::Error)> {
        match self.finalize() {
            Ok(_) => Ok(self.writer),
            Err(e) => Err((self.writer, e.into())),
        }
    }
}

fn calc_crc(
    reader: &mut impl Read,
    total_crc: &mut crc32fast::Hasher,
) -> Result<u32, std::io::Error> {
    let mut group_hash = crc32fast::Hasher::new();
    let mut buf = Vec::with_capacity(1024);
    loop {
        buf.clear();
        reader.by_ref().take(1024).read_to_end(&mut buf)?;
        if buf.is_empty() {
            break;
        }
        group_hash.update(&buf);
        total_crc.update(&buf);
    }
    Ok(group_hash.finalize())
}

impl<W: Write> Write for IceWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let (write_buf, file_size) = match self.current_group {
            Group::Group1 => (&mut self.group1.buffer, &mut self.group1.file_size),
            Group::Group2 => (&mut self.group2.buffer, &mut self.group2.file_size),
        };
        let wrote = write_buf.write(buf)?;
        *file_size += wrote;
        Ok(wrote)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.group1.buffer.flush()?;
        self.group2.buffer.flush()
    }
}

pub fn align(size: usize, align_to: usize) -> usize {
    (size + align_to - 1) & (usize::MAX ^ (align_to - 1))
}
