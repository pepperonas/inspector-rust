//! The faker registry — the **single source of truth** for every generator.
//!
//! A metadata table ([`CATALOG`]) describes each generator (name, aliases,
//! category, description, real locale support, composite/numeric flags); one
//! [`gen_one`] match produces a value. Command, template and the `{faker:…}`
//! snippet placeholder all route through the same two, so there is never a
//! second generator list to drift out of sync.

use super::locale::{Locale, LOC_ADDRESS, LOC_EN, LOC_LOREM, LOC_NAME, LOC_PHONE};
use rand010::rngs::StdRng;
use rand010::{Rng, RngExt}; // Rng: fill_bytes; RngExt: random/random_range/random_bool
use serde_json::{json, Value};

/// Static metadata for one generator (no generation logic — that's `gen_one`).
pub struct GenSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub category: &'static str,
    pub description: &'static str,
    /// Locales that actually localise this generator (EN implicit).
    pub supported: &'static [Locale],
    pub composite: bool,
    /// Accepts a range/format arg (`int 1..100`, `date:%d.%m.%Y`, `words:5`).
    pub numeric: bool,
    /// Ordered field names for a composite record (empty for scalars).
    pub fields: &'static [&'static str],
}

/// Parsed generator argument (the part after the name / `:`).
#[derive(Default, Clone)]
pub struct Args {
    pub raw: Option<String>,
}

impl Args {
    pub fn new(raw: Option<String>) -> Self {
        Args { raw }
    }
    /// `1..100` / `1..=100` → inclusive `(lo, hi)`, else the defaults.
    pub fn int_range(&self, lo: i64, hi: i64) -> (i64, i64) {
        let Some(r) = self.raw.as_deref() else { return (lo, hi) };
        let r = r.trim();
        let (a, b) = if let Some((a, b)) = r.split_once("..=") {
            (a, b)
        } else if let Some((a, b)) = r.split_once("..") {
            (a, b)
        } else {
            return (lo, hi);
        };
        let a = a.trim().parse().unwrap_or(lo);
        let b = b.trim().parse().unwrap_or(hi);
        if a <= b {
            (a, b)
        } else {
            (b, a)
        }
    }
    /// A bare count (`words:5`), else the default.
    pub fn count(&self, default: usize) -> usize {
        self.raw
            .as_deref()
            .and_then(|s| s.trim().parse().ok())
            .filter(|n: &usize| *n >= 1 && *n <= 200)
            .unwrap_or(default)
    }
    /// A strftime format string (`date:%d.%m.%Y`), taken verbatim.
    pub fn fmt(&self) -> Option<&str> {
        self.raw.as_deref().filter(|s| !s.is_empty())
    }
}

/// Dispatch to the typed fake faker for a runtime locale. `$faker` is the faker
/// constructor path; it is called with the matching `fake::locales::*` marker.
/// Every listed faker is generic over all 15 locales (fake inherits EN via
/// trait defaults), so this always compiles; [`super::locale::resolve`] has
/// already collapsed unsupported locales to EN before we get here.
macro_rules! localized {
    ($rng:expr, $loc:expr, $faker:path) => {{
        use fake::locales::*;
        use fake::Fake;
        let s: String = match $loc {
            Locale::En => $faker(EN).fake_with_rng($rng),
            Locale::DeDe => $faker(DE_DE).fake_with_rng($rng),
            Locale::FrFr => $faker(FR_FR).fake_with_rng($rng),
            Locale::ItIt => $faker(IT_IT).fake_with_rng($rng),
            Locale::PtBr => $faker(PT_BR).fake_with_rng($rng),
            Locale::PtPt => $faker(PT_PT).fake_with_rng($rng),
            Locale::NlNl => $faker(NL_NL).fake_with_rng($rng),
            Locale::JaJp => $faker(JA_JP).fake_with_rng($rng),
            Locale::ZhCn => $faker(ZH_CN).fake_with_rng($rng),
            Locale::ZhTw => $faker(ZH_TW).fake_with_rng($rng),
            Locale::ArSa => $faker(AR_SA).fake_with_rng($rng),
            Locale::CyGb => $faker(CY_GB).fake_with_rng($rng),
            Locale::FaIr => $faker(FA_IR).fake_with_rng($rng),
            Locale::TrTr => $faker(TR_TR).fake_with_rng($rng),
        };
        s
    }};
}

/// EN-only string faker (locale-independent generators).
macro_rules! en {
    ($rng:expr, $faker:path) => {{
        use fake::locales::EN;
        use fake::Fake;
        let s: String = $faker(EN).fake_with_rng($rng);
        s
    }};
}

const HEX: &[u8] = b"0123456789abcdef";

fn hex_string(rng: &mut StdRng, n: usize) -> String {
    (0..n).map(|_| HEX[rng.random_range(0..16)] as char).collect()
}

