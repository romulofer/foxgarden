use super::*;

#[test]
fn every_portuguese_variant_maps_to_pt_br() {
    for locale in ["pt_BR.UTF-8", "pt-BR", "pt", "pt_PT.UTF-8", "PT_br"] {
        assert_eq!(Lang::from_locale(locale), Lang::PtBr, "{locale}");
    }
}

#[test]
fn everything_else_maps_to_en_us() {
    for locale in ["en_US.UTF-8", "C", "POSIX", "es_ES", "", "de_DE@euro"] {
        assert_eq!(Lang::from_locale(locale), Lang::EnUs, "{locale}");
    }
}

#[test]
fn tags_round_trip() {
    for lang in Lang::ALL {
        assert_eq!(Lang::from_tag(lang.tag()), Some(lang));
    }
    assert_eq!(Lang::from_tag("klingon"), None);
}

#[test]
fn t_follows_set_lang() {
    with_lang(Lang::EnUs, || assert_eq!(t().menu.file, "File"));
    with_lang(Lang::PtBr, || assert_eq!(t().menu.file, "Arquivo"));
}

/// pt-BR is the primary language, so it's what the app shows before
/// anything has detected a locale or restored a setting.
#[test]
fn pt_br_is_the_default() {
    assert_eq!(Lang::default(), Lang::PtBr);
}
