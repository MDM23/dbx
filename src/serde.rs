use core::slice;
use std::{
    fmt::{self, Display},
    net::IpAddr,
};

use serde::{
    de::{MapAccess, Visitor},
    Deserializer,
};
use time::OffsetDateTime;
use tokio_postgres::{
    types::{FromSql, Type},
    Column, Row,
};

#[derive(Debug)]
pub enum Error {
    Unknown,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("unknown")
    }
}

impl serde::de::Error for Error {
    fn custom<T: Display>(msg: T) -> Self {
        println!("{msg}");
        Error::Unknown
    }
}

impl std::error::Error for Error {}

pub struct PgRow<'a> {
    columns: slice::Iter<'a, Column>,
    values: slice::Iter<'a, Column>,
    row: &'a Row,
}

impl<'a> From<&'a Row> for PgRow<'a> {
    fn from(row: &'a Row) -> Self {
        Self {
            columns: row.columns().iter(),
            values: row.columns().iter(),
            row,
        }
    }
}

impl<'a, 'de> MapAccess<'de> for PgRow<'a> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: serde::de::DeserializeSeed<'de>,
    {
        self.columns
            .next()
            .map(|col| seed.deserialize(FieldName(col.name())))
            .transpose()
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::DeserializeSeed<'de>,
    {
        let key = self.values.next();

        seed.deserialize(PgOptCol(self.row.try_get(key.unwrap().name()).unwrap()))
    }
}

#[derive(Debug)]
pub struct FieldName<'a>(&'a str);

impl<'a> FieldName<'a> {
    pub fn new(name: &'a str) -> Self {
        Self(name)
    }
}

impl<'a, 'de> Deserializer<'de> for FieldName<'a> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_str(self.0)
    }

    ::serde::forward_to_deserialize_any! {
        bool u8 u16 u32 u64 i8 i16 i32 i64 f32 f64 char str string unit option
        seq bytes byte_buf map unit_struct newtype_struct
        tuple_struct struct tuple enum identifier ignored_any
    }
}

pub struct PgOptCol<'a>(Option<PgCol<'a>>);

impl<'a, 'de> Deserializer<'de> for PgOptCol<'a> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.0 {
            Some(col) => col.deserialize_any(visitor),
            None => visitor.visit_none(),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.0 {
            Some(col) => visitor.visit_some(col),
            None => visitor.visit_none(),
        }
    }

    ::serde::forward_to_deserialize_any! {
        bool u8 u16 u32 u64 i8 i16 i32 i64 f32 f64 char str string unit
        seq bytes byte_buf map unit_struct newtype_struct
        tuple_struct struct tuple enum identifier ignored_any
    }
}

pub struct PgCol<'a> {
    ty: tokio_postgres::types::Type,
    raw: &'a [u8],
}

impl<'a> FromSql<'a> for PgCol<'a> {
    fn from_sql(
        ty: &tokio_postgres::types::Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self {
            ty: ty.to_owned(),
            raw,
        })
    }

    fn accepts(ty: &tokio_postgres::types::Type) -> bool {
        true
    }
}

use time::format_description::{modifier, BorrowedFormatItem, Component};

const DATE_FORMAT: &[BorrowedFormatItem<'_>] = &[
    BorrowedFormatItem::Component(Component::Year(modifier::Year::default())),
    BorrowedFormatItem::Literal(b"-"),
    BorrowedFormatItem::Component(Component::Month(modifier::Month::default())),
    BorrowedFormatItem::Literal(b"-"),
    BorrowedFormatItem::Component(Component::Day(modifier::Day::default())),
];

const TIME_FORMAT: &[BorrowedFormatItem<'_>] = &[
    BorrowedFormatItem::Component(Component::Hour(modifier::Hour::default())),
    BorrowedFormatItem::Literal(b":"),
    BorrowedFormatItem::Component(Component::Minute(modifier::Minute::default())),
    BorrowedFormatItem::Literal(b":"),
    BorrowedFormatItem::Component(Component::Second(modifier::Second::default())),
    BorrowedFormatItem::Literal(b"."),
    BorrowedFormatItem::Component(Component::Subsecond(modifier::Subsecond::default())),
];

