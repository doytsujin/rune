use crate::collections::HashMap;
use crate::error;
use crate::packages::bytes::Bytes;
use crate::tls;
use crate::value::ValuePtr;
use crate::vm::VmError;
use serde::{de, ser};
use std::fmt;

/// Deserialize implementation for value pointers.
///
/// **Warning:** This only works if a `Vm` is accessible through [tls], like by
/// being set up with [tls::inject_vm] or [tls::InjectVm].
impl<'de> de::Deserialize<'de> for ValuePtr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(VmVisitor)
    }
}

/// Serialize implementation for value pointers.
///
/// **Warning:** This only works if a `Vm` is accessible through [tls], like by
/// being set up with [tls::inject_vm] or [tls::InjectVm].
impl ser::Serialize for ValuePtr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        use serde::ser::SerializeMap as _;
        use serde::ser::SerializeSeq as _;

        match *self {
            ValuePtr::None => serializer.serialize_unit(),
            ValuePtr::Bool(b) => serializer.serialize_bool(b),
            ValuePtr::Char(c) => serializer.serialize_char(c),
            ValuePtr::Integer(integer) => serializer.serialize_i64(integer),
            ValuePtr::Float(float) => serializer.serialize_f64(float),
            ValuePtr::StaticString(slot) => tls::with_vm(|_, unit| {
                let string = unit.lookup_string(slot).map_err(ser::Error::custom)?;
                serializer.serialize_str(string)
            }),
            ValuePtr::String(slot) => tls::with_vm(|vm, _| {
                let string = vm.string_ref(slot).map_err(ser::Error::custom)?;
                serializer.serialize_str(&*string)
            }),
            ValuePtr::Array(slot) => tls::with_vm(|vm, _| {
                let array = vm.array_ref(slot).map_err(ser::Error::custom)?;
                let mut serializer = serializer.serialize_seq(Some(array.len()))?;

                for value in &*array {
                    serializer.serialize_element(value)?;
                }

                serializer.end()
            }),
            ValuePtr::Object(slot) => tls::with_vm(|vm, _| {
                let object = vm.object_ref(slot).map_err(ser::Error::custom)?;
                let mut serializer = serializer.serialize_map(Some(object.len()))?;

                for (key, value) in &*object {
                    serializer.serialize_entry(key, value)?;
                }

                serializer.end()
            }),
            ValuePtr::External(..) => {
                return Err(ser::Error::custom("cannot serialize external objects"));
            }
            ValuePtr::Type(..) => {
                return Err(ser::Error::custom("cannot serialize type objects"));
            }
            ValuePtr::Fn(..) => {
                return Err(ser::Error::custom("cannot serialize fn objects"));
            }
        }
    }
}

impl de::Error for VmError {
    fn custom<T>(msg: T) -> Self
    where
        T: fmt::Display,
    {
        VmError::UserError {
            error: error::Error::msg(msg.to_string()),
        }
    }
}

struct VmVisitor;

impl<'de> de::Visitor<'de> for VmVisitor {
    type Value = ValuePtr;

    fn expecting(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("any valid value")
    }

    #[inline]
    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        tls::with_vm(|vm, _| Ok(vm.string_allocate(value.to_owned())))
    }

    #[inline]
    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        tls::with_vm(|vm, _| Ok(vm.string_allocate(value)))
    }

    #[inline]
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        tls::with_vm(|vm, _| Ok(vm.external_allocate(Bytes::from_bytes(v.to_vec()))))
    }

    #[inline]
    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        tls::with_vm(|vm, _| Ok(vm.external_allocate(Bytes::from_bytes(v))))
    }

    #[inline]
    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::Integer(v as i64))
    }

    #[inline]
    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::Integer(v as i64))
    }

    #[inline]
    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::Integer(v as i64))
    }

    #[inline]
    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::Integer(v))
    }

    #[inline]
    fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::Integer(v as i64))
    }

    #[inline]
    fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::Integer(v as i64))
    }

    #[inline]
    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::Integer(v as i64))
    }

    #[inline]
    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::Integer(v as i64))
    }

    #[inline]
    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::Integer(v as i64))
    }

    #[inline]
    fn visit_u128<E>(self, v: u128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::Integer(v as i64))
    }

    #[inline]
    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::Bool(v))
    }

    #[inline]
    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::None)
    }

    #[inline]
    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(ValuePtr::None)
    }

    #[inline]
    fn visit_seq<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
    where
        V: de::SeqAccess<'de>,
    {
        let mut vec = Vec::new();

        while let Some(elem) = visitor.next_element()? {
            vec.push(elem);
        }

        tls::with_vm(|vm, _| Ok(vm.array_allocate(vec)))
    }

    #[inline]
    fn visit_map<V>(self, mut visitor: V) -> Result<Self::Value, V::Error>
    where
        V: de::MapAccess<'de>,
    {
        let mut object = HashMap::<String, ValuePtr>::new();

        while let Some((key, value)) = visitor.next_entry()? {
            object.insert(key, value);
        }

        tls::with_vm(|vm, _| Ok(vm.object_allocate(object)))
    }
}
