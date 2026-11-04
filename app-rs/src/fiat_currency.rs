//! Fiat currency defaults by device region.

use lexe_common::api::fiat_rates::IsoCurrencyCode;

/// The fiat currency of `region` (ISO 3166-1 alpha-2), if Lexe supports it.
pub fn region_fiat_currency(region: &str) -> Option<IsoCurrencyCode> {
    FIAT_CURRENCY_REGIONS
        .iter()
        .find(|(_code, regions)| {
            regions.iter().any(|r| r.eq_ignore_ascii_case(region))
        })
        .map(|(code, _regions)| *code)
}

/// The regions (ISO 3166-1 alpha-2) where each supported currency is legal
/// tender, in the order of [`SUPPORTED_FIAT_CURRENCIES`].
///
/// Generated from CLDR by `gen_fiat_currency_regions` in the tests below.
///
/// [`SUPPORTED_FIAT_CURRENCIES`]: lexe_common::api::fiat_rates::SUPPORTED_FIAT_CURRENCIES
#[rustfmt::skip]
const FIAT_CURRENCY_REGIONS: &[(IsoCurrencyCode, &[&str])] = &[
    (IsoCurrencyCode::AED, &["AE"]),
    (IsoCurrencyCode::ARS, &["AR"]),
    (IsoCurrencyCode::AUD, &["AU", "CC", "CX", "HM", "KI", "NF", "NR", "TV"]),
    (IsoCurrencyCode::BRL, &["BR"]),
    (IsoCurrencyCode::CAD, &["CA"]),
    (IsoCurrencyCode::CHF, &["CH", "LI"]),
    (IsoCurrencyCode::CLP, &["CL"]),
    (IsoCurrencyCode::CNY, &["CN"]),
    (IsoCurrencyCode::COP, &["CO"]),
    (IsoCurrencyCode::CZK, &["CZ"]),
    (IsoCurrencyCode::DKK, &["DK", "FO", "GL"]),
    (IsoCurrencyCode::EGP, &["EG"]),
    (IsoCurrencyCode::EUR, &[
        "AD", "AT", "AX", "BE", "BG", "BL", "CY", "DE", "EA", "EE", "ES", "EU",
        "FI", "FR", "GF", "GP", "GR", "HR", "IC", "IE", "IT", "LT", "LU", "LV",
        "MC", "ME", "MF", "MQ", "MT", "NL", "PM", "PT", "RE", "SI", "SK", "SM",
        "TF", "VA", "XK", "YT",
    ]),
    (IsoCurrencyCode::GBP, &["GB", "GG", "GS", "IM", "JE", "TA"]),
    (IsoCurrencyCode::HKD, &["HK"]),
    (IsoCurrencyCode::HUF, &["HU"]),
    (IsoCurrencyCode::IDR, &["ID"]),
    (IsoCurrencyCode::ILS, &["IL", "PS"]),
    (IsoCurrencyCode::INR, &["BT", "IN"]),
    (IsoCurrencyCode::IQD, &["IQ"]),
    (IsoCurrencyCode::JPY, &["JP"]),
    (IsoCurrencyCode::KRW, &["KR"]),
    (IsoCurrencyCode::KWD, &["KW"]),
    (IsoCurrencyCode::KZT, &["KZ"]),
    (IsoCurrencyCode::LBP, &["LB"]),
    (IsoCurrencyCode::MAD, &["EH", "MA"]),
    (IsoCurrencyCode::MOP, &["MO"]),
    (IsoCurrencyCode::MXN, &["MX"]),
    (IsoCurrencyCode::MYR, &["MY"]),
    (IsoCurrencyCode::NGN, &["NG"]),
    (IsoCurrencyCode::NOK, &["BV", "NO", "SJ"]),
    (IsoCurrencyCode::NZD, &["CK", "NU", "NZ", "PN", "TK"]),
    (IsoCurrencyCode::PEN, &["PE"]),
    (IsoCurrencyCode::PHP, &["PH"]),
    (IsoCurrencyCode::PKR, &["PK"]),
    (IsoCurrencyCode::PLN, &["PL"]),
    (IsoCurrencyCode::QAR, &["QA"]),
    (IsoCurrencyCode::RON, &["RO"]),
    (IsoCurrencyCode::RUB, &["RU"]),
    (IsoCurrencyCode::SAR, &["SA"]),
    (IsoCurrencyCode::SEK, &["SE"]),
    (IsoCurrencyCode::SGD, &["SG"]),
    (IsoCurrencyCode::THB, &["TH"]),
    (IsoCurrencyCode::TRY, &["TR"]),
    (IsoCurrencyCode::TWD, &["TW"]),
    (IsoCurrencyCode::UAH, &["UA"]),
    (IsoCurrencyCode::USD, &[
        "AS", "BQ", "DG", "EC", "FM", "GU", "HT", "IO", "MH", "MP", "PA", "PR",
        "PW", "SV", "TC", "TL", "UM", "US", "VG", "VI", "ZW",
    ]),
    (IsoCurrencyCode::VND, &["VN"]),
    (IsoCurrencyCode::ZAR, &["LS", "NA", "ZA"]),
];

#[cfg(test)]
mod test {
    use std::{
        collections::{BTreeMap, HashSet},
        str::FromStr,
    };

    use lexe_common::api::fiat_rates::SUPPORTED_FIAT_CURRENCIES;

    use super::*;

