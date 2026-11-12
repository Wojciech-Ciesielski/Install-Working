// Języki Shrimp Install (wszystkie z Windows 11) i Setup (wszystkie z GNOME).
//
// tag     nasz identyfikator: nazwa plików tłumaczeń (boxes/<box>/i18n/<tag>.txt), małe litery
// name    nazwa języka w tym języku (endonim), pokazywana na liście w Setup
// flags   w = język Windows (tłumaczymy Shrimp Install), g = język GNOME (tłumaczymy Setup), r = pismo od prawej do lewej
// bcp47   znacznik dla Intl i atrybutu lang, gdy inny niż tag
// intl    zapasowy znacznik dla Intl, gdy ICU nie zna tego języka (liczby, jednostki, nazwy krajów)
// match   dodatkowe znaczniki (małe litery), które wybierają ten język, np. „zh-tw” → zh-hant
// text    język zapasowy dla brakujących tekstów (zanim użyjemy angielskiego), np. keczua → hiszpański
export interface LangInfo {
  tag: string;
  name: string;
  rtl?: boolean;
  win?: boolean;
  gnome?: boolean;
  bcp47?: string;
  intl?: string;
  match?: string[];
  text?: string;
}

type Row = [tag: string, name: string, flags: string, extra?: { bcp47?: string; intl?: string; match?: string[]; text?: string }];