fn uuid_v4(rng: &mut StdRng) -> String {
    let mut b = [0u8; 16];
    rng.fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// ISO 3166 mod-97 check digits for a syntactically valid (fictional) IBAN.
fn iban(rng: &mut StdRng, country: &str) -> String {
    // BBAN: 18 digits (DE-style length works for a generic fake).
    let bban: String = (0..18).map(|_| (b'0' + rng.random_range(0..10) as u8) as char).collect();
    // Move country+"00" to the end, letters → digits (A=10…Z=35), mod 97.
    let rearranged = format!("{bban}{country}00");
    let mut numeric = String::new();
    for c in rearranged.chars() {
        if c.is_ascii_digit() {
            numeric.push(c);
        } else {
            numeric.push_str(&(c as u32 - 'A' as u32 + 10).to_string());
        }
    }
    let mut rem = 0u32;
    for c in numeric.chars() {
        rem = (rem * 10 + c.to_digit(10).unwrap()) % 97;
    }
    let check = 98 - rem;
    format!("{country}{check:02}{bban}")
}

/// EAN-13 with a valid check digit (fictional).
fn ean13(rng: &mut StdRng) -> String {
    let mut d: Vec<u32> = (0..12).map(|_| rng.random_range(0..10)).collect();
    let sum: u32 = d.iter().enumerate().map(|(i, x)| if i % 2 == 0 { *x } else { *x * 3 }).sum();
    let check = (10 - (sum % 10)) % 10;
    d.push(check);
    d.iter().map(|x| char::from_digit(*x, 10).unwrap()).collect()
}

/// Generate ONE value for `name`. Returns `None` for an unknown generator.
/// `locale` is already resolved (supported-or-EN). Composites recurse.
pub fn gen_one(name: &str, rng: &mut StdRng, locale: Locale, args: &Args) -> Option<Value> {
    use fake::faker;
    let s: String = match name {
        // ── Person ──────────────────────────────────────────────────────
        "first_name" => localized!(rng, locale, faker::name::raw::FirstName),
        "last_name" => localized!(rng, locale, faker::name::raw::LastName),
        "name" => localized!(rng, locale, faker::name::raw::Name),
        "title" => localized!(rng, locale, faker::name::raw::Title),
        "suffix" => localized!(rng, locale, faker::name::raw::Suffix),
        "job" => en!(rng, faker::job::raw::Title),
        "gender" => ["male", "female", "non-binary"][rng.random_range(0..3)].to_string(),
        // ── Internet ────────────────────────────────────────────────────
        "email" => en!(rng, faker::internet::raw::FreeEmail),
        "free_email" => en!(rng, faker::internet::raw::FreeEmail),
        "safe_email" => en!(rng, faker::internet::raw::SafeEmail),
        "username" => en!(rng, faker::internet::raw::Username),
        "password" => {
            use fake::locales::EN;
            use fake::Fake;
            let p: String = faker::internet::raw::Password(EN, 10..17).fake_with_rng(rng);
            p
        }
        "domain" => {
            let w = en!(rng, faker::lorem::raw::Word).to_lowercase();
            let suffix = en!(rng, faker::internet::raw::DomainSuffix);
            format!("{w}.{suffix}")
        }
        "url" => {
            let w = en!(rng, faker::lorem::raw::Word).to_lowercase();
            let suffix = en!(rng, faker::internet::raw::DomainSuffix);
            format!("https://www.{w}.{suffix}/")
        }
        "ipv4" => en!(rng, faker::internet::raw::IPv4),
        "ipv6" => en!(rng, faker::internet::raw::IPv6),
        "mac" => en!(rng, faker::internet::raw::MACAddress),
        "user_agent" => en!(rng, faker::internet::raw::UserAgent),
        // ── Address ─────────────────────────────────────────────────────
        "street" => localized!(rng, locale, faker::address::raw::StreetName),
        "building" => localized!(rng, locale, faker::address::raw::BuildingNumber),
        "city" => localized!(rng, locale, faker::address::raw::CityName),
        "zip" => localized!(rng, locale, faker::address::raw::ZipCode),
        "state" => localized!(rng, locale, faker::address::raw::StateName),
        "state_abbr" => localized!(rng, locale, faker::address::raw::StateAbbr),
        "country" => localized!(rng, locale, faker::address::raw::CountryName),
        "country_code" => localized!(rng, locale, faker::address::raw::CountryCode),
        "address" => {
            let street = localized!(rng, locale, faker::address::raw::StreetName);
            let building = localized!(rng, locale, faker::address::raw::BuildingNumber);
            let zip = localized!(rng, locale, faker::address::raw::ZipCode);
            let city = localized!(rng, locale, faker::address::raw::CityName);
            let country = localized!(rng, locale, faker::address::raw::CountryName);
            format!("{street} {building}\n{zip} {city}\n{country}")
        }
        "latlng" => {
            let lat = rng.random_range(-9000..=9000) as f64 / 100.0;
            let lng = rng.random_range(-18000..=18000) as f64 / 100.0;
            format!("{lat:.4},{lng:.4}")
        }
        // ── Phone ───────────────────────────────────────────────────────
        "phone" => localized!(rng, locale, faker::phone_number::raw::PhoneNumber),
        "cell" => localized!(rng, locale, faker::phone_number::raw::CellNumber),
        // ── Company ─────────────────────────────────────────────────────
        "company" => en!(rng, faker::company::raw::CompanyName),
        "industry" => en!(rng, faker::company::raw::Industry),
        "catchphrase" => en!(rng, faker::company::raw::CatchPhrase),
        "buzzword" => en!(rng, faker::company::raw::Buzzword),
        "profession" => en!(rng, faker::company::raw::Profession),
        // ── Finance ─────────────────────────────────────────────────────
        "iban" => iban(rng, "DE"),
        "bic" => {
            let bank = hex_string(rng, 4).to_uppercase();
            format!("{bank}DEFFXXX")
        }
        "credit_card" => en!(rng, faker::creditcard::raw::CreditCardNumber),
        "currency_code" => en!(rng, faker::currency::raw::CurrencyCode),
        "currency_name" => en!(rng, faker::currency::raw::CurrencyName),
        "currency_symbol" => en!(rng, faker::currency::raw::CurrencySymbol),
        "bitcoin" => {
            const B58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
            let n = rng.random_range(26..=34);
            let body: String = (0..n).map(|_| B58[rng.random_range(0..B58.len())] as char).collect();
            format!("1{body}")
        }
        // ── Lorem ───────────────────────────────────────────────────────
        "word" => localized!(rng, locale, faker::lorem::raw::Word),
        "words" => {
            use fake::locales::*;
            use fake::Fake;
            let n = args.count(3);
            let v: Vec<String> = match locale {
                Locale::ZhCn => faker::lorem::raw::Words(ZH_CN, n..n + 1).fake_with_rng(rng),
                Locale::FaIr => faker::lorem::raw::Words(FA_IR, n..n + 1).fake_with_rng(rng),
                _ => faker::lorem::raw::Words(EN, n..n + 1).fake_with_rng(rng),
            };
            v.join(" ")
        }
        "sentence" => {
            use fake::locales::EN;
            use fake::Fake;
            let s: String = faker::lorem::raw::Sentence(EN, 4..12).fake_with_rng(rng);
            s
        }
        "paragraph" => {
            use fake::locales::EN;
            use fake::Fake;
            let s: String = faker::lorem::raw::Paragraph(EN, 3..6).fake_with_rng(rng);
            s
        }
        // ── Time ────────────────────────────────────────────────────────
        "date" => {
            use fake::locales::EN;
            use fake::Fake;
            if let Some(fmt) = args.fmt() {
                let d: chrono::NaiveDate = faker::chrono::raw::Date(EN).fake_with_rng(rng);
                d.format(fmt).to_string()
            } else {
                faker::chrono::raw::Date(EN).fake_with_rng(rng)
            }
        }
        "time" => {
            use fake::locales::EN;
            use fake::Fake;
            faker::chrono::raw::Time(EN).fake_with_rng(rng)
        }
        "datetime" | "iso8601" => {
            use fake::locales::EN;
            use fake::Fake;
            faker::chrono::raw::DateTime(EN).fake_with_rng(rng)
        }
        "unix" => rng.random_range(1_000_000_000i64..=1_900_000_000).to_string(),
        // ── Numbers ─────────────────────────────────────────────────────
        "int" => {
            let (lo, hi) = args.int_range(0, 1000);
            return Some(json!(rng.random_range(lo..=hi)));
        }
        "float" => {
            let (lo, hi) = args.int_range(0, 1000);
            let f = lo as f64 + rng.random::<f64>() * (hi - lo) as f64;
            return Some(json!((f * 100.0).round() / 100.0));
        }
        "bool" => return Some(json!(rng.random_bool(0.5))),
        "digit" => char::from_digit(rng.random_range(0..10), 10).unwrap().to_string(),
        // ── Misc ────────────────────────────────────────────────────────
        "uuid" => uuid_v4(rng),
        "isbn" => en!(rng, faker::barcode::raw::Isbn13),
        "ean" => ean13(rng),
        "barcode" => ean13(rng),
        "license_plate" => {
            // fake only impls LicencePlate for FR/IT/NL, and it's not a useful
            // generic — hand-roll a plausible German-style plate.
            let region = ["B", "M", "K", "HH", "F", "S", "L"][rng.random_range(0..7)];
            let letters: String = (0..2)
                .map(|_| (b'A' + rng.random_range(0..26) as u8) as char)
                .collect();
            format!("{region}-{letters} {}", rng.random_range(100..=9999))
        }
        "color" => {
            // fake's `color` module is behind the `random_color` feature; a
            // small CSS-name list avoids that dep.
            const NAMES: &[&str] = &[
                "red", "green", "blue", "cyan", "magenta", "yellow", "black", "white",
                "gray", "orange", "purple", "pink", "brown", "teal", "navy", "olive",
                "maroon", "lime", "aqua", "silver",
            ];
            NAMES[rng.random_range(0..NAMES.len())].to_string()
        }
        "hex_color" => format!("#{}", hex_string(rng, 6)),
        "mime" => en!(rng, faker::filesystem::raw::MimeType),
        "filepath" => en!(rng, faker::filesystem::raw::FilePath),
        "semver" => format!(
            "{}.{}.{}",
            rng.random_range(0..3),
            rng.random_range(0..20),
            rng.random_range(0..20)
        ),
        "hash" => hex_string(rng, 40),
        // ── Composites ──────────────────────────────────────────────────
        _ if name == "person"
            || name == "user"
            || name == "address_full"
            || name == "company_full"
            || name == "order" =>
        {
            return Some(gen_composite(name, rng, locale));
        }
        _ => return None,
    };
    Some(Value::String(s))
}

/// Build a coherent composite record. Where natural, fields are derived from
/// each other (email/username from the generated name) so a `person` row reads
/// like one real person, not a bag of unrelated values.
fn gen_composite(name: &str, rng: &mut StdRng, locale: Locale) -> Value {
    let one = |rng: &mut StdRng, g: &str, loc: Locale| -> String {
        match gen_one(g, rng, loc, &Args::default()) {
            Some(Value::String(s)) => s,
            Some(v) => v.to_string(),
            None => String::new(),
        }
    };
    let slug = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .to_lowercase()
            .replace(' ', ".")
    };
    match name {
        "person" => {
            let first = one(rng, "first_name", locale);
            let last = one(rng, "last_name", locale);
            let email = format!("{}.{}@example.com", slug(&first), slug(&last));
            json!({
                "first_name": first,
                "last_name": last,
                "email": email,
                "phone": one(rng, "phone", locale),
                "city": one(rng, "city", locale),
            })
        }
        "user" => {
            let first = one(rng, "first_name", locale);
            let last = one(rng, "last_name", locale);
            let username = format!("{}{}", slug(&first).replace('.', ""), rng.random_range(1..999));
            json!({
                "id": uuid_v4(rng),
                "username": username,
                "email": format!("{}.{}@example.com", slug(&first), slug(&last)),
                "password": one(rng, "password", locale),
            })
        }
        "address_full" => json!({
            "street": one(rng, "street", locale),
            "building": one(rng, "building", locale),
            "zip": one(rng, "zip", locale),
            "city": one(rng, "city", locale),
            "state": one(rng, "state", locale),
            "country": one(rng, "country", locale),
        }),
        "company_full" => json!({
            "company": one(rng, "company", locale),
            "industry": one(rng, "industry", locale),
            "catchphrase": one(rng, "catchphrase", locale),
            "buzzword": one(rng, "buzzword", locale),
        }),
        "order" => json!({
            "id": uuid_v4(rng),
            "customer": format!("{} {}", one(rng, "first_name", locale), one(rng, "last_name", locale)),
            "quantity": rng.random_range(1..=20),
            "total": (rng.random_range(100..100000) as f64 / 100.0),
            "date": one(rng, "date", locale),
        }),
        _ => json!({}),
    }
}