    // Rows mirror `SUPPORTED_FIAT_CURRENCIES`, in order, and no region appears
    // twice, so each region has one currency.
    #[test]
    fn test_fiat_currency_regions() {
        let codes = FIAT_CURRENCY_REGIONS.iter().map(|(code, _regions)| code);
        let supported =
            SUPPORTED_FIAT_CURRENCIES.iter().map(|(code, _name)| code);
        assert!(codes.eq(supported));

        let mut seen = HashSet::new();
        for (_code, regions) in FIAT_CURRENCY_REGIONS {
            for region in *regions {
                assert!(
                    seen.insert(region),
                    "{region} is under two currencies"
                );
            }
        }
    }

    #[test]
    fn test_region_fiat_currency() {
        assert_eq!(region_fiat_currency("US"), Some(IsoCurrencyCode::USD));
        assert_eq!(region_fiat_currency("fr"), Some(IsoCurrencyCode::EUR));
        assert_eq!(region_fiat_currency("AQ"), None);
        assert_eq!(region_fiat_currency(""), None);
    }

    // --- Utilities --- //

    #[test]
    fn test_regions_from_cldr() {
        let json = serde_json::json!({
            "supplemental": { "currencyData": { "region": {
                // Current legal tender.
                "US": [{"USD": {"_from": "1792-01-01"}}],
                // A retired currency gives way to its successor.
                "XA": [
                    {"USD": {"_from": "1900-01-01", "_to": "2000-01-01"}},
                    {"EUR": {"_from": "2000-01-01"}},
                ],
                // Not tender, even without an end date.
                "XB": [
                    {"USD": {"_tender": "false"}},
                    {"EUR": {"_from": "2000-01-01"}},
                ],
                // The first *supported* current currency wins.
                "HT": [
                    {"HTG": {"_from": "1872-08-26"}},
                    {"USD": {"_from": "1915-01-01"}},
                ],
                // No supported currency at all.
                "AL": [{"ALL": {"_from": "1946-01-01"}}],
            }}}
        });

        let rows = regions_from_cldr(&json);
        assert_eq!(rows[&IsoCurrencyCode::USD], ["HT", "US"]);
        assert_eq!(rows[&IsoCurrencyCode::EUR], ["XA", "XB"]);
        assert!(rows.values().flatten().all(|region| *region != "AL"));
    }

    /// Regenerates `FIAT_CURRENCY_REGIONS` from CLDR, prints it as Rust
    /// source, and checks that the current table matches. If it doesn't,
    /// paste the printed rows over the current ones.
    ///
    /// ```bash
    /// $ cargo test -p app-rs -- gen_fiat_currency_regions --ignored --nocapture
    /// ```
    #[ignore]
    #[tokio::test]
    async fn gen_fiat_currency_regions() {
        // A cldr-json release tag. To refresh, pick the latest `N.0.0` tag
        // from <https://github.com/unicode-org/cldr-json/tags> and rerun this
        // test.
        const URL: &str = concat!(
            "https://raw.githubusercontent.com/unicode-org/cldr-json/49.0.0",
            "/cldr-json/cldr-core/supplemental/currencyData.json",
        );

        let client = reqwest::Client::builder()
            .use_preconfigured_tls(lexe_tls::external_client_config())
            .build()
            .unwrap();
        let json = client
            .get(URL)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap();
        let rows = regions_from_cldr(&json);

        // Render the rows in the table's format.
        let mut source = String::new();
        for (code, regions) in &rows {
            let quoted = regions
                .iter()
                .map(|region| format!("\"{region}\""))
                .collect::<Vec<_>>();
            let one_line = format!(
                "    (IsoCurrencyCode::{code}, &[{}]),",
                quoted.join(", ")
            );
            if one_line.len() <= 80 {
                source.push_str(&one_line);
                source.push('\n');
            } else {
                source.push_str(&format!("    (IsoCurrencyCode::{code}, &[\n"));
                for chunk in quoted.chunks(12) {
                    source
                        .push_str(&format!("        {},\n", chunk.join(", ")));
                }
                source.push_str("    ]),\n");
            }
        }
        println!("{source}");

        let expected = rows.into_iter().collect::<Vec<_>>();
        let actual = FIAT_CURRENCY_REGIONS
            .iter()
            .map(|(code, regions)| (*code, regions.to_vec()))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "FIAT_CURRENCY_REGIONS is stale");
    }

    /// The regions where each supported currency is current legal tender,
    /// per CLDR's `currencyData.json`.
    fn regions_from_cldr(
        json: &serde_json::Value,
    ) -> BTreeMap<IsoCurrencyCode, Vec<&str>> {
        let regions = json["supplemental"]["currencyData"]["region"]
            .as_object()
            .unwrap();

        // Each region lists its currencies in order of preference. Keep the
        // ones that are current legal tender, then take the first of those
        // that is supported.
        let mut rows = SUPPORTED_FIAT_CURRENCIES
            .iter()
            .map(|(code, _name)| (*code, Vec::new()))
            .collect::<BTreeMap<IsoCurrencyCode, Vec<&str>>>();
        for (region, currencies) in regions {
            let mut current =
                currencies.as_array().unwrap().iter().filter_map(|entry| {
                    // Each entry is a one-key object: `{"EUR": {"_from": ..}}`.
                    let (code, dates) =
                        entry.as_object().unwrap().iter().next().unwrap();
                    let is_current = dates.get("_to").is_none()
                        && dates.get("_tender").and_then(|t| t.as_str())
                            != Some("false");
                    is_current
                        .then(|| IsoCurrencyCode::from_str(code).ok())
                        .flatten()
                });
            if let Some(code) = current.find(|code| rows.contains_key(code)) {
                rows.get_mut(&code).unwrap().push(region);
            }
        }
        rows
    }
}
