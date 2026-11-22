//! Uppercase codes the NER pass never reports as names.
//!
//! News-trained NER models tag country, currency, and language codes and SQL
//! keywords as organizations or locations with high confidence. Each list is
//! sorted for binary search; the `sorted` test keeps it so.

/// ISO 3166-1 alpha-2 and alpha-3 country codes.
const COUNTRIES: &[&str] = &[
    "ABW", "AD", "AE", "AF", "AFG", "AG", "AGO", "AI", "AIA", "AL", "ALA", "ALB", "AM", "AND", "AO", "AQ", "AR", "ARE",
    "ARG", "ARM", "AS", "ASM", "AT", "ATA", "ATF", "ATG", "AU", "AUS", "AUT", "AW", "AX", "AZ", "AZE", "BA", "BB",
    "BD", "BDI", "BE", "BEL", "BEN", "BES", "BF", "BFA", "BG", "BGD", "BGR", "BH", "BHR", "BHS", "BI", "BIH", "BJ",
    "BL", "BLM", "BLR", "BLZ", "BM", "BMU", "BN", "BO", "BOL", "BQ", "BR", "BRA", "BRB", "BRN", "BS", "BT", "BTN",
    "BV", "BVT", "BW", "BWA", "BY", "BZ", "CA", "CAF", "CAN", "CC", "CCK", "CD", "CF", "CG", "CH", "CHE", "CHL", "CHN",
    "CI", "CIV", "CK", "CL", "CM", "CMR", "CN", "CO", "COD", "COG", "COK", "COL", "COM", "CPV", "CR", "CRI", "CU",
    "CUB", "CUW", "CV", "CW", "CX", "CXR", "CY", "CYM", "CYP", "CZ", "CZE", "DE", "DEU", "DJ", "DJI", "DK", "DM",
    "DMA", "DNK", "DO", "DOM", "DZ", "DZA", "EC", "ECU", "EE", "EG", "EGY", "EH", "ER", "ERI", "ES", "ESH", "ESP",
    "EST", "ET", "ETH", "FI", "FIN", "FJ", "FJI", "FK", "FLK", "FM", "FO", "FR", "FRA", "FRO", "FSM", "GA", "GAB",
    "GB", "GBR", "GD", "GE", "GEO", "GF", "GG", "GGY", "GH", "GHA", "GI", "GIB", "GIN", "GL", "GLP", "GM", "GMB", "GN",
    "GNB", "GNQ", "GP", "GQ", "GR", "GRC", "GRD", "GRL", "GS", "GT", "GTM", "GU", "GUF", "GUM", "GUY", "GW", "GY",
    "HK", "HKG", "HM", "HMD", "HN", "HND", "HR", "HRV", "HT", "HTI", "HU", "HUN", "ID", "IDN", "IE", "IL", "IM", "IMN",
    "IN", "IND", "IO", "IOT", "IQ", "IR", "IRL", "IRN", "IRQ", "IS", "ISL", "ISR", "IT", "ITA", "JAM", "JE", "JEY",
    "JM", "JO", "JOR", "JP", "JPN", "KAZ", "KE", "KEN", "KG", "KGZ", "KH", "KHM", "KI", "KIR", "KM", "KN", "KNA",
    "KOR", "KP", "KR", "KW", "KWT", "KY", "KZ", "LA", "LAO", "LB", "LBN", "LBR", "LBY", "LC", "LCA", "LI", "LIE", "LK",
    "LKA", "LR", "LS", "LSO", "LT", "LTU", "LU", "LUX", "LV", "LVA", "LY", "MA", "MAC", "MAF", "MAR", "MC", "MCO",
    "MD", "MDA", "MDG", "MDV", "ME", "MEX", "MF", "MG", "MH", "MHL", "MK", "MKD", "ML", "MLI", "MLT", "MM", "MMR",
    "MN", "MNE", "MNG", "MNP", "MO", "MOZ", "MP", "MQ", "MR", "MRT", "MS", "MSR", "MT", "MTQ", "MU", "MUS", "MV", "MW",
    "MWI", "MX", "MY", "MYS", "MYT", "MZ", "NA", "NAM", "NC", "NCL", "NE", "NER", "NF", "NFK", "NG", "NGA", "NI",
    "NIC", "NIU", "NL", "NLD", "NO", "NOR", "NP", "NPL", "NR", "NRU", "NU", "NZ", "NZL", "OM", "OMN", "PA", "PAK",
    "PAN", "PCN", "PE", "PER", "PF", "PG", "PH", "PHL", "PK", "PL", "PLW", "PM", "PN", "PNG", "POL", "PR", "PRI",
    "PRK", "PRT", "PRY", "PS", "PSE", "PT", "PW", "PY", "PYF", "QA", "QAT", "RE", "REU", "RO", "ROU", "RS", "RU",
    "RUS", "RW", "RWA", "SA", "SAU", "SB", "SC", "SD", "SDN", "SE", "SEN", "SG", "SGP", "SGS", "SH", "SHN", "SI", "SJ",
    "SJM", "SK", "SL", "SLB", "SLE", "SLV", "SM", "SMR", "SN", "SO", "SOM", "SPM", "SR", "SRB", "SS", "SSD", "ST",
    "STP", "SUR", "SV", "SVK", "SVN", "SWE", "SWZ", "SX", "SXM", "SY", "SYC", "SYR", "SZ", "TC", "TCA", "TCD", "TD",
    "TF", "TG", "TGO", "TH", "THA", "TJ", "TJK", "TK", "TKL", "TKM", "TL", "TLS", "TM", "TN", "TO", "TON", "TR", "TT",
    "TTO", "TUN", "TUR", "TUV", "TV", "TW", "TWN", "TZ", "TZA", "UA", "UG", "UGA", "UKR", "UM", "UMI", "URY", "US",
    "USA", "UY", "UZ", "UZB", "VA", "VAT", "VC", "VCT", "VE", "VEN", "VG", "VGB", "VI", "VIR", "VN", "VNM", "VU",
    "VUT", "WF", "WLF", "WS", "WSM", "YE", "YEM", "YT", "ZA", "ZAF", "ZM", "ZMB", "ZW", "ZWE",
];