const ROWS: Row[] = [
  // — największe języki, w kolejności tłumaczenia —
  ["en", "English", "wg"], ["pl", "Polski", "wg"], ["de", "Deutsch", "wg"], ["fr", "Français", "wg"], ["es", "Español", "wg"],
  ["pt-br", "Português (Brasil)", "wg", { bcp47: "pt-BR", match: ["pt"] }], ["pt-pt", "Português (Portugal)", "wg", { bcp47: "pt-PT", match: ["pt-ao", "pt-mz", "pt-cv"] }],
  ["it", "Italiano", "wg"], ["ru", "Русский", "wg"], ["uk", "Українська", "wg"],
  ["zh-hans", "简体中文", "wg", { bcp47: "zh-Hans", match: ["zh", "zh-cn", "zh-sg"] }], ["zh-hant", "繁體中文", "wg", { bcp47: "zh-Hant", match: ["zh-tw", "zh-hk", "zh-mo"] }],
  ["ja", "日本語", "wg"], ["ko", "한국어", "wg"], ["tr", "Türkçe", "wg"], ["nl", "Nederlands", "wg"], ["cs", "Čeština", "wg"], ["sk", "Slovenčina", "wg"],
  ["sv", "Svenska", "wg"], ["nb", "Norsk bokmål", "wg", { match: ["no"] }], ["nn", "Norsk nynorsk", "wg", { text: "nb" }], ["da", "Dansk", "wg"], ["fi", "Suomi", "wg"],
  ["ar", "العربية", "wgr"], ["he", "עברית", "wgr", { match: ["iw"] }], ["fa", "فارسی", "wgr"],
  ["hi", "हिन्दी", "wg"], ["id", "Bahasa Indonesia", "wg", { match: ["in"] }], ["vi", "Tiếng Việt", "wg"], ["th", "ไทย", "wg"], ["ms", "Bahasa Melayu", "wg"],
  ["el", "Ελληνικά", "wg"], ["hu", "Magyar", "wg"], ["ro", "Română", "wg"], ["bg", "Български", "wg"], ["hr", "Hrvatski", "wg"], ["sl", "Slovenščina", "wg"],
  ["sr-cyrl", "Српски", "wg", { bcp47: "sr-Cyrl", match: ["sr"] }], ["sr-latn", "Srpski", "wg", { bcp47: "sr-Latn", match: ["sr-me"] }],
  ["lt", "Lietuvių", "wg"], ["lv", "Latviešu", "wg"], ["et", "Eesti", "wg"], ["is", "Íslenska", "wg"],
  ["ca", "Català", "wg"], ["eu", "Euskara", "wg"], ["gl", "Galego", "wg"], ["ca-valencia", "Valencià", "wg", { bcp47: "ca-ES-valencia", intl: "ca", match: ["ca-es-valencia"], text: "ca" }],
  ["bn", "বাংলা", "wg"], ["ta", "தமிழ்", "wg"], ["te", "తెలుగు", "wg"], ["mr", "मराठी", "wg"], ["gu", "ગુજરાતી", "wg"], ["kn", "ಕನ್ನಡ", "wg"], ["ml", "മലയാളം", "wg"],
  ["ur", "اردو", "wr"], ["fil", "Filipino", "w", { match: ["tl"] }],
  ["bs", "Bosanski", "wg", { match: ["bs-latn"] }], ["mk", "Македонски", "wg"], ["sq", "Shqip", "wg"], ["be", "Беларуская", "wg"], ["ka", "ქართული", "wg"], ["hy", "Հայերեն", "wg"],
  ["az", "Azərbaycan", "wg", { match: ["az-latn"] }], ["kk", "Қазақша", "wg"], ["uz", "Oʻzbekcha", "wg", { match: ["uz-latn"] }], ["ky", "Кыргызча", "wg"], ["tg", "Тоҷикӣ", "wg"],
  ["mn", "Монгол", "wg"], ["tk", "Türkmençe", "w"], ["tt", "Татарча", "w", { text: "ru" }], ["ug", "ئۇيغۇرچە", "wgr"],
  ["af", "Afrikaans", "wg"], ["sw", "Kiswahili", "wg"], ["am", "አማርኛ", "wg"], ["zu", "isiZulu", "wg"], ["xh", "isiXhosa", "wg"],
  ["km", "ខ្មែរ", "wg"], ["lo", "ລາວ", "wg"], ["si", "සිංහල", "wg"], ["ne", "नेपाली", "wg"], ["pa", "ਪੰਜਾਬੀ", "wg"], ["or", "ଓଡ଼ିଆ", "wg"], ["as", "অসমীয়া", "wg"],
  ["ga", "Gaeilge", "wg"], ["gd", "Gàidhlig", "wg"], ["cy", "Cymraeg", "wg"], ["mt", "Malti", "w"], ["lb", "Lëtzebuergesch", "w", { text: "de" }], ["mi", "Māori", "wg"],
  ["ckb", "کوردیی ناوەندی", "wgr", { match: ["ku-arab", "ku-arab-iq"] }], ["ku", "Kurdî", "g"], ["sd", "سنڌي", "wr", { match: ["sd-arab"] }],
  ["pa-arab", "پنجابی", "wr", { bcp47: "pa-Arab", match: ["pa-pk"] }], ["prs", "دری", "wr", { intl: "fa-AF", match: ["fa-af"], text: "fa" }],
  ["kok", "कोंकणी", "w", { text: "mr" }], ["nso", "Sesotho sa Leboa", "wg"], ["tn", "Setswana", "w", { text: "en" }], ["rw", "Kinyarwanda", "wg"], ["ti", "ትግርኛ", "w"],
  ["ha", "Hausa", "w"], ["ig", "Igbo", "w"], ["yo", "Yorùbá", "w"], ["wo", "Wolof", "w", { text: "fr" }],
  ["chr", "ᏣᎳᎩ", "w", { text: "en" }], ["iu", "Inuktitut", "w", { intl: "en-CA", text: "en" }], ["quz", "Runasimi", "w", { intl: "es-PE", text: "es" }], ["quc", "K’iche’", "w", { intl: "es-GT", match: ["qut", "qut-gt"], text: "es" }],
  // — tylko GNOME —
  ["ab", "Аҧсуа", "g", { intl: "ru", text: "ru" }], ["an", "Aragonés", "g", { intl: "es", text: "es" }], ["ast", "Asturianu", "g", { text: "es" }], ["be-latn", "Biełaruskaja", "g", { bcp47: "be-Latn", intl: "be", text: "be" }],
  ["br", "Brezhoneg", "g", { text: "fr" }], ["crh", "Qırımtatarca", "g", { intl: "tr", text: "tr" }], ["dz", "རྫོང་ཁ", "g"], ["eo", "Esperanto", "g"], ["fo", "Føroyskt", "g"], ["fur", "Furlan", "g", { text: "it" }],
  ["fy", "Frysk", "g", { text: "nl" }], ["ia", "Interlingua", "g"], ["ie", "Interlingue", "g", { intl: "en" }], ["kab", "Taqbaylit", "g", { text: "fr" }], ["kw", "Kernewek", "g"],
  ["mai", "मैथिली", "g", { intl: "hi", text: "hi" }], ["mg", "Malagasy", "g", { text: "fr" }], ["mjw", "Karbi", "g", { intl: "en-IN" }], ["my", "မြန်မာ", "g"], ["nds", "Plattdüütsch", "g", { text: "de" }],
  ["oc", "Occitan", "g", { text: "fr" }], ["os", "Ирон", "g", { text: "ru" }], ["sat", "ᱥᱟᱱᱛᱟᱲᱤ", "g", { text: "hi" }], ["uz-cyrl", "Ўзбекча", "g", { bcp47: "uz-Cyrl", match: ["uz-cyrl-uz"], text: "uz" }], ["wa", "Walon", "g", { intl: "fr", text: "fr" }],
];

