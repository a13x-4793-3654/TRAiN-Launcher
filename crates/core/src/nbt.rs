//! `servers.dat`(マルチプレイのサーバー一覧)を読み書きするための最小限のNBT実装。
//!
//! 未知のタグもそのまま保持して書き戻すため、公式ランチャーや他のツールが書いた内容を
//! 壊さない([`crate::server_list`]参照)。[TRAiN-Setup](https://github.com/a13x-4793-3654/TRAiN-Setup)
//! の `TrainClientSetup/Nbt.cs` と同じ設計を踏襲している(ビッグエンディアン、文字列は
//! Javaの「修正UTF-8」)。
//!
//! 対応タグ型: Byte/Short/Int/Long/Float/Double/ByteArray/String/List/Compound/IntArray/LongArray。
//! `servers.dat` に必要な範囲のみを実装しており、汎用NBTライブラリではない。

use std::io::{self, Read, Write};

use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TagType {
    End = 0,
    Byte = 1,
    Short = 2,
    Int = 3,
    Long = 4,
    Float = 5,
    Double = 6,
    ByteArray = 7,
    String = 8,
    List = 9,
    Compound = 10,
    IntArray = 11,
    LongArray = 12,
}

impl TagType {
    fn from_u8(value: u8) -> Result<Self, CoreError> {
        Ok(match value {
            0 => Self::End,
            1 => Self::Byte,
            2 => Self::Short,
            3 => Self::Int,
            4 => Self::Long,
            5 => Self::Float,
            6 => Self::Double,
            7 => Self::ByteArray,
            8 => Self::String,
            9 => Self::List,
            10 => Self::Compound,
            11 => Self::IntArray,
            12 => Self::LongArray,
            other => {
                return Err(CoreError::InvalidNbt(format!(
                    "未知のNBTタグです (type={other})"
                )))
            }
        })
    }
}

/// 1個のNBTタグの値。キーの並び順を保つため、[`Tag::Compound`]は`Vec`で持つ
/// (`BTreeMap`ではソートされて書き戻し順が変わってしまうため使わない)。
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    /// 要素型と要素本体。空リストの要素型は仕様上 `End` になり得る。
    List(TagType, Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    fn type_of(&self) -> TagType {
        match self {
            Tag::Byte(_) => TagType::Byte,
            Tag::Short(_) => TagType::Short,
            Tag::Int(_) => TagType::Int,
            Tag::Long(_) => TagType::Long,
            Tag::Float(_) => TagType::Float,
            Tag::Double(_) => TagType::Double,
            Tag::ByteArray(_) => TagType::ByteArray,
            Tag::String(_) => TagType::String,
            Tag::List(_, _) => TagType::List,
            Tag::Compound(_) => TagType::Compound,
            Tag::IntArray(_) => TagType::IntArray,
            Tag::LongArray(_) => TagType::LongArray,
        }
    }
}

/// [`Tag::Compound`]の中身を、キー順を保ったまま操作するための薄いラッパー。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Compound(pub Vec<(String, Tag)>);

impl Compound {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, name: &str) -> Option<&Tag> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    pub fn get_string(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(Tag::String(value)) => Some(value.as_str()),
            _ => None,
        }
    }

    pub fn set(&mut self, name: impl Into<String>, value: Tag) {
        let name = name.into();
        if let Some(entry) = self.0.iter_mut().find(|(key, _)| *key == name) {
            entry.1 = value;
        } else {
            self.0.push((name, value));
        }
    }

    pub fn into_tag(self) -> Tag {
        Tag::Compound(self.0)
    }

    fn from_tag(tag: Tag) -> Result<Self, CoreError> {
        match tag {
            Tag::Compound(entries) => Ok(Self(entries)),
            other => Err(CoreError::InvalidNbt(format!(
                "TAG_Compoundを期待しましたが {:?} でした",
                other.type_of()
            ))),
        }
    }
}

/// ルートの`TAG_Compound`を読み込む(NBTのルートは常に無名の`TAG_Compound`)。
pub fn read<R: Read>(reader: &mut R) -> Result<Compound, CoreError> {
    let tag_type = read_u8(reader)?;
    let tag_type = TagType::from_u8(tag_type)?;
    if tag_type != TagType::Compound {
        return Err(CoreError::InvalidNbt(format!(
            "NBTのルートがTAG_Compoundではありません (type={tag_type:?})"
        )));
    }
    let _name = read_name(reader)?;
    Compound::from_tag(read_payload(reader, TagType::Compound)?)
}

