//! Turkish data used by the detectors.
//!
//! Data only, written in folded form, like `ortu-normalize`'s lexicon
//! (ADR-0004). Tests check the form and the counts. Large curated datasets
//! (name gazetteers, provinces and districts) arrive in M4 as data files.

/// One cue rule as data; `lib.rs` turns it into an `ortu_core::CueRule`.
pub(crate) struct CueRuleData {
    /// The kind's `ortu.toml` name.
    pub(crate) kind: &'static str,
    /// Patterns in folded form; a trailing `*` makes the last word a prefix.
    pub(crate) patterns: &'static [&'static str],
    pub(crate) window_before: u16,
    pub(crate) window_after: u16,
    pub(crate) boost: f32,
    /// An assistant asking with this cue opens the caller's next turn.
    pub(crate) cross_turn: bool,
}

/// Cue rules. Patterns are matched on flattened stems, so `IBAN'ım`, `ibanım`
/// and `ıban` all match `iban` (algorithm 10).
pub(crate) const CUE_RULES: &[CueRuleData] = &[
    CueRuleData {
        kind: "tckn",
        patterns: &["tc", "t.c", "te ce", "tckn", "kimlik", "kimliğ", "nüfus"],
        window_before: 6,
        window_after: 6,
        boost: 2.0,
        cross_turn: true,
    },
    CueRuleData {
        kind: "iban",
        patterns: &["iban", "hesap numara*"],
        window_before: 6,
        window_after: 6,
        boost: 2.0,
        cross_turn: true,
    },
    CueRuleData {
        kind: "phone",
        patterns: &[
            "telefon*",
            "cep",
            "ceb",
            "gsm",
            "numara*",
            "ulaş*",
            "arayın",
            "aranacak*",
        ],
        window_before: 6,
        window_after: 6,
        boost: 2.0,
        cross_turn: true,
    },
    // Only an assistant question opens a caller's turn for names; within a
    // turn, the name detector's own patterns carry the evidence.
    CueRuleData {
        kind: "person_name",
        patterns: &["adınız*", "isminiz*", "ad soyad*", "adınızı soyadınızı"],
        window_before: 0,
        window_after: 0,
        boost: 1.0,
        cross_turn: true,
    },
];

/// Geographic area codes of Turkey: 81 provinces, two for Istanbul (212
/// European side, 216 Asian side).
///
/// Source: the national numbering plan that ICTA (BTK) notified to the ITU on
/// 2 March 2018 (ITU-T document T02020000D60002). On 2026-10-20 this list was
/// compared code by code with the plan's geographic NDCs: 82 codes, no
/// difference. 392 (Northern Cyprus) is reachable through +90 but is not part
/// of Turkey's plan and is not listed. Re-check when BTK publishes a newer plan.
pub(crate) const AREA_CODES: &[u16] = &[
    212, 216, 222, 224, 226, 228, 232, 236, 242, 246, 248, 252, 256, 258, 262, 264, 266, 272, 274,
    276, 282, 284, 286, 288, 312, 318, 322, 324, 326, 328, 332, 338, 342, 344, 346, 348, 352, 354,
    356, 358, 362, 364, 366, 368, 370, 372, 374, 376, 378, 380, 382, 384, 386, 388, 412, 414, 416,
    422, 424, 426, 428, 432, 434, 436, 438, 442, 446, 452, 454, 456, 458, 462, 464, 466, 472, 474,
    476, 478, 482, 484, 486, 488,
];

/// Words that introduce a first name or a full name: `adım Ahmet Yılmaz`.
/// `ben X` is deliberately absent until the name gazetteer exists (M4):
/// without it, `ben belediyeden arıyorum` would mask `belediyeden`.
pub(crate) const NAME_INTRODUCERS: &[&str] = &["adım", "ismim"];

/// Words that introduce a surname: `soyadım Yılmaz`.
pub(crate) const SURNAME_INTRODUCERS: &[&str] = &["soyadım", "soyismim"];

