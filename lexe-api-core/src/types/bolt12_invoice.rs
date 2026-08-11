use lexe_serde::base64_or_bytes;
use lexe_std::const_assert_mem_size;
use lightning::{
    offers::invoice::Bolt12Invoice as LdkBolt12Invoice, util::ser::Writeable,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// A BOLT12 invoice, sent by the payee in response to an invoice request for
/// an [`Offer`].
///
/// Serialized as base64; LDK exposes only raw [`Writeable`] bytes.
///
/// [`Offer`]: super::offer::Offer
#[derive(Debug, Eq, PartialEq)]
pub struct Bolt12Invoice(pub LdkBolt12Invoice);

const_assert_mem_size!(Bolt12Invoice, 1616);

impl From<LdkBolt12Invoice> for Bolt12Invoice {
    #[inline]
    fn from(value: LdkBolt12Invoice) -> Self {
        Self(value)
    }
}

impl Serialize for Bolt12Invoice {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        base64_or_bytes::serialize(self.0.encode(), serializer)
    }
}

impl<'de> Deserialize<'de> for Bolt12Invoice {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        let bytes = base64_or_bytes::deserialize::<D, Vec<u8>>(deserializer)?;
        LdkBolt12Invoice::try_from(bytes)
            .map(Self)
            // `Bolt12ParseError` doesn't impl `Display`.
            .map_err(|e| de::Error::custom(format!("Invalid BOLT12: {e:?}")))
    }
}