export const LANGS: LangInfo[] = ROWS.map(([tag, name, flags, x]) => ({
  tag, name, rtl: flags.includes("r") || undefined, win: flags.includes("w") || undefined, gnome: flags.includes("g") || undefined, ...x,
}));

/**
 * Locale GNOME/glibc dla listy języków w Setup: [id locale (bez .UTF-8), tag tłumaczenia, nazwa, gdy inna niż endonim języka].
 * Sumę języków wyznaczają listy gnome-shell i gnome-control-center (po/LINGUAS), bez en@shaw. Agama pokazuje tylko te,
 * które zna system (glibc), więc wpisy bez locale w systemie po prostu znikają z listy.
 */
export const GNOME_LOCALES: [locale: string, tag: string, name?: string][] = [
  ["pl_PL", "pl"], ["en_US", "en"], ["en_GB", "en", "English (UK)"], ["en_CA", "en", "English (Canada)"], ["de_DE", "de"], ["es_ES", "es"], ["fr_FR", "fr"],
  ["it_IT", "it"], ["pt_PT", "pt-pt"], ["pt_BR", "pt-br"], ["nl_NL", "nl"], ["cs_CZ", "cs"], ["sk_SK", "sk"], ["uk_UA", "uk"], ["ru_RU", "ru"],
  ["sv_SE", "sv"], ["nb_NO", "nb"], ["nn_NO", "nn"], ["da_DK", "da"], ["fi_FI", "fi"], ["hu_HU", "hu"], ["lt_LT", "lt"], ["lv_LV", "lv"], ["et_EE", "et"],
  ["ro_RO", "ro"], ["bg_BG", "bg"], ["hr_HR", "hr"], ["sl_SI", "sl"], ["sr_RS", "sr-cyrl"], ["sr_RS@latin", "sr-latn"], ["bs_BA", "bs"], ["mk_MK", "mk"],
  ["sq_AL", "sq"], ["el_GR", "el"], ["tr_TR", "tr"], ["is_IS", "is"], ["ga_IE", "ga"], ["gd_GB", "gd"], ["cy_GB", "cy"], ["br_FR", "br"], ["kw_GB", "kw"],
  ["fy_NL", "fy"], ["fur_IT", "fur"], ["ca_ES", "ca"], ["ca_ES@valencia", "ca-valencia"], ["eu_ES", "eu"], ["gl_ES", "gl"], ["ast_ES", "ast"], ["an_ES", "an"],
  ["oc_FR", "oc"], ["wa_BE", "wa"], ["nds_DE", "nds"], ["eo", "eo"], ["ia_FR", "ia"], ["ie", "ie"], ["fo_FO", "fo"],
  ["ja_JP", "ja"], ["ko_KR", "ko"], ["zh_CN", "zh-hans"], ["zh_TW", "zh-hant", "中文（台灣）"], ["zh_HK", "zh-hant", "中文（香港）"],
  ["ar_EG", "ar"], ["he_IL", "he"], ["fa_IR", "fa"], ["ckb_IQ", "ckb"], ["ku_TR", "ku"], ["ug_CN", "ug"],
  ["hi_IN", "hi"], ["bn_BD", "bn"], ["bn_IN", "bn", "বাংলা (ভারত)"], ["as_IN", "as"], ["or_IN", "or"], ["gu_IN", "gu"], ["mr_IN", "mr"], ["pa_IN", "pa"],
  ["ta_IN", "ta"], ["te_IN", "te"], ["kn_IN", "kn"], ["ml_IN", "ml"], ["ne_NP", "ne"], ["mai_IN", "mai"], ["sat_IN", "sat"], ["si_LK", "si"], ["dz_BT", "dz"], ["mjw_IN", "mjw"],
  ["id_ID", "id"], ["ms_MY", "ms"], ["vi_VN", "vi"], ["th_TH", "th"], ["lo_LA", "lo"], ["km_KH", "km"], ["my_MM", "my"], ["mi_NZ", "mi"], ["mg_MG", "mg"],
  ["ka_GE", "ka"], ["hy_AM", "hy"], ["az_AZ", "az"], ["kk_KZ", "kk"], ["ky_KG", "ky"], ["uz_UZ", "uz"], ["uz_UZ@cyrillic", "uz-cyrl"], ["tg_TJ", "tg"],
  ["mn_MN", "mn"], ["be_BY", "be"], ["be_BY@latin", "be-latn"], ["os_RU", "os"], ["ab_GE", "ab"], ["crh_UA", "crh"],
  ["af_ZA", "af"], ["zu_ZA", "zu"], ["xh_ZA", "xh"], ["nso_ZA", "nso"], ["rw_RW", "rw"], ["sw_KE", "sw"], ["am_ET", "am"], ["kab_DZ", "kab"],
];