/// Two-word introducers of a full name: `adım soyadım Ahmet Yılmaz`.
pub(crate) const FULL_NAME_INTRODUCERS: &[&[&str]] = &[&["adım", "soyadım"], &["ad", "soyad"]];

/// Forms of address that precede a name: `Sayın Ahmet Yılmaz`. Assistants
/// use them when they read a name back, so they matter for persisted
/// transcripts.
pub(crate) const ADDRESS_TITLES: &[&str] = &["sayın"];

/// Honorifics that follow a name: `Ahmet Bey`, `Ayşe Hanım`.
pub(crate) const HONORIFICS: &[&str] = &["bey", "hanım", "beyefendi", "hanımefendi"];

/// Words that follow a name introducer without being a name (`ben de`,
/// `adım adım`, `ben şimdi`). Names that are also common words (`Umut`,
/// `Deniz`) are not listed: after an introducer they are names.
pub(crate) const NOT_NAMES: &[&str] = &[
    "adım",
    "ismim",
    "soyadım",
    "ben",
    "sen",
    "biz",
    "siz",
    "o",
    "bu",
    "şu",
    "bir",
    "de",
    "da",
    "ve",
    "ile",
    "ki",
    "mi",
    "mı",
    "mu",
    "mü",
    "ne",
    "şey",
    "şimdi",
    "sadece",
    "yalnız",
    "zaten",
    "aslında",
    "galiba",
    "sanırım",
    "evet",
    "hayır",
    "tamam",
    "peki",
    "yani",
    "işte",
    "çok",
    "daha",
    "en",
    "hiç",
    "her",
    "hep",
    "yine",
    "bile",
    "artık",
    "hemen",
    "şöyle",
    "böyle",
    "öyle",
    "burada",
    "orada",
    "size",
    "sizi",
    "sizin",
    "sana",
    "seni",
    "senin",
    "onu",
    "ona",
    "onun",
    "bana",
    "beni",
    "benim",
    "efendim",
    "merhaba",
    "iyi",
    "günler",
    "akşamlar",
    "teşekkür",
    "teşekkürler",
    "sağ",
    "ol",
    "olun",
    "değil",
    "var",
    "yok",
    "için",
    "gibi",
    "kadar",
    "ama",
    "fakat",
    "ancak",
    "çünkü",
    "bey",
    "hanım",
    "belediye",
    "müşteri",
    "vatandaş",
    "abone",
    // Addressees after `Sayın` that are not names: `Sayın müşterimiz`.
    "müşterimiz",
    "vatandaşımız",
    "abonemiz",
    "üyemiz",
    "kullanıcımız",
    "yetkili",
    "ilgili",
    // Titles that take an honorific without being a name: `Müdür Bey`.
    "müdür",
    "başkan",
    "doktor",
    "hoca",
    "avukat",
    "memur",
    "amir",
    "şef",
    "usta",
    "öğretmen",
];

/// Endings of finite verbs (`arıyorum`, `soracaktım`): `ben arıyorum` is not a
/// name.
pub(crate) const VERB_ENDINGS: &[&str] = &[
    "yor",
    "yorum",
    "yoruz",
    "yorsun",
    "yorsunuz",
    "yorlar",
    "yordum",
    "yorduk",
    "acağım",
    "eceğim",
    "acağız",
    "eceğiz",
    "acaktım",
    "ecektim",
    "mıştım",
    "miştim",
    "muştum",
    "müştüm",
    "abilirim",
    "ebilirim",
];

