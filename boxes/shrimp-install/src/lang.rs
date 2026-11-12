//! Znacznik języka Shrimp (i18n/<znacznik>.txt, GRUB: themes/shrimp/lang/<znacznik>.cfg) z nazwy języka Windows (BCP-47).

pub fn tag(bcp: &str) -> String {
    let b = bcp.to_ascii_lowercase();
    let (lang, rest) = match b.find('-') {
        Some(i) => (&b[..i], &b[i..]),
        None => (b.as_str(), ""),
    };
    let has = |s: &str| rest.contains(s);
    let t = match lang {
        "zh" => if has("-hant") || has("-tw") || has("-hk") || has("-mo") { "zh-hant" } else { "zh-hans" },
        "pt" => if has("-pt") { "pt-pt" } else { "pt-br" },
        "sr" => if has("-latn") { "sr-latn" } else { "sr-cyrl" },
        "uz" if has("-cyrl") => "uz-cyrl",
        "be" if has("-latn") => "be-latn",
        "ca" if has("valencia") => "ca-valencia",
        "no" => "nb",
        "" => "en",
        l => l,
    };
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::tag;
    #[test]
    fn tags() {
        assert_eq!(tag("pl-PL"), "pl");
        assert_eq!(tag("zh-Hant-TW"), "zh-hant");
        assert_eq!(tag("zh-CN"), "zh-hans");
        assert_eq!(tag("pt-PT"), "pt-pt");
        assert_eq!(tag("pt-BR"), "pt-br");
        assert_eq!(tag("sr-Latn-RS"), "sr-latn");
        assert_eq!(tag("ca-ES-valencia"), "ca-valencia");
        assert_eq!(tag("no"), "nb");
        assert_eq!(tag(""), "en");
        assert_eq!(tag("EN-us"), "en");
    }
}