const UTC_OFFSET_HOUR: modifier::OffsetHour = {
    let mut m = modifier::OffsetHour::default();
    m.sign_is_mandatory = true;
    m
};

const UTC_OFFSET_MINUTE: modifier::OffsetMinute = modifier::OffsetMinute::default();
const UTC_OFFSET_SECOND: modifier::OffsetSecond = modifier::OffsetSecond::default();

const UTC_OFFSET_FORMAT: &[BorrowedFormatItem<'_>] = &[
    BorrowedFormatItem::Component(Component::OffsetHour(UTC_OFFSET_HOUR)),
    BorrowedFormatItem::Optional(&BorrowedFormatItem::Compound(&[
        BorrowedFormatItem::Literal(b":"),
        BorrowedFormatItem::Component(Component::OffsetMinute(UTC_OFFSET_MINUTE)),
        BorrowedFormatItem::Optional(&BorrowedFormatItem::Compound(&[
            BorrowedFormatItem::Literal(b":"),
            BorrowedFormatItem::Component(Component::OffsetSecond(UTC_OFFSET_SECOND)),
        ])),
    ])),
];

const OFFSET_DATE_TIME_FORMAT: &[BorrowedFormatItem<'_>] = &[
    BorrowedFormatItem::Compound(DATE_FORMAT),
    BorrowedFormatItem::Literal(b" "),
    BorrowedFormatItem::Compound(TIME_FORMAT),
    BorrowedFormatItem::Literal(b" "),
    BorrowedFormatItem::Compound(UTC_OFFSET_FORMAT),
];

impl<'a, 'de> Deserializer<'de> for PgCol<'a> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.ty {
            Type::BOOL => visitor.visit_bool(FromSql::from_sql(&self.ty, &self.raw).unwrap()),
            Type::FLOAT4 => visitor.visit_f32(FromSql::from_sql(&self.ty, &self.raw).unwrap()),
            Type::FLOAT8 => visitor.visit_f64(FromSql::from_sql(&self.ty, &self.raw).unwrap()),
            Type::INT2 => visitor.visit_i16(FromSql::from_sql(&self.ty, &self.raw).unwrap()),
            Type::INT4 => visitor.visit_i32(FromSql::from_sql(&self.ty, &self.raw).unwrap()),
            Type::INT8 => visitor.visit_i64(FromSql::from_sql(&self.ty, &self.raw).unwrap()),
            Type::TEXT | Type::VARCHAR | Type::BPCHAR => {
                visitor.visit_string(FromSql::from_sql(&self.ty, &self.raw).unwrap())
            }

            Type::INET => {
                visitor.visit_string(IpAddr::from_sql(&self.ty, &self.raw).unwrap().to_string())
            }

            #[cfg(feature = "serde-json")]
            Type::JSONB | Type::JSON => serde_json::Value::from_sql(&self.ty, &self.raw)
                .unwrap()
                .deserialize_any(visitor)
                .map_err(|_| Error::Unknown),

            #[cfg(feature = "uuid")]
            Type::UUID => visitor.visit_bytes(FromSql::from_sql(&self.ty, &self.raw).unwrap()),

            #[cfg(feature = "time")]
            Type::TIMESTAMPTZ => visitor.visit_string(
                OffsetDateTime::from_sql(&self.ty, &self.raw)
                    .unwrap()
                    .format(OFFSET_DATE_TIME_FORMAT)
                    .unwrap(),
            ),

            ty => unimplemented!("{:?}", ty),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if let Some(col) = <Option<Self> as FromSql>::from_sql(&self.ty, &self.raw).unwrap() {
            visitor.visit_some(col)
        } else {
            visitor.visit_none()
        }
    }

    ::serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

impl<'a, 'de> Deserializer<'de> for PgRow<'a> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        todo!()
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(self)
    }

    fn deserialize_struct<V>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    ::serde::forward_to_deserialize_any! {
        bool u8 u16 u32 u64 i8 i16 i32 i64 f32 f64 char str string unit option
        seq bytes byte_buf unit_struct newtype_struct
        tuple_struct tuple enum identifier ignored_any
    }
}