/// ISO 4217 alphabetic currency codes.
const CURRENCIES: &[&str] = &[
    "AED", "AFN", "ALL", "AMD", "AOA", "ARS", "AUD", "AWG", "AZN", "BAM", "BBD", "BDT", "BHD", "BIF", "BMD", "BND",
    "BOB", "BOV", "BRL", "BSD", "BTN", "BWP", "BYN", "BZD", "CAD", "CDF", "CHE", "CHF", "CHW", "CLF", "CLP", "CNY",
    "COP", "COU", "CRC", "CUP", "CVE", "CZK", "DJF", "DKK", "DOP", "DZD", "EGP", "ERN", "ETB", "EUR", "FJD", "FKP",
    "GBP", "GEL", "GHS", "GIP", "GMD", "GNF", "GTQ", "GYD", "HKD", "HNL", "HTG", "HUF", "IDR", "ILS", "INR", "IQD",
    "IRR", "ISK", "JMD", "JOD", "JPY", "KES", "KGS", "KHR", "KMF", "KPW", "KRW", "KWD", "KYD", "KZT", "LAK", "LBP",
    "LKR", "LRD", "LSL", "LYD", "MAD", "MDL", "MGA", "MKD", "MMK", "MNT", "MOP", "MRU", "MUR", "MVR", "MWK", "MXN",
    "MXV", "MYR", "MZN", "NAD", "NGN", "NIO", "NOK", "NPR", "NZD", "OMR", "PAB", "PEN", "PGK", "PHP", "PKR", "PLN",
    "PYG", "QAR", "RON", "RSD", "RUB", "RWF", "SAR", "SBD", "SCR", "SDG", "SEK", "SGD", "SHP", "SLE", "SOS", "SRD",
    "SSP", "STN", "SVC", "SYP", "SZL", "THB", "TJS", "TMT", "TND", "TOP", "TRY", "TTD", "TWD", "TZS", "UAH", "UGX",
    "USD", "USN", "UYI", "UYU", "UYW", "UZS", "VED", "VES", "VND", "VUV", "WST", "XAD", "XAF", "XAG", "XAU", "XBA",
    "XBB", "XBC", "XBD", "XCD", "XCG", "XDR", "XOF", "XPD", "XPF", "XPT", "XSU", "XTS", "XUA", "XXX", "YER", "ZAR",
    "ZMW", "ZWG",
];

