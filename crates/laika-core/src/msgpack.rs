//! Minimal MessagePack decoder into `serde_json::Value`.
//!
//! Lightroom's cloud library stores every document as MessagePack. Laika
//! only reads it, so a small decoder is enough: binary payloads become hex
//! strings and extension types become null.

use serde_json::{Map, Number, Value};

/// Decode one MessagePack value (trailing bytes are ignored).
pub fn decode(bytes: &[u8]) -> Result<Value, String> {
    let mut r = Reader { b: bytes, i: 0 };
    r.value(0)
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

const MAX_DEPTH: usize = 64;

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self
            .i
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or("msgpack: truncated")?;
        let s = &self.b[self.i..end];
        self.i = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn be(&mut self, n: usize) -> Result<u64, String> {
        Ok(self
            .take(n)?
            .iter()
            .fold(0u64, |acc, &b| (acc << 8) | b as u64))
    }

    fn str(&mut self, n: usize) -> Result<Value, String> {
        let s = self.take(n)?;
        Ok(Value::String(String::from_utf8_lossy(s).into_owned()))
    }

    fn bin(&mut self, n: usize) -> Result<Value, String> {
        let s = self.take(n)?;
        Ok(Value::String(
            s.iter().map(|b| format!("{b:02x}")).collect(),
        ))
    }

    fn array(&mut self, n: usize, depth: usize) -> Result<Value, String> {
        let mut v = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            v.push(self.value(depth + 1)?);
        }
        Ok(Value::Array(v))
    }

    fn map(&mut self, n: usize, depth: usize) -> Result<Value, String> {
        let mut m = Map::new();
        for _ in 0..n {
            let k = match self.value(depth + 1)? {
                Value::String(s) => s,
                other => other.to_string(),
            };
            let v = self.value(depth + 1)?;
            m.insert(k, v);
        }
        Ok(Value::Object(m))
    }

    fn ext(&mut self, n: usize) -> Result<Value, String> {
        self.take(1 + n)?;
        Ok(Value::Null)
    }

    fn float(f: f64) -> Value {
        Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return Err("msgpack: nested too deeply".to_string());
        }
        let t = self.u8()?;
        Ok(match t {
            0x00..=0x7f => Value::from(t as u64),
            0x80..=0x8f => self.map((t & 0x0f) as usize, depth)?,
            0x90..=0x9f => self.array((t & 0x0f) as usize, depth)?,
            0xa0..=0xbf => self.str((t & 0x1f) as usize)?,
            0xc0 => Value::Null,
            0xc2 => Value::Bool(false),
            0xc3 => Value::Bool(true),
            0xc4 => {
                let n = self.be(1)? as usize;
                self.bin(n)?
            }
            0xc5 => {
                let n = self.be(2)? as usize;
                self.bin(n)?
            }
            0xc6 => {
                let n = self.be(4)? as usize;
                self.bin(n)?
            }
            0xc7 => {
                let n = self.be(1)? as usize;
                self.ext(n)?
            }
            0xc8 => {
                let n = self.be(2)? as usize;
                self.ext(n)?
            }
            0xc9 => {
                let n = self.be(4)? as usize;
                self.ext(n)?
            }
            0xca => Self::float(f32::from_bits(self.be(4)? as u32) as f64),
            0xcb => Self::float(f64::from_bits(self.be(8)?)),
            0xcc => Value::from(self.be(1)?),
            0xcd => Value::from(self.be(2)?),
            0xce => Value::from(self.be(4)?),
            0xcf => Value::from(self.be(8)?),
            0xd0 => Value::from(self.be(1)? as u8 as i8 as i64),
            0xd1 => Value::from(self.be(2)? as u16 as i16 as i64),
            0xd2 => Value::from(self.be(4)? as u32 as i32 as i64),
            0xd3 => Value::from(self.be(8)? as i64),
            0xd4 => self.ext(1)?,
            0xd5 => self.ext(2)?,
            0xd6 => self.ext(4)?,
            0xd7 => self.ext(8)?,
            0xd8 => self.ext(16)?,
            0xd9 => {
                let n = self.be(1)? as usize;
                self.str(n)?
            }
            0xda => {
                let n = self.be(2)? as usize;
                self.str(n)?
            }
            0xdb => {
                let n = self.be(4)? as usize;
                self.str(n)?
            }
            0xdc => {
                let n = self.be(2)? as usize;
                self.array(n, depth)?
            }
            0xdd => {
                let n = self.be(4)? as usize;
                self.array(n, depth)?
            }
            0xde => {
                let n = self.be(2)? as usize;
                self.map(n, depth)?
            }
            0xdf => {
                let n = self.be(4)? as usize;
                self.map(n, depth)?
            }
            0xe0..=0xff => Value::from(t as i8 as i64),
            0xc1 => return Err("msgpack: reserved byte".to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::decode;
    use serde_json::json;

    #[test]
    fn decodes_documents_like_lightroom_writes() {
        // {"type": "asset", "n": [1, -2, 300, 1.5], "ok": true, "b": bin(0xab)}
        let mut b = vec![0x84];
        b.extend([
            0xa4, b't', b'y', b'p', b'e', 0xa5, b'a', b's', b's', b'e', b't',
        ]);
        b.extend([0xa1, b'n', 0x94, 0x01, 0xfe, 0xcd, 0x01, 0x2c, 0xcb]);
        b.extend(1.5f64.to_bits().to_be_bytes());
        b.extend([0xa2, b'o', b'k', 0xc3]);
        b.extend([0xa1, b'b', 0xc4, 0x01, 0xab]);
        assert_eq!(
            decode(&b).unwrap(),
            json!({"type": "asset", "n": [1, -2, 300, 1.5], "ok": true, "b": "ab"})
        );
    }

    #[test]
    fn truncated_input_is_an_error_not_a_panic() {
        assert!(decode(&[0x85, 0xa4, b't']).is_err());
        assert!(decode(&[0xdb, 0xff, 0xff, 0xff, 0xff]).is_err());
        assert!(decode(&[]).is_err());
    }
}
