//! Helpers for UI input forms.

use std::str::FromStr;

pub(crate) fn validate_bitcoin_address(
    address_str: &str,
    config_network: common::cli::Network,
) -> Result<(), String> {
    // Ensure the address is well-formed, regardless of network (mainnet,
    // testnet, regtest)
    let address = bitcoin::Address::from_str(address_str)
        // TODO(phlip9): Most of these error messages are not appropriate to
        // show to users. They also need to be translated. We'll need our own
        // enum here that we send back to flutter for display.
        .map_err(|err| err.to_string())?;

    // Ensure the address matches the current build's configured network
    if !address.is_valid_for_network(config_network.to_inner()) {
        let address_network = address.network;
        return Err(format!(
            "This is a {address_network} address, which isn't valid for \
             {config_network}"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use common::cli::{
        MAINNET_NETWORK as MAINNET, REGTEST_NETWORK as REGTEST,
        TESTNET_NETWORK as TESTNET,
    };

    use super::*;

    // A quick sanity check
    #[test]
    fn test_validate_bitcoin_address() {
        let valid = [
            ("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", MAINNET),
            ("BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4", MAINNET),
            ("1QJVDzdqb1VpbDK7uDeyVXy9mR27CJiyhY", MAINNET),
            ("33iFwdLuRpW1uK1RTRqsoi8rR4NpDzk66k", MAINNET),
            ("bc1zw508d6qejxtdg4y5r3zarvaryvaxxpcs", MAINNET),
            ("tb1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3q0sl5k7", TESTNET),
            ("bcrt1q2nfxmhd4n3c8834pj72xagvyr9gl57n5r94fsl", REGTEST),
        ];

        let invalid = [
            ("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", TESTNET),
            ("bc1qw508d6qejxtdg4y5r3zarvary0c5xw", MAINNET),
            ("BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4", REGTEST),
            ("BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T46969", MAINNET),
            ("1QJVDzdqb1VpbDK7uDeyVXy9mR27CJiyhY", REGTEST),
            ("33iFwdLuRpW1uK1RTRqsoi8rR4NpDzk66k", TESTNET),
            ("tb1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3q0sl5k7", MAINNET),
        ];

        for (addr_str, network) in valid {
            validate_bitcoin_address(addr_str, network).unwrap();
        }
        for (addr_str, network) in invalid {
            validate_bitcoin_address(addr_str, network).unwrap_err();
        }
    }
}
