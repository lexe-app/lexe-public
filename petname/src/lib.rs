/// `ADJECTIVES` and `NOUNS` word lists for petname generation.
mod words;

/// Converts the [`[u8; 32]`] from `UserPk::as_array` to a petname, e.g.
///
/// "ab4eebdce1b68f70085e60df6fa53263029468c26541579fb62fc1a7014a94d2"
/// -> "deluxe-ram"
///
/// Interprets the first 8 bytes as two little-endian u32s to index into
/// the adjective and noun word lists.
///
/// The generated petnames are of the form `<adjective>-<noun>` and have a
/// cardinality of 1198 x 1056 = 1,265,088 possible combinations.
pub fn userpk_to_petname(user_pk: &[u8; 32]) -> String {
    // u32 is sufficiently large to cover the word lists and have a distribution
    // which is close to uniform when reduced modulo the list length.
    let adjective_idx =
        u32::from_le_bytes([user_pk[0], user_pk[1], user_pk[2], user_pk[3]]);
    let noun_idx =
        u32::from_le_bytes([user_pk[4], user_pk[5], user_pk[6], user_pk[7]]);

    let adjective =
        words::ADJECTIVES[adjective_idx as usize % words::ADJECTIVES.len()];
    let noun = words::NOUNS[noun_idx as usize % words::NOUNS.len()];

    format!("{adjective}-{noun}")
}

#[cfg(test)]
mod test {
    use byte_array::ByteArray;
    use common::{
        api::user::UserPk,
        rng::{RngExt, SysRng},
    };

    use super::*;

    /// Generates random test vectors - run manually to create new snapshots.
    #[test]
    #[ignore]
    fn generate_test_vectors() {
        let mut rng = SysRng::new();
        for _ in 0..20 {
            let bytes: [u8; 32] = rng.gen_bytes();
            let user_pk = UserPk::from_array(bytes);
            let petname = userpk_to_petname(&bytes);
            println!("(\"{user_pk}\", \"{petname}\"),");
        }
    }

    /// Ensures that we haven't changed our scheme.
    #[test]
    fn petname_snapshot() {
        #[rustfmt::skip]
        let test_vectors: [(&str, &str); 20] = [
            ("ab4eebdce1b68f70085e60df6fa53263029468c26541579fb62fc1a7014a94d2", "deluxe-ram"),
            ("0e19b73607ba6d715e775a4ff99711ef2932ae964a9c09835d154d14e6ea8eb6", "full-trumpetfish"),
            ("b9c6f2b0ed933e3b13ff993b3d8ebd921cb9fd13b6fe81d6411d994b3d72f593", "diligent-alien"),
            ("b5e9fae1c54e4a3a166d676318858a6bd0dd4210a943a5cab74c4ddeef859361", "prompt-koi"),
            ("f901785330b643bffa3d6cf896c7408d0be6a881117ab7305feb7651607e93e1", "expressive-panda"),
            ("6f719c775f95f03cc4ee49114eb53c6af1044ff299b2e6059aa3acf4eb073238", "droll-polecat"),
            ("46ebebbe9cdb20856d13353cbb513525a966f7ef89e0750803e64fbb0d887f4d", "available-quetzal"),
            ("f8f5ec348ecbd1a5421e6d312319e8b1023e5137fe9227e62b5ea94a5e9ca8d4", "deliberate-alligator"),
            ("f45eb561a84e32a894e56375f3184032ffbd0b1dcf4256ed3ccb505f46228308", "changeable-pig"),
            ("6edd622b0b79c17f0e918abad53e9bdaaba4878d24e2ddaefd12aa9ec78d8682", "moral-labrador"),
            ("2cc9715a9334c06c631106c820e9282d5a99161789461a3ec1030f3f5ee01711", "humane-anaconda"),
            ("ab8c767cef6cc0d5d0bfa3a8d5d9e18c56522ddab363a9d742d837281dcb1250", "auspicious-shad"),
            ("e90008e0092153f62b8a3d402c06fecdeb239565f3b25e376860e064f6cdf9d4", "brainy-seahorse"),
            ("ac27def1d5e0c8fb515c0cc267fc832e21717e186415f490ab8d4910eba860b2", "concise-velvetbreast"),
            ("14cf3177fc6476c58dd5d2aa3e5fc56e32de82ff98ff0272672396f331abd7eb", "temperate-bustard"),
            ("a98f2d19acec7a49f5654adcf2ec4c59a9281521f652d530aeb102239b1aa98c", "discerning-haddock"),
            ("49009cd9d89ee0b7d7f9cdfdac5bdf5812148ff82ed35886f0d8db9f8af86684", "right-grosbeak"),
            ("933b81cb8d5fa98819b169dce55affb127a5ac4f9e72065943b7f2bdcc025715", "immortal-ladybeetle"),
            ("672ef792767096bc647169b1f2f2a142df9e8b4cedcf9fc4236cdf180803902d", "noted-crocodile"),
            ("53d454dd64053cc1f4769705c15ae0696e71e8b06bd308f886fe58fa3d585292", "rightful-guinea"),
        ];

        for (hex_input, expected_petname) in test_vectors {
            let user_pk: UserPk = hex_input.parse().unwrap();
            assert_eq!(userpk_to_petname(user_pk.as_array()), expected_petname);
        }
    }
}