/// ISO 639-1 language codes in capitals, as locale columns store them (`EN`, `CS`).
const LANGUAGES: &[&str] = &[
    "AA", "AB", "AE", "AF", "AK", "AM", "AN", "AR", "AS", "AV", "AY", "AZ", "BA", "BE", "BG", "BI", "BM", "BN", "BO",
    "BR", "BS", "CA", "CE", "CH", "CO", "CR", "CS", "CU", "CV", "CY", "DA", "DE", "DV", "DZ", "EE", "EL", "EN", "EO",
    "ES", "ET", "EU", "FA", "FF", "FI", "FJ", "FO", "FR", "FY", "GA", "GD", "GL", "GN", "GU", "GV", "HA", "HE", "HI",
    "HO", "HR", "HT", "HU", "HY", "HZ", "IA", "ID", "IE", "IG", "II", "IK", "IO", "IS", "IT", "IU", "JA", "JV", "KA",
    "KG", "KI", "KJ", "KK", "KL", "KM", "KN", "KO", "KR", "KS", "KU", "KV", "KW", "KY", "LA", "LB", "LG", "LI", "LN",
    "LO", "LT", "LU", "LV", "MG", "MH", "MI", "MK", "ML", "MN", "MR", "MS", "MT", "MY", "NA", "NB", "ND", "NE", "NG",
    "NL", "NN", "NO", "NR", "NV", "NY", "OC", "OJ", "OM", "OR", "OS", "PA", "PI", "PL", "PS", "PT", "QU", "RM", "RN",
    "RO", "RU", "RW", "SA", "SC", "SD", "SE", "SG", "SI", "SK", "SL", "SM", "SN", "SO", "SQ", "SR", "SS", "ST", "SU",
    "SV", "SW", "TA", "TE", "TG", "TH", "TI", "TK", "TL", "TN", "TO", "TR", "TS", "TT", "TW", "TY", "UG", "UK", "UR",
    "UZ", "VE", "VI", "VO", "WA", "WO", "XH", "YI", "YO", "ZA", "ZH", "ZU",
];

/// SQL keywords that catalog SQL (view definitions, defaults) spells in capitals.
const SQL_KEYWORDS: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "AND",
    "ANY",
    "AS",
    "ASC",
    "BETWEEN",
    "BY",
    "CASE",
    "CAST",
    "CHECK",
    "COLLATE",
    "COLUMN",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "CURRENT_DATE",
    "CURRENT_TIMESTAMP",
    "DEFAULT",
    "DEFINER",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DROP",
    "ELSE",
    "END",
    "EXISTS",
    "FALSE",
    "FETCH",
    "FOR",
    "FOREIGN",
    "FROM",
    "FULL",
    "GROUP",
    "HAVING",
    "IF",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LATERAL",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NATURAL",
    "NOT",
    "NULL",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "PARTITION",
    "PRIMARY",
    "REFERENCES",
    "RIGHT",
    "ROLLUP",
    "SECURITY",
    "SELECT",
    "SET",
    "SQL",
    "TABLE",
    "THEN",
    "TRUE",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VALUES",
    "VIEW",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];

/// Reports whether `word` is one of the listed codes, matched exactly.
pub(crate) fn is_code(word: &str) -> bool {
    [COUNTRIES, CURRENCIES, LANGUAGES, SQL_KEYWORDS]
        .iter()
        .any(|list| list.binary_search(&word).is_ok())
}

#[cfg(test)]
mod tests {
    use super::{COUNTRIES, CURRENCIES, LANGUAGES, SQL_KEYWORDS, is_code};

    #[test]
    fn sorted() {
        for list in [COUNTRIES, CURRENCIES, LANGUAGES, SQL_KEYWORDS] {
            assert!(list.is_sorted(), "{list:?}");
        }
    }

    #[test]
    fn matches_exact_uppercase_codes_only() {
        for code in ["AT", "AUT", "EUR", "EN", "AS", "VIEW"] {
            assert!(is_code(code), "{code:?}");
        }
        for word in ["at", "As", "IBM", "LEE", "SAP", "NYC", ""] {
            assert!(!is_code(word), "{word:?}");
        }
    }
}