/// ルートの`TAG_Compound`を書き出す。
pub fn write<W: Write>(writer: &mut W, root: &Compound) -> Result<(), CoreError> {
    write_u8(writer, TagType::Compound as u8)?;
    write_name(writer, "")?;
    write_payload(writer, &root.clone().into_tag())?;
    Ok(())
}

fn read_payload<R: Read>(reader: &mut R, tag_type: TagType) -> Result<Tag, CoreError> {
    Ok(match tag_type {
        TagType::End => {
            return Err(CoreError::InvalidNbt(
                "TAG_Endの値は読み取れません".to_string(),
            ))
        }
        TagType::Byte => Tag::Byte(read_u8(reader)? as i8),
        TagType::Short => Tag::Short(read_i16(reader)?),
        TagType::Int => Tag::Int(read_i32(reader)?),
        TagType::Long => Tag::Long(read_i64(reader)?),
        TagType::Float => Tag::Float(f32::from_bits(read_i32(reader)? as u32)),
        TagType::Double => Tag::Double(f64::from_bits(read_i64(reader)? as u64)),
        TagType::ByteArray => {
            let len = read_length(reader)?;
            Tag::ByteArray(read_exact(reader, len)?)
        }
        TagType::String => Tag::String(read_name(reader)?),
        TagType::List => {
            let element_type = TagType::from_u8(read_u8(reader)?)?;
            let len = read_length(reader)?;
            let mut items = Vec::with_capacity(len);
            // 空リストの要素型はTAG_Endになることがあるため、その場合は読み進めない。
            if element_type != TagType::End {
                for _ in 0..len {
                    items.push(read_payload(reader, element_type)?);
                }
            }
            Tag::List(element_type, items)
        }
        TagType::Compound => {
            let mut entries = Vec::new();
            loop {
                let entry_type = TagType::from_u8(read_u8(reader)?)?;
                if entry_type == TagType::End {
                    break;
                }
                let name = read_name(reader)?;
                entries.push((name, read_payload(reader, entry_type)?));
            }
            Tag::Compound(entries)
        }
        TagType::IntArray => {
            let len = read_length(reader)?;
            let mut values = Vec::with_capacity(len);
            for _ in 0..len {
                values.push(read_i32(reader)?);
            }
            Tag::IntArray(values)
        }
        TagType::LongArray => {
            let len = read_length(reader)?;
            let mut values = Vec::with_capacity(len);
            for _ in 0..len {
                values.push(read_i64(reader)?);
            }
            Tag::LongArray(values)
        }
    })
}

fn write_payload<W: Write>(writer: &mut W, tag: &Tag) -> Result<(), CoreError> {
    match tag {
        Tag::Byte(value) => write_u8(writer, *value as u8)?,
        Tag::Short(value) => write_i16(writer, *value)?,
        Tag::Int(value) => write_i32(writer, *value)?,
        Tag::Long(value) => write_i64(writer, *value)?,
        Tag::Float(value) => write_i32(writer, value.to_bits() as i32)?,
        Tag::Double(value) => write_i64(writer, value.to_bits() as i64)?,
        Tag::ByteArray(value) => {
            write_length(writer, value.len())?;
            writer.write_all(value)?;
        }
        Tag::String(value) => write_name(writer, value)?,
        Tag::List(declared_type, items) => {
            // 実際の要素があればその型を優先する(異種混在のリストは想定しない)。
            let element_type = items.first().map(Tag::type_of).unwrap_or(*declared_type);
            write_u8(writer, element_type as u8)?;
            write_length(writer, items.len())?;
            for item in items {
                write_payload(writer, item)?;
            }
        }
        Tag::Compound(entries) => {
            for (name, value) in entries {
                write_u8(writer, value.type_of() as u8)?;
                write_name(writer, name)?;
                write_payload(writer, value)?;
            }
            write_u8(writer, TagType::End as u8)?;
        }
        Tag::IntArray(values) => {
            write_length(writer, values.len())?;
            for value in values {
                write_i32(writer, *value)?;
            }
        }
        Tag::LongArray(values) => {
            write_length(writer, values.len())?;
            for value in values {
                write_i64(writer, *value)?;
            }
        }
    }
    Ok(())
}

