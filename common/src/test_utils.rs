use proptest::arbitrary::Arbitrary;
use proptest::proptest;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::ed25519;

pub fn assert_bcs_roundtrip<T>()
where
    T: Arbitrary + PartialEq + Serialize + DeserializeOwned,
{
    proptest!(|(value: T)| {
        let ser_value = bcs::to_bytes(&value).unwrap();
        let value2 = bcs::from_bytes::<T>(&ser_value).unwrap();
        let ser_value2 = bcs::to_bytes(&value2).unwrap();

        assert_eq!(&value, &value2);
        assert_eq!(&ser_value, &ser_value2);
    });
}

pub fn assert_signed_roundtrip<T>()
where
    T: Arbitrary + PartialEq + Serialize + DeserializeOwned + ed25519::Signable,
{
    proptest!(|(seed: [u8; 32], value: T)| {
        let key_pair = ed25519::KeyPair::from_seed(&seed);
        let pubkey = key_pair.public_key();

        let (ser_value, signed_value) = key_pair.sign_struct(&value).unwrap();
        let signed_value2 = pubkey.verify_self_signed_struct(&ser_value).unwrap();
        let (ser_value2, _) = key_pair.sign_struct(signed_value2.inner()).unwrap();

        assert_eq!(signed_value, signed_value2.as_ref());
        assert_eq!(&ser_value, &ser_value2);
    });
}