/// Field names in tool arguments and results that hold a kind of personal
/// data, mapped to `ortu.toml` kind names. Keys are `snake_case` in flattened
/// form (`Müşteri Adı` and `musteriAdi` both become `musteri_adi`); English
/// and Turkish spellings are listed because tool schemas use both.
pub(crate) const FIELD_NAMES: &[(&str, &str)] = &[
    ("tckn", "tckn"),
    ("tc", "tckn"),
    ("tc_no", "tckn"),
    ("tc_kimlik", "tckn"),
    ("tc_kimlik_no", "tckn"),
    ("kimlik_no", "tckn"),
    ("kimlik_numarasi", "tckn"),
    ("national_id", "tckn"),
    ("identity_number", "tckn"),
    ("iban", "iban"),
    ("iban_no", "iban"),
    ("phone", "phone"),
    ("phone_number", "phone"),
    ("telefon", "phone"),
    ("telefon_no", "phone"),
    ("tel", "phone"),
    ("gsm", "phone"),
    ("cep", "phone"),
    ("cep_telefonu", "phone"),
    ("mobile", "phone"),
    ("msisdn", "phone"),
    ("caller_id", "phone"),
    ("name", "full_name"),
    ("full_name", "full_name"),
    ("fullname", "full_name"),
    ("customer_name", "full_name"),
    ("ad_soyad", "full_name"),
    ("adsoyad", "full_name"),
    ("isim", "full_name"),
    ("isim_soyisim", "full_name"),
    ("musteri_adi", "full_name"),
    ("first_name", "person_name"),
    ("firstname", "person_name"),
    ("given_name", "person_name"),
    ("ad", "person_name"),
    ("adi", "person_name"),
    ("last_name", "person_surname"),
    ("lastname", "person_surname"),
    ("surname", "person_surname"),
    ("family_name", "person_surname"),
    ("soyad", "person_surname"),
    ("soyadi", "person_surname"),
    ("soyisim", "person_surname"),
    ("email", "email"),
    ("e_mail", "email"),
    ("mail", "email"),
    ("eposta", "email"),
    ("e_posta", "email"),
    ("address", "address"),
    ("adres", "address"),
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ortu_normalize::turkish_fold;

    use super::{
        ADDRESS_TITLES, AREA_CODES, CUE_RULES, FIELD_NAMES, FULL_NAME_INTRODUCERS, HONORIFICS,
        NAME_INTRODUCERS, NOT_NAMES, SURNAME_INTRODUCERS, VERB_ENDINGS,
    };

    #[test]
    fn area_codes_cover_81_provinces_without_duplicates() {
        let unique: HashSet<_> = AREA_CODES.iter().collect();
        assert_eq!(AREA_CODES.len(), 82, "81 provinces, Istanbul has two codes");
        assert_eq!(unique.len(), AREA_CODES.len());
        assert!(AREA_CODES.iter().all(|code| (200..500).contains(code)));
    }

    #[test]
    fn every_word_is_written_in_folded_form() {
        let words = CUE_RULES
            .iter()
            .flat_map(|rule| rule.patterns.iter())
            .flat_map(|pattern| pattern.trim_end_matches('*').split_whitespace())
            .chain(NAME_INTRODUCERS.iter().copied())
            .chain(SURNAME_INTRODUCERS.iter().copied())
            .chain(FULL_NAME_INTRODUCERS.iter().flat_map(|p| p.iter().copied()))
            .chain(HONORIFICS.iter().copied())
            .chain(ADDRESS_TITLES.iter().copied())
            .chain(NOT_NAMES.iter().copied())
            .chain(VERB_ENDINGS.iter().copied());
        for word in words {
            assert_eq!(turkish_fold(word), word, "`{word}` is not folded");
        }
    }

    #[test]
    fn field_names_are_flattened_snake_case_and_unique() {
        let keys: HashSet<_> = FIELD_NAMES.iter().map(|(key, _)| key).collect();
        assert_eq!(keys.len(), FIELD_NAMES.len());
        for (key, kind) in FIELD_NAMES {
            assert!(
                key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{key}"
            );
            assert!(
                ortu_core::EntityKind::from_config_name(kind).is_some(),
                "{kind}"
            );
        }
    }

    #[test]
    fn cue_rule_kinds_are_known() {
        for rule in CUE_RULES {
            assert!(
                ortu_core::EntityKind::from_config_name(rule.kind).is_some(),
                "{}",
                rule.kind
            );
        }
    }
}