fn read_length<R: Read>(reader: &mut R) -> Result<usize, CoreError> {
    let len = read_i32(reader)?;
    if len < 0 {
        return Err(CoreError::InvalidNbt("NBTの要素数が負の値です".to_string()));
    }
    Ok(len as usize)
}

fn write_length<W: Write>(writer: &mut W, len: usize) -> Result<(), CoreError> {
    write_i32(
        writer,
        i32::try_from(len)
            .map_err(|_| CoreError::InvalidNbt("NBTの要素数が多すぎます".to_string()))?,
    )
}

fn read_u8<R: Read>(reader: &mut R) -> Result<u8, CoreError> {
    let mut buf = [0u8; 1];
    read_fully(reader, &mut buf)?;
    Ok(buf[0])
}

fn read_i16<R: Read>(reader: &mut R) -> Result<i16, CoreError> {
    let mut buf = [0u8; 2];
    read_fully(reader, &mut buf)?;
    Ok(i16::from_be_bytes(buf))
}

fn read_i32<R: Read>(reader: &mut R) -> Result<i32, CoreError> {
    let mut buf = [0u8; 4];
    read_fully(reader, &mut buf)?;
    Ok(i32::from_be_bytes(buf))
}

fn read_i64<R: Read>(reader: &mut R) -> Result<i64, CoreError> {
    let mut buf = [0u8; 8];
    read_fully(reader, &mut buf)?;
    Ok(i64::from_be_bytes(buf))
}

fn read_exact<R: Read>(reader: &mut R, len: usize) -> Result<Vec<u8>, CoreError> {
    let mut buf = vec![0u8; len];
    read_fully(reader, &mut buf)?;
    Ok(buf)
}

fn read_fully<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<(), CoreError> {
    reader.read_exact(buf).map_err(|err| {
        if err.kind() == io::ErrorKind::UnexpectedEof {
            CoreError::InvalidNbt("NBTの読み取り中にデータが尽きました".to_string())
        } else {
            CoreError::Io(err)
        }
    })
}

fn write_u8<W: Write>(writer: &mut W, value: u8) -> Result<(), CoreError> {
    writer.write_all(&[value])?;
    Ok(())
}

fn write_i16<W: Write>(writer: &mut W, value: i16) -> Result<(), CoreError> {
    writer.write_all(&value.to_be_bytes())?;
    Ok(())
}

fn write_i32<W: Write>(writer: &mut W, value: i32) -> Result<(), CoreError> {
    writer.write_all(&value.to_be_bytes())?;
    Ok(())
}

fn write_i64<W: Write>(writer: &mut W, value: i64) -> Result<(), CoreError> {
    writer.write_all(&value.to_be_bytes())?;
    Ok(())
}

/// NBTの文字列(タグ名・TAG_Stringの値)を読む。長さは符号なし16bit、内容はJavaの
/// 「修正UTF-8」(通常のUTF-8とはU+0000とサロゲートペアの扱いが異なる)。
fn read_name<R: Read>(reader: &mut R) -> Result<String, CoreError> {
    let len = {
        let mut buf = [0u8; 2];
        read_fully(reader, &mut buf)?;
        u16::from_be_bytes(buf) as usize
    };
    let bytes = read_exact(reader, len)?;
    decode_modified_utf8(&bytes)
}

fn write_name<W: Write>(writer: &mut W, value: &str) -> Result<(), CoreError> {
    let bytes = encode_modified_utf8(value)?;
    let len = u16::try_from(bytes.len()).map_err(|_| {
        CoreError::InvalidNbt("NBTに書き出せる文字列の上限を超えました".to_string())
    })?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&bytes)?;
    Ok(())
}

/// 絵文字等(サロゲートペア)を含むサーバー名を壊さないよう、UTF-16コード単位ごとに
/// Javaの「修正UTF-8」でエンコードする([TrainClientSetup/Nbt.cs](https://github.com/a13x-4793-3654/TRAiN-Setup)
/// の`JavaModifiedUtf8.Encode`と同じ規則)。
fn encode_modified_utf8(value: &str) -> Result<Vec<u8>, CoreError> {
    let mut buffer = Vec::with_capacity(value.len() + 8);
    for unit in value.encode_utf16() {
        if (0x0001..=0x007F).contains(&unit) {
            buffer.push(unit as u8);
        } else if unit <= 0x07FF {
            buffer.push(0xC0 | ((unit >> 6) as u8));
            buffer.push(0x80 | ((unit & 0x3F) as u8));
        } else {
            buffer.push(0xE0 | ((unit >> 12) as u8));
            buffer.push(0x80 | (((unit >> 6) & 0x3F) as u8));
            buffer.push(0x80 | ((unit & 0x3F) as u8));
        }
    }
    Ok(buffer)
}