/// The metadata catalogue (single source of truth for the UI + resolution).
#[rustfmt::skip]
pub const CATALOG: &[GenSpec] = &[
    // Person
    GenSpec { name: "first_name", aliases: &["fn", "vorname"], category: "Person", description: "Given name", supported: LOC_NAME, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "last_name", aliases: &["ln", "nachname"], category: "Person", description: "Family name", supported: LOC_NAME, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "name", aliases: &["fullname"], category: "Person", description: "Full name", supported: LOC_NAME, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "title", aliases: &["salutation"], category: "Person", description: "Name prefix (Mr/Dr)", supported: LOC_NAME, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "suffix", aliases: &[], category: "Person", description: "Name suffix (Jr/PhD)", supported: LOC_NAME, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "job", aliases: &["jobtitle"], category: "Person", description: "Job title", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "gender", aliases: &[], category: "Person", description: "Gender", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    // Internet
    GenSpec { name: "email", aliases: &["mail"], category: "Internet", description: "Email address", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "free_email", aliases: &[], category: "Internet", description: "Free-provider email", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "safe_email", aliases: &[], category: "Internet", description: "example.* email", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "username", aliases: &["uname"], category: "Internet", description: "Username", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "password", aliases: &["pw"], category: "Internet", description: "Password (toy — see pwgen)", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "domain", aliases: &[], category: "Internet", description: "Domain name", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "url", aliases: &[], category: "Internet", description: "URL", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "ipv4", aliases: &["ip"], category: "Internet", description: "IPv4 address", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "ipv6", aliases: &[], category: "Internet", description: "IPv6 address", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "mac", aliases: &[], category: "Internet", description: "MAC address", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "user_agent", aliases: &["ua"], category: "Internet", description: "Browser user-agent", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    // Address
    GenSpec { name: "street", aliases: &["strasse"], category: "Address", description: "Street name", supported: LOC_ADDRESS, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "building", aliases: &["housenumber"], category: "Address", description: "Building number", supported: LOC_ADDRESS, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "city", aliases: &["stadt"], category: "Address", description: "City", supported: LOC_ADDRESS, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "zip", aliases: &["plz", "postcode"], category: "Address", description: "Postal code", supported: LOC_ADDRESS, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "state", aliases: &["bundesland"], category: "Address", description: "State / region", supported: LOC_ADDRESS, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "state_abbr", aliases: &[], category: "Address", description: "State abbreviation", supported: LOC_ADDRESS, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "country", aliases: &["land"], category: "Address", description: "Country", supported: LOC_ADDRESS, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "country_code", aliases: &[], category: "Address", description: "ISO country code", supported: LOC_ADDRESS, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "address", aliases: &["adresse"], category: "Address", description: "Full postal address (multi-line)", supported: LOC_ADDRESS, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "latlng", aliases: &["geo", "coords"], category: "Address", description: "Latitude,longitude", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    // Phone
    GenSpec { name: "phone", aliases: &["tel", "telefon"], category: "Phone", description: "Phone number", supported: LOC_PHONE, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "cell", aliases: &["mobile", "handy"], category: "Phone", description: "Mobile number", supported: LOC_PHONE, composite: false, numeric: false, fields: &[] },
    // Company
    GenSpec { name: "company", aliases: &["firma"], category: "Company", description: "Company name", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "industry", aliases: &[], category: "Company", description: "Industry", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "catchphrase", aliases: &[], category: "Company", description: "Catchphrase", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "buzzword", aliases: &[], category: "Company", description: "Buzzword", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "profession", aliases: &["beruf"], category: "Company", description: "Profession", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    // Finance
    GenSpec { name: "iban", aliases: &[], category: "Finance", description: "IBAN (valid check digits, fictional)", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "bic", aliases: &["swift"], category: "Finance", description: "BIC / SWIFT", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "credit_card", aliases: &["cc"], category: "Finance", description: "Credit-card number (fictional)", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "currency_code", aliases: &[], category: "Finance", description: "Currency code (EUR)", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "currency_name", aliases: &[], category: "Finance", description: "Currency name", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "currency_symbol", aliases: &[], category: "Finance", description: "Currency symbol", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "bitcoin", aliases: &["btc"], category: "Finance", description: "Bitcoin address (fictional)", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    // Lorem
    GenSpec { name: "word", aliases: &[], category: "Lorem", description: "One word", supported: LOC_LOREM, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "words", aliases: &[], category: "Lorem", description: "N words (words 5)", supported: LOC_LOREM, composite: false, numeric: true, fields: &[] },
    GenSpec { name: "sentence", aliases: &[], category: "Lorem", description: "A sentence", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "paragraph", aliases: &[], category: "Lorem", description: "A paragraph", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    // Time
    GenSpec { name: "date", aliases: &[], category: "Time", description: "Date (date %d.%m.%Y)", supported: LOC_EN, composite: false, numeric: true, fields: &[] },
    GenSpec { name: "time", aliases: &[], category: "Time", description: "Time of day", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "datetime", aliases: &[], category: "Time", description: "Date + time", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "iso8601", aliases: &[], category: "Time", description: "ISO-8601 timestamp", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "unix", aliases: &["epoch"], category: "Time", description: "Unix timestamp", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    // Numbers
    GenSpec { name: "int", aliases: &["number"], category: "Numbers", description: "Integer (int 1..100)", supported: LOC_EN, composite: false, numeric: true, fields: &[] },
    GenSpec { name: "float", aliases: &[], category: "Numbers", description: "Decimal (float 0..1)", supported: LOC_EN, composite: false, numeric: true, fields: &[] },
    GenSpec { name: "bool", aliases: &["boolean"], category: "Numbers", description: "true / false", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "digit", aliases: &[], category: "Numbers", description: "Single digit 0-9", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    // Misc
    GenSpec { name: "uuid", aliases: &["guid"], category: "Misc", description: "UUID v4", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "isbn", aliases: &[], category: "Misc", description: "ISBN-13", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "ean", aliases: &[], category: "Misc", description: "EAN-13 barcode", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "barcode", aliases: &[], category: "Misc", description: "Barcode (EAN-13)", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "license_plate", aliases: &["plate", "kennzeichen"], category: "Misc", description: "License plate", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "color", aliases: &["colour"], category: "Misc", description: "Named color", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "hex_color", aliases: &["hex"], category: "Misc", description: "#rrggbb color", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "mime", aliases: &["mimetype"], category: "Misc", description: "MIME type", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "filepath", aliases: &["path"], category: "Misc", description: "File path", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "semver", aliases: &["version"], category: "Misc", description: "Semantic version", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    GenSpec { name: "hash", aliases: &["sha"], category: "Misc", description: "40-char hex hash", supported: LOC_EN, composite: false, numeric: false, fields: &[] },
    // Composites
    GenSpec { name: "person", aliases: &["persona"], category: "Composite", description: "Person record (name, email, phone, city)", supported: LOC_NAME, composite: true, numeric: false, fields: &["first_name", "last_name", "email", "phone", "city"] },
    GenSpec { name: "user", aliases: &["account"], category: "Composite", description: "User account (id, username, email, password)", supported: LOC_NAME, composite: true, numeric: false, fields: &["id", "username", "email", "password"] },
    GenSpec { name: "address_full", aliases: &[], category: "Composite", description: "Full address record", supported: LOC_ADDRESS, composite: true, numeric: false, fields: &["street", "building", "zip", "city", "state", "country"] },
    GenSpec { name: "company_full", aliases: &[], category: "Composite", description: "Company record", supported: LOC_EN, composite: true, numeric: false, fields: &["company", "industry", "catchphrase", "buzzword"] },
    GenSpec { name: "order", aliases: &[], category: "Composite", description: "Order record", supported: LOC_NAME, composite: true, numeric: false, fields: &["id", "customer", "quantity", "total", "date"] },
];

/// Look up a spec by name or alias. An exact **name** match always wins over an
/// alias match, so a composite (`user`) can't be shadowed by another
/// generator's alias (`username`'s old `user` alias) regardless of table order.
pub fn lookup(name: &str) -> Option<&'static GenSpec> {
    let n = name.trim().to_ascii_lowercase();
    CATALOG
        .iter()
        .find(|g| g.name == n)
        .or_else(|| CATALOG.iter().find(|g| g.aliases.iter().any(|a| *a == n)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Args::int_range ──────────────────────────────────────────────────────

    fn args(raw: &str) -> Args {
        Args::new(Some(raw.to_string()))
    }

    #[test]
    fn int_range_none_and_garbage_fall_back_to_defaults() {
        assert_eq!(Args::new(None).int_range(1, 100), (1, 100));
        assert_eq!(args("nonsense").int_range(1, 100), (1, 100));
        assert_eq!(args("").int_range(2, 9), (2, 9));
        // A lone bound with no `..` is not a range → defaults, not a panic.
        assert_eq!(args("50").int_range(1, 100), (1, 100));
    }

    #[test]
    fn int_range_parses_exclusive_and_inclusive() {
        assert_eq!(args("1..10").int_range(0, 0), (1, 10));
        assert_eq!(args("1..=10").int_range(0, 0), (1, 10));
        // `..=` is matched before `..`, so the trailing `=` is never leaked
        // into the second bound (which would make it unparseable → default).
        assert_eq!(args("5..=5").int_range(9, 9), (5, 5));
    }

    #[test]
    fn int_range_swaps_a_reversed_range() {
        // A reversed range is normalised, never yields lo > hi.
        assert_eq!(args("100..1").int_range(0, 0), (1, 100));
        assert_eq!(args("100..=1").int_range(0, 0), (1, 100));
    }

    #[test]
    fn int_range_tolerates_whitespace_and_negatives() {
        assert_eq!(args(" -5 .. 5 ").int_range(0, 0), (-5, 5));
        assert_eq!(args("-10..-2").int_range(0, 0), (-10, -2));
        // A single unparseable bound falls back to that side's default.
        assert_eq!(args("x..10").int_range(1, 99), (1, 10));
        assert_eq!(args("1..y").int_range(0, 99), (1, 99));
    }

    // ── Args::count ──────────────────────────────────────────────────────────

    #[test]
    fn count_accepts_the_valid_band_and_clamps_the_rest_to_the_default() {
        assert_eq!(args("5").count(3), 5);
        assert_eq!(args("1").count(3), 1); // lower edge in-band
        assert_eq!(args("200").count(3), 200); // upper edge in-band
        assert_eq!(args("0").count(3), 3); // below band → default
        assert_eq!(args("201").count(3), 3); // above band → default
        assert_eq!(args("nope").count(7), 7); // garbage → default
        assert_eq!(Args::new(None).count(4), 4); // absent → default
        assert_eq!(args("  10 ").count(3), 10); // trimmed
    }

    // ── Args::fmt ────────────────────────────────────────────────────────────

    #[test]
    fn fmt_returns_the_raw_string_or_none_when_empty() {
        assert_eq!(args("%d.%m.%Y").fmt(), Some("%d.%m.%Y"));
        assert_eq!(args("").fmt(), None);
        assert_eq!(Args::new(None).fmt(), None);
        // A format made only of spaces is NOT empty → passed verbatim.
        assert_eq!(args("   ").fmt(), Some("   "));
    }

    // ── lookup ───────────────────────────────────────────────────────────────

    #[test]
    fn lookup_is_case_and_whitespace_insensitive() {
        assert_eq!(lookup("  FIRST_NAME ").unwrap().name, "first_name");
        assert_eq!(lookup("VORNAME").unwrap().name, "first_name"); // via alias
        assert!(lookup("definitely-not-a-generator").is_none());
    }

    #[test]
    fn lookup_prefers_an_exact_name_over_an_alias() {
        // `user` is a composite generator AND `username` is a separate one; the
        // exact-name match must win regardless of table order.
        assert!(lookup("user").unwrap().composite);
        assert_eq!(lookup("username").unwrap().name, "username");
    }

    // ── CATALOG structural invariants ────────────────────────────────────────

    #[test]
    fn every_name_and_alias_is_globally_unique() {
        let mut seen = std::collections::HashSet::new();
        for g in CATALOG {
            assert!(seen.insert(g.name), "duplicate generator name {}", g.name);
            for a in g.aliases {
                assert!(seen.insert(*a), "alias {a} collides (generator {})", g.name);
            }
        }
    }

    #[test]
    fn names_and_aliases_are_lowercase_with_no_whitespace() {
        // `lookup` lowercases the query, so an upper-case catalogue key would be
        // permanently unreachable.
        for g in CATALOG {
            for key in std::iter::once(g.name).chain(g.aliases.iter().copied()) {
                assert_eq!(key, key.to_ascii_lowercase(), "{key} must be lowercase");
                assert!(!key.chars().any(char::is_whitespace), "{key} has whitespace");
                assert!(!key.is_empty(), "empty key in generator {}", g.name);
            }
        }
    }

    #[test]
    fn composites_declare_fields_and_scalars_do_not() {
        for g in CATALOG {
            if g.composite {
                assert!(!g.fields.is_empty(), "composite {} has no fields", g.name);
            } else {
                assert!(g.fields.is_empty(), "scalar {} declares fields", g.name);
            }
        }
    }

    #[test]
    fn every_generator_carries_a_category_and_description() {
        for g in CATALOG {
            assert!(!g.category.is_empty(), "{} has no category", g.name);
            assert!(!g.description.is_empty(), "{} has no description", g.name);
        }
    }

    // ── Check-digit math (the silent-failure-prone bits) ─────────────────────

    /// Validate an IBAN by the ISO-13616 rule: move the first 4 chars to the
    /// end, expand letters A→10…Z→35 into their decimal digits, and the whole
    /// number taken mod 97 must equal 1. Mirrors the standard, not the module.
    fn iban_mod97_is_one(iban: &str) -> bool {
        if iban.len() < 5 {
            return false;
        }
        let rearranged = format!("{}{}", &iban[4..], &iban[..4]);
        let mut rem: u32 = 0;
        for c in rearranged.chars() {
            let val = if c.is_ascii_digit() {
                c as u32 - '0' as u32
            } else if c.is_ascii_uppercase() {
                c as u32 - 'A' as u32 + 10
            } else {
                return false;
            };
            // A letter contributes TWO decimal digits (10..=35) — feed both.
            if val >= 10 {
                rem = (rem * 10 + val / 10) % 97;
                rem = (rem * 10 + val % 10) % 97;
            } else {
                rem = (rem * 10 + val) % 97;
            }
        }
        rem == 1
    }

    #[test]
    fn iban_has_valid_iso13616_check_digits() {
        use rand010::SeedableRng;
        // The check-digit arithmetic is the kind of thing that "looks right" but
        // silently produces IBANs that fail validation. Pin it against the real
        // mod-97 rule across many seeds so a regression in the letter→digit
        // expansion or the `98 - rem` step is caught.
        for seed in 0..64u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let acct = iban(&mut rng, "DE");
            assert!(acct.starts_with("DE"), "country prefix preserved: {acct}");
            assert_eq!(acct.len(), 22, "a DE IBAN is 22 chars: {acct}");
            // Check digits are always 02..=98 (two digits, never 00/01/99).
            let check: u32 = acct[2..4].parse().expect("check digits are numeric");
            assert!((2..=98).contains(&check), "check digits out of range: {acct}");
            assert!(iban_mod97_is_one(&acct), "IBAN fails mod-97 validation: {acct}");
        }
    }

    #[test]
    fn ean13_has_a_valid_gtin_check_digit() {
        use rand010::SeedableRng;
        for seed in 0..64u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let code = ean13(&mut rng);
            assert_eq!(code.len(), 13, "EAN-13 is 13 digits: {code}");
            assert!(code.chars().all(|c| c.is_ascii_digit()), "all digits: {code}");
            // GTIN-13: Σ dᵢ·(i even → 1, i odd → 3) ≡ 0 (mod 10).
            let sum: u32 = code
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    let d = c.to_digit(10).unwrap();
                    if i % 2 == 0 { d } else { d * 3 }
                })
                .sum();
            assert_eq!(sum % 10, 0, "invalid EAN-13 check digit: {code}");
        }
    }

    #[test]
    fn uuid_v4_is_canonical_with_version_and_variant_bits_set() {
        use rand010::SeedableRng;
        // RFC 4122 §4.4: version nibble = 4, variant nibble ∈ {8,9,a,b}, and the
        // canonical 8-4-4-4-12 hyphenation. A bug in the bit-masking would leak a
        // non-v4 UUID that some parsers reject.
        for seed in 0..64u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let u = uuid_v4(&mut rng);
            assert_eq!(u.len(), 36, "canonical length: {u}");
            let b = u.as_bytes();
            for i in [8usize, 13, 18, 23] {
                assert_eq!(b[i], b'-', "hyphen expected at {i}: {u}");
            }
            for (i, c) in u.chars().enumerate() {
                if [8, 13, 18, 23].contains(&i) {
                    continue;
                }
                assert!(
                    c.is_ascii_hexdigit() && !c.is_ascii_uppercase(),
                    "non-lowercase-hex at {i}: {u}"
                );
            }
            assert_eq!(&u[14..15], "4", "version nibble must be 4: {u}");
            assert!(
                matches!(&u[19..20], "8" | "9" | "a" | "b"),
                "variant nibble must be 8/9/a/b: {u}"
            );
        }
    }

    // ── gen_one ↔ CATALOG referential integrity ──────────────────────────────

    #[test]
    fn gen_one_produces_a_value_for_every_catalogue_entry() {
        use super::super::locale::Locale;
        use rand010::SeedableRng;
        // A catalogue name with no matching `gen_one` arm (typo, forgotten arm)
        // would surface in the UI but silently generate nothing — this catches
        // that drift for every scalar AND composite.
        let mut rng = StdRng::seed_from_u64(12345);
        for g in CATALOG {
            assert!(
                gen_one(g.name, &mut rng, Locale::En, &Args::default()).is_some(),
                "catalogue generator `{}` has no gen_one arm",
                g.name
            );
        }
        // An unknown name is None — never a panic or a bogus value.
        assert!(
            gen_one("definitely_not_a_generator", &mut rng, Locale::En, &Args::default()).is_none()
        );
    }

    #[test]
    fn composite_metadata_fields_match_the_generated_record_keys() {
        use super::super::locale::Locale;
        use rand010::SeedableRng;
        use std::collections::BTreeSet;
        // The `fields` metadata drives the UI's column list; if it drifts from
        // the JSON `gen_composite` actually emits, the UI shows phantom / missing
        // columns. Pin the two together.
        let mut rng = StdRng::seed_from_u64(7);
        for g in CATALOG.iter().filter(|g| g.composite) {
            let v = gen_composite(g.name, &mut rng, Locale::En);
            let obj = v
                .as_object()
                .unwrap_or_else(|| panic!("composite {} is not a JSON object", g.name));
            let produced: BTreeSet<&str> = obj.keys().map(String::as_str).collect();
            let declared: BTreeSet<&str> = g.fields.iter().copied().collect();
            assert_eq!(produced, declared, "composite {} keys drift from its metadata", g.name);
        }
    }

    #[test]
    fn numeric_generators_honour_the_range_argument() {
        use super::super::locale::Locale;
        use rand010::SeedableRng;
        let mut rng = StdRng::seed_from_u64(99);
        // A degenerate lo==hi range pins the value exactly, regardless of RNG.
        assert_eq!(
            gen_one("int", &mut rng, Locale::En, &Args::new(Some("42..42".into()))).unwrap(),
            json!(42)
        );
        assert_eq!(
            gen_one("float", &mut rng, Locale::En, &Args::new(Some("5..5".into()))).unwrap(),
            json!(5.0)
        );
        // A real range stays inside the INCLUSIVE bounds across many draws.
        for _ in 0..200 {
            let n = gen_one("int", &mut rng, Locale::En, &Args::new(Some("10..20".into())))
                .unwrap()
                .as_i64()
                .unwrap();
            assert!((10..=20).contains(&n), "int escaped its range: {n}");
        }
        // bool is a JSON boolean, not a stringified one.
        assert!(gen_one("bool", &mut rng, Locale::En, &Args::default())
            .unwrap()
            .is_boolean());
    }
}
