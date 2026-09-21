use super::*;
use crate::{Lang, with_lang};

#[test]
fn interpolates_in_both_languages() {
    with_lang(Lang::PtBr, || {
        assert_eq!(failed_to_save("disco cheio"), "falha ao salvar: disco cheio");
    });
    with_lang(Lang::EnUs, || {
        assert_eq!(failed_to_save("disk full"), "failed to save: disk full");
    });
}

#[test]
fn a_message_with_two_holes_fills_both() {
    with_lang(Lang::EnUs, || {
        assert_eq!(
            failed_to_read("/tmp/a.java", "no such file"),
            "failed to read /tmp/a.java: no such file"
        );
    });
}

/// `install_pin_explanation` names the pinned version twice in both
/// languages — a rewording that drops one of them would leave a
/// dangling "will still install" with nothing after it.
#[test]
fn a_repeated_hole_is_filled_every_time() {
    for lang in Lang::ALL {
        with_lang(lang, || {
            assert_eq!(
                install_pin_explanation("10.20.1").matches("10.20.1").count(),
                2,
                "{lang:?}"
            );
        });
    }
}