fn decode_modified_utf8(bytes: &[u8]) -> Result<String, CoreError> {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b & 0x80 == 0 {
            units.push(b as u16);
            i += 1;
        } else if b & 0xE0 == 0xC0 {
            require(i + 1 < bytes.len())?;
            units.push((((b & 0x1F) as u16) << 6) | ((bytes[i + 1] & 0x3F) as u16));
            i += 2;
        } else if b & 0xF0 == 0xE0 {
            require(i + 2 < bytes.len())?;
            units.push(
                (((b & 0x0F) as u16) << 12)
                    | (((bytes[i + 1] & 0x3F) as u16) << 6)
                    | ((bytes[i + 2] & 0x3F) as u16),
            );
            i += 3;
        } else {
            return Err(CoreError::InvalidNbt(
                "NBTの文字列を解釈できませんでした".to_string(),
            ));
        }
    }
    String::from_utf16(&units)
        .map_err(|_| CoreError::InvalidNbt("NBTの文字列がUTF-16として不正です".to_string()))
}

fn require(condition: bool) -> Result<(), CoreError> {
    if condition {
        Ok(())
    } else {
        Err(CoreError::InvalidNbt(
            "NBTの文字列が途中で終わっています".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_simple_compound() {
        let mut root = Compound::new();
        let mut servers = Vec::new();
        let mut entry = Compound::new();
        entry.set("name", Tag::String("碓氷鯖".to_string()));
        entry.set("ip", Tag::String("mc.example.com:25565".to_string()));
        servers.push(entry.into_tag());
        root.set("servers", Tag::List(TagType::Compound, servers));

        let mut buffer = Vec::new();
        write(&mut buffer, &root).unwrap();

        let read_back = read(&mut buffer.as_slice()).unwrap();
        let Some(Tag::List(TagType::Compound, items)) = read_back.get("servers").cloned() else {
            panic!("servers list missing");
        };
        let Tag::Compound(entries) = &items[0] else {
            panic!("not a compound");
        };
        let entry = Compound(entries.clone());
        assert_eq!(entry.get_string("name"), Some("碓氷鯖"));
        assert_eq!(entry.get_string("ip"), Some("mc.example.com:25565"));
    }

    #[test]
    fn round_trips_emoji_server_name() {
        let mut root = Compound::new();
        root.set("value", Tag::String("🎮鯖".to_string()));

        let mut buffer = Vec::new();
        write(&mut buffer, &root).unwrap();

        let read_back = read(&mut buffer.as_slice()).unwrap();
        assert_eq!(read_back.get_string("value"), Some("🎮鯖"));
    }

    #[test]
    fn round_trips_empty_list() {
        let mut root = Compound::new();
        root.set("servers", Tag::List(TagType::End, Vec::new()));

        let mut buffer = Vec::new();
        write(&mut buffer, &root).unwrap();

        let read_back = read(&mut buffer.as_slice()).unwrap();
        assert!(matches!(read_back.get("servers"), Some(Tag::List(_, items)) if items.is_empty()));
    }

    #[test]
    fn preserves_unknown_tags() {
        let mut root = Compound::new();
        let mut entry = Compound::new();
        entry.set("name", Tag::String("test".to_string()));
        entry.set("ip", Tag::String("127.0.0.1".to_string()));
        entry.set("acceptTextures", Tag::Byte(1));
        root.set(
            "servers",
            Tag::List(TagType::Compound, vec![entry.into_tag()]),
        );

        let mut buffer = Vec::new();
        write(&mut buffer, &root).unwrap();
        let read_back = read(&mut buffer.as_slice()).unwrap();

        let Some(Tag::List(_, items)) = read_back.get("servers").cloned() else {
            panic!("servers missing");
        };
        let Tag::Compound(entries) = &items[0] else {
            panic!("not compound");
        };
        assert_eq!(
            Compound(entries.clone()).get("acceptTextures"),
            Some(&Tag::Byte(1))
        );
    }
}
