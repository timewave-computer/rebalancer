use std::fmt::{self, Display};

use cosmwasm_std::{from_json, StdError, StdResult};
use cw_storage_plus::{KeyDeserialize, PrimaryKey};
use serde::{
    de,
    ser::{self, SerializeSeq},
    Deserialize, Deserializer, Serialize,
};

use crate::error::AuctionError;

#[derive(
    ::std::clone::Clone,
    ::std::fmt::Debug,
    ::std::cmp::PartialEq,
    ::cosmwasm_schema::schemars::JsonSchema,
)]
#[schemars(crate = "::cosmwasm_schema::schemars")]
pub struct Pair(pub String, pub String);

impl Display for Pair {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}/{}", self.0, self.1)
    }
}

impl<'a> PrimaryKey<'a> for Pair {
    type Prefix = <String as PrimaryKey<'a>>::Prefix;
    type SubPrefix = <String as PrimaryKey<'a>>::SubPrefix;
    type Suffix = <String as PrimaryKey<'a>>::Suffix;
    type SuperSuffix = <String as PrimaryKey<'a>>::SuperSuffix;

    fn key(&self) -> Vec<cw_storage_plus::Key> {
        let mut a = self.0.key();
        a.extend(self.1.key());
        a
    }
}

impl Pair {
    pub fn verify(&self) -> Result<(), AuctionError> {
        if self.0.is_empty() || self.1.is_empty() || self.0 == self.1 {
            return Err(AuctionError::InvalidPair);
        }
        Ok(())
    }
}

impl From<(String, String)> for Pair {
    fn from(pair: (String, String)) -> Self {
        Pair(pair.0, pair.1)
    }
}

impl From<Pair> for (String, String) {
    fn from(pair: Pair) -> Self {
        (pair.0, pair.1)
    }
}

impl From<Vec<u8>> for Pair {
    fn from(value: Vec<u8>) -> Self {
        from_json(value).expect("couldn't parse Pair from Vec<u8>")
    }
}

/// Serializes as a decimal string
impl Serialize for Pair {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(2))?;
        seq.serialize_element(&self.0)?;
        seq.serialize_element(&self.1)?;
        seq.end()
    }
}

/// Deserializes as a base64 string
impl<'de> Deserialize<'de> for Pair {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(PairVisitor)
    }
}

struct PairVisitor;

impl<'de> de::Visitor<'de> for PairVisitor {
    type Value = Pair;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("array-encoded pair")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let first: String = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let second: String = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        Ok(Pair(first, second))
    }
}

fn parse_length(value: &[u8]) -> StdResult<usize> {
    Ok(u16::from_be_bytes(
        value
            .try_into()
            .map_err(|_| StdError::generic_err("Could not read 2 byte length"))?,
    )
    .into())
}

impl KeyDeserialize for Pair {
    type Output = Pair;

    fn from_vec(mut value: Vec<u8>) -> cosmwasm_std::StdResult<Self::Output> {
        let mut tu = value.split_off(2);
        let t_len = parse_length(&value)?;
        let u = tu.split_off(t_len);

        Ok((String::from_vec(tu)?, String::from_vec(u)?).into())
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_from() {
        let pair = super::Pair::from(("uatom".to_string(), "untrn".to_string()));
        assert_eq!(pair.0, "uatom");
        assert_eq!(pair.1, "untrn");
    }
}
