use rcgen::{DistinguishedName, DnType};

pub fn lexe_distinguished_name_prefix() -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CountryName, "US");
    name.push(DnType::StateOrProvinceName, "CA");
    name.push(DnType::OrganizationName, "lexe-tech");
    name
}
