//! Serde helpers that write byte arrays as MessagePack `bin` instead of an array of integers.

use std::fmt;

use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserializer, Serializer};

/// `#[serde(with = "crate::bytes::fixed")]` for `[u8; N]`.
pub(crate) mod fixed {
    use super::*;

    pub(crate) fn serialize<S: Serializer, const N: usize>(
        v: &[u8; N],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(v)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        d: D,
    ) -> Result<[u8; N], D::Error> {
        struct V<const N: usize>;
        impl<'de, const N: usize> Visitor<'de> for V<N> {
            type Value = [u8; N];
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{N} bytes")
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                v.try_into().map_err(|_| E::invalid_length(v.len(), &self))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut out = [0u8; N];
                for (i, b) in out.iter_mut().enumerate() {
                    *b = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                }
                Ok(out)
            }
        }
        d.deserialize_bytes(V::<N>)
    }
}

/// `#[serde(with = "crate::bytes::vec")]` for `Vec<u8>`.
pub(crate) mod vec {
    use super::*;

    pub(crate) fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(v)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                Ok(v)
            }
        }
        d.deserialize_byte_buf(V)
    }
}
